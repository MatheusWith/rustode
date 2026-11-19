//! Go to definition, find references, rename and format in the terminal.
//! What they do is the workspace's (`rustode_workspace`'s `request_definition`,
//! `request_references`, `rename_symbol`, `format_document`, `locations`); this
//! module maps keys and draws a list in the quick input box.
//!
//! Keys, as in VS Code (`src/vs/editor/contrib/gotoSymbol/browser/goToCommands.ts:276-296`,
//! `:669-681`; `src/vs/editor/contrib/rename/browser/rename.ts:365-370`;
//! `src/vs/editor/contrib/format/browser/formatActions.ts:218-224`): F12 goes to
//! the definition, Shift+F12 lists the references, F2 renames and Ctrl+Shift+I
//! formats the document. A terminal without an extended keyboard protocol
//! sends Ctrl+Shift+I as Tab; the palette runs them all too.
//!
//! One location is gone to; several are listed in the quick input box (VS
//! Code opens a peek widget), where typing narrows the list by file or text
//! and Enter goes to the selected one. F2 opens the same box with the word
//! under the cursor, to type the new name in; Enter renames. What a request
//! did or why it did nothing shows in the status bar.

use crate::app::{App, Focus, QuickInput, QuickItem};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustode_workspace::SymbolLocation;

impl App {
    /// Handles the keys of these commands; returns whether `key` was theirs.
    pub(crate) fn navigate_key(&mut self, key: KeyEvent) -> bool {
        let m = key.modifiers;
        let (ctrl, shift) = (
            m.contains(KeyModifiers::CONTROL),
            m.contains(KeyModifiers::SHIFT),
        );
        let alt = m.contains(KeyModifiers::ALT);
        let id = match key.code {
            KeyCode::F(12) if !ctrl && !alt && !shift => "editor.action.revealDefinition",
            KeyCode::F(12) if !ctrl && !alt && shift => "editor.action.goToReferences",
            KeyCode::F(2) if !ctrl && !alt && !shift => "editor.action.rename",
            KeyCode::Char('i' | 'I') if ctrl && shift && !alt => "editor.action.formatDocument",
            _ => return false,
        };
        self.message = None;
        self.run_language(id)
    }

    /// Runs one of the four language commands by id; returns whether `id`
    /// is one.
    pub(crate) fn run_language(&mut self, id: &str) -> bool {
        if !matches!(
            id,
            "editor.action.revealDefinition"
                | "editor.action.goToReferences"
                | "editor.action.rename"
                | "editor.action.formatDocument"
        ) {
            return false;
        }
        if id == "editor.action.rename" {
            self.open_rename();
            return true;
        }
        match self.workspace.execute(id) {
            Ok(true) => {}
            Ok(false) => {
                self.message = Some("No language server can do that for this document".to_owned());
            }
            Err(err) => self.message = Some(err.to_string()),
        }
        true
    }

    /// Opens the quick input box on the word under the cursor, to type the
    /// new name in.
    fn open_rename(&mut self) {
        let Some(id) = self.document() else {
            self.message = Some("No text editor active at this time".to_owned());
            return;
        };
        match self.workspace.rename_target(id) {
            Ok(Some(target)) => {
                self.quick = Some(QuickInput {
                    text: target.text,
                    rename: Some(id),
                    ..QuickInput::default()
                });
            }
            Ok(None) => {
                self.message = Some("No language server can rename here".to_owned());
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// Renames the symbol at the cursor of `id` to `name`.
    pub(crate) fn rename_to(&mut self, name: &str) {
        let Some(id) = self.document() else { return };
        match self.workspace.rename_symbol(id, name) {
            Ok(true) => {}
            Ok(false) => self.message = Some("Choose another name to rename to".to_owned()),
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// The locations the workspace found arrive: lists them in the quick
    /// input box.
    pub(crate) fn show_locations(&mut self) {
        let Some(list) = self.workspace.locations() else {
            if self.quick.as_ref().is_some_and(|q| q.locations) {
                self.quick = None;
            }
            return;
        };
        self.message = Some(format!(
            "{} of '{}': {}",
            list.kind.title(),
            list.word,
            list.locations.len()
        ));
        self.quick = Some(QuickInput {
            locations: true,
            ..QuickInput::default()
        });
    }

    /// The rows of the quick input box when it lists locations, narrowed by
    /// what was typed; or when it asks for a name.
    pub(crate) fn navigate_items(&self, query: &str) -> Option<Vec<QuickItem>> {
        let quick = self.quick.as_ref()?;
        if quick.rename.is_some() {
            return Some(vec![QuickItem::Hint(format!(
                "Press Enter to rename to '{}', Escape to cancel",
                query.trim()
            ))]);
        }
        if !quick.locations {
            return None;
        }
        let list = self.workspace.locations()?;
        Some(
            list.filtered(query)
                .into_iter()
                .map(|(index, location)| QuickItem::Location(index, location.clone()))
                .collect(),
        )
    }

    /// Goes to the `index`th location of the workspace's list.
    pub(crate) fn pick_location(&mut self, index: usize) {
        match self.workspace.pick_location(index) {
            Ok(_) => self.focus = Focus::Editor,
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// The quick input box closed without a pick.
    pub(crate) fn close_locations(&mut self) {
        self.workspace.dismiss_locations();
    }
}

/// How a location shows: its file and line, e.g. `main.rs:12`.
pub(crate) fn place(location: &SymbolLocation) -> String {
    let name = location
        .path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    format!("{name}:{}", location.start.line + 1)
}
