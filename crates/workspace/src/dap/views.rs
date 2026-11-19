//! The debugger's views as the frontends use them: the call stack, the
//! variables and watch trees, the console, and the commands that step the
//! program. They follow the views of `src/vs/workbench/contrib/debug/browser/`
//! (`callStackView.ts`, `variablesView.ts:110-112` for the first scope that
//! opens by itself, `watchExpressionsView.ts`, `repl.ts`) and the commands
//! of `debugCommands.ts:500-560`, reimplemented without their UI.

use super::model::{
    self, ConsoleKind, ConsoleLine, DebugTree, FrameInfo, Node, ThreadInfo, VariableRow,
};
use super::session::Pending;
use super::{DebugError, DebugState, StepKind};
use crate::{Event, Workspace};
use rustode_config::{ContextValue, parse_when};
use serde_json::json;

impl Workspace {
    /// The program's threads, from the adapter.
    #[must_use]
    pub fn debug_threads(&self) -> Vec<ThreadInfo> {
        self.dap
            .session
            .as_ref()
            .map_or_else(Vec::new, |s| s.threads.clone())
    }

    /// The call stack of the stopped thread, innermost frame first.
    #[must_use]
    pub fn debug_frames(&self) -> Vec<FrameInfo> {
        self.dap
            .session
            .as_ref()
            .map_or_else(Vec::new, |s| s.frames.clone())
    }

    /// The frame whose variables and watch values are shown: an index into
    /// [`Workspace::debug_frames`].
    #[must_use]
    pub fn debug_selected_frame(&self) -> usize {
        self.dap.session.as_ref().map_or(0, |s| s.selected)
    }

    /// Selects a frame: the editor goes to its source line, and the
    /// variables and watch expressions are read again for it.
    ///
    /// # Errors
    /// When the program is not stopped or there is no such frame.
    pub fn select_debug_frame(&mut self, index: usize) -> Result<(), DebugError> {
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        if session.state != DebugState::Stopped {
            return Err(DebugError::NotStopped);
        }
        if index >= session.frames.len() {
            return Err(DebugError::NoSuchRow);
        }
        session.invalidate_frame();
        session.selected = index;
        self.dap_frame_changed();
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// The visible rows of the variables or the watch expressions.
    #[must_use]
    pub fn debug_rows(&self, tree: DebugTree) -> Vec<VariableRow> {
        match tree {
            DebugTree::Variables => self
                .dap
                .session
                .as_ref()
                .map_or_else(Vec::new, |s| model::rows(&s.scopes)),
            DebugTree::Watch => model::rows(&self.dap.watches),
        }
    }

    /// Expands or collapses the row; an expansion asks the adapter for the
    /// children the first time, and they show once they arrive.
    ///
    /// # Errors
    /// When there is no such row, or its children can only be asked for
    /// while the program is stopped, or the adapter has too many requests
    /// waiting.
    pub fn toggle_debug_row(&mut self, tree: DebugTree, row: usize) -> Result<(), DebugError> {
        let stopped = self
            .dap
            .session
            .as_ref()
            .is_some_and(|s| s.state == DebugState::Stopped);
        let room = self
            .dap
            .session
            .as_ref()
            .is_some_and(super::session::Session::has_room);
        let nodes: &mut [Node] = match tree {
            DebugTree::Variables => match self.dap.session.as_mut() {
                Some(session) => &mut session.scopes,
                None => return Err(DebugError::NotRunning),
            },
            DebugTree::Watch => &mut self.dap.watches,
        };
        let (node, depth) = model::at(nodes, row).ok_or(DebugError::NoSuchRow)?;
        let mut ask = None;
        if node.expanded {
            node.expanded = false;
        } else if node.children.as_ref().is_some_and(|c| !c.is_empty()) {
            node.expanded = true;
        } else if node.children.is_none() && node.reference > 0 && depth < model::MAX_DEPTH {
            if !stopped {
                return Err(DebugError::NotStopped);
            }
            if !room {
                return Err(DebugError::Busy);
            }
            node.expanded = true;
            node.loading = true;
            ask = Some(node.reference);
        }
        if let (Some(reference), Some(session)) = (ask, self.dap.session.as_mut()) {
            session.request_state(
                "variables",
                &json!({"variablesReference": reference}),
                Pending::Variables(reference),
            );
        }
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// Adds a watch expression, evaluated at once when the program is
    /// stopped (`watchExpressionsView.ts:612-635`).
    ///
    /// # Errors
    /// When the expression is empty or there are too many.
    pub fn add_watch(&mut self, expression: &str) -> Result<(), DebugError> {
        let expression = expression.trim();
        if expression.is_empty() {
            return Err(DebugError::BadWatch(
                "a watch expression cannot be empty".to_owned(),
            ));
        }
        if self.dap.watches.len() >= model::MAX_WATCHES {
            return Err(DebugError::BadWatch(format!(
                "there can be at most {} watch expressions",
                model::MAX_WATCHES
            )));
        }
        self.dap.next_watch += 1;
        let id = self.dap.next_watch;
        self.dap.watches.push(Node::watch(id, expression));
        if let Some(session) = self.dap.session.as_mut()
            && session.state == DebugState::Stopped
            && let Some(frame) = session.frame_id()
        {
            session.request_state(
                "evaluate",
                &json!({"expression": expression, "frameId": frame, "context": "watch"}),
                Pending::Watch(id),
            );
        }
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// Removes the watch expression at `index` of the watch rows' `root`;
    /// returns whether there was one.
    pub fn remove_watch(&mut self, root: usize) -> bool {
        if root >= self.dap.watches.len() {
            return false;
        }
        self.dap.watches.remove(root);
        self.events.push(Event::DebugChanged);
        true
    }

    /// Removes every watch expression
    /// (`workbench.debug.viewlet.action.removeAllWatchExpressions`).
    pub fn remove_all_watches(&mut self) {
        self.dap.watches.clear();
        self.events.push(Event::DebugChanged);
    }

    /// Evaluates an expression typed in the debug console (context
    /// `repl`); the result, or the adapter's complaint, is a console line
    /// when it arrives. Works while the program runs too.
    ///
    /// # Errors
    /// When there is no session, or the adapter has too many requests
    /// waiting.
    pub fn evaluate_in_console(&mut self, expression: &str) -> Result<(), DebugError> {
        let expression = expression.trim();
        if expression.is_empty() {
            return Ok(());
        }
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        let mut arguments = json!({"expression": expression, "context": "repl"});
        if session.state == DebugState::Stopped
            && let Some(frame) = session.frame_id()
        {
            arguments["frameId"] = json!(frame);
        }
        if !session.request_state("evaluate", &arguments, Pending::Repl) {
            return Err(DebugError::Busy);
        }
        self.dap_console(ConsoleKind::Input, expression);
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// The debug console's lines, oldest first.
    #[must_use]
    pub fn debug_console(&self) -> &[ConsoleLine] {
        &self.dap.console
    }

    /// Whether a debug command's `when` clause holds now, so a frontend
    /// knows what a key means: F6 pauses while the program runs
    /// (`debugCommands.ts:550-558`) and moves the focus otherwise. A command
    /// with no clause always applies.
    #[must_use]
    pub fn command_applies(&self, id: &str) -> bool {
        let Some(clause) = crate::COMMANDS
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.when)
        else {
            return true;
        };
        let state = self.debug_status().state;
        let frames = self.dap.session.as_ref().map_or(0, |s| s.frames.len());
        let context = |key: &str| match key {
            "debugState" => ContextValue::Str(state.label().to_owned()),
            "inDebugMode" => ContextValue::Bool(state != DebugState::Inactive),
            "threadHasMultipleStackFrames" => ContextValue::Bool(frames > 1),
            "activeEditorGroupEmpty" => ContextValue::Bool(self.active_document().is_none()),
            "multipleEditorGroups" => ContextValue::Bool(self.group_ids().len() > 1),
            _ => ContextValue::Undefined,
        };
        parse_when(clause).is_ok_and(|when| when.evaluate(&context))
    }

    /// Steps the stopped program: over the line, into the call, or out of
    /// the frame.
    ///
    /// # Errors
    /// When there is no session or the program is not stopped.
    pub fn step_debugging(&mut self, kind: StepKind) -> Result<(), DebugError> {
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        if !session.can_resume() {
            return Err(DebugError::NotStopped);
        }
        session.step(kind);
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// Asks the running program to stop where it is.
    ///
    /// # Errors
    /// When there is no session or the program is not running.
    pub fn pause_debugging(&mut self) -> Result<(), DebugError> {
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        if session.state != DebugState::Running {
            return Err(DebugError::NotRunningProgram);
        }
        session.pause();
        Ok(())
    }
}
