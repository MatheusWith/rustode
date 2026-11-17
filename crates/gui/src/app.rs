//! The window: the sidebar on the left (the explorer, or the Extensions
//! view after Ctrl+Shift+X, see `extensions.rs`), the tab strip and the
//! editor on the right, the status bar along the bottom, and the title.
//! Everything is drawn from the workspace's state; the window keeps only
//! what is on screen (scroll positions, the selected explorer row, focus).
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - the title follows VS Code's default,
//!   `${dirty}${activeEditorShort}${separator}${rootName}${separator}${appName}`,
//!   with `● ` for unsaved changes and ` - ` between the parts, an empty part
//!   dropped with its separator
//!   (`src/vs/workbench/browser/parts/titlebar/windowTitle.ts:41-53`, `:59`;
//!   `src/vs/base/common/labels.ts:347-410`);
//! - the sidebar starts `min(300, width / 4)` points wide, and at least 170
//!   (`src/vs/workbench/browser/layout.ts:2908`, `:3055`;
//!   `src/vs/workbench/browser/parts/sidebar/sidebarPart.ts:50`); hiding it
//!   hands its focus to the editor (`layout.ts:1993-1998`).
//!
//! As in the TUI, closing a tab with unsaved changes asks first: a second
//! Ctrl+W, or a second click on its close button, discards them.

use crate::keys::{self, Workbench};
use crate::latency::PresentLatency;
use crate::status::STATUS_BAR_HEIGHT;
use crate::tabs::{self, TAB_HEIGHT};
use crate::{editor, explorer, extensions, status};
use eframe::egui::{
    self, Color32, Context, FontId, Label, Rect, RichText, UiBuilder, Vec2, ViewportCommand,
};
use rustode_workspace::{
    Document, DocumentId, Error, Event, ExtensionEvent, NOT_LOADED, Workspace,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The status bar prompt after closing a tab with unsaved changes.
const CLOSE_PROMPT: &str = "Unsaved changes: Ctrl+W again closes without saving, Ctrl+S saves";
const EDITOR_BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const HINT: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);

/// The editor window: open documents in tabs and an optional folder in the
/// explorer.
pub struct EditorApp {
    workspace: Workspace,
    /// Where each document is scrolled to, kept when switching tabs.
    views: HashMap<DocumentId, editor::View>,
    explorer: explorer::View,
    strip: tabs::Strip,
    /// Whether the sidebar is shown (Ctrl+B).
    sidebar: bool,
    /// The sidebar's width, set on the first frame.
    sidebar_width: Option<f32>,
    /// A prompt or a failure of the last command; the next key clears it.
    message: Option<String>,
    /// Why the last save or edit failed; shown until a save succeeds.
    error: Option<String>,
    /// The latest notice about a file on disk.
    notice: Option<String>,
    /// The tab whose unsaved changes the next close discards.
    confirm_close: Option<DocumentId>,
    /// The title last sent to the window.
    title: Option<String>,
    /// Set when the window runs under [`crate::run`], to time frames.
    latency: Option<Rc<RefCell<PresentLatency>>>,
    /// The Extensions view, shown in the sidebar instead of the explorer.
    extensions: extensions::Panel,
    /// Whether the workspace wakes this window when extension results arrive.
    waker_set: bool,
}

/// Where each part goes this frame.
struct Areas {
    sidebar: Option<Rect>,
    tabs: Rect,
    editor: Rect,
    status: Rect,
}

fn areas(rect: Rect, sidebar: Option<f32>) -> Areas {
    let (body, status) = rect.split_top_bottom_at_y(rect.bottom() - STATUS_BAR_HEIGHT);
    let (sidebar, group) = match sidebar {
        Some(width) => {
            let (sidebar, group) = body.split_left_right_at_x(body.left() + width);
            (Some(sidebar), group)
        }
        None => (None, body),
    };
    let (tabs, editor) = group.split_top_bottom_at_y(group.top() + TAB_HEIGHT);
    Areas {
        sidebar,
        tabs,
        editor,
        status,
    }
}

/// VS Code's first sidebar width for a window `width` points wide (never
/// more than half the window until the width breakpoints arrive).
fn sidebar_width(width: f32) -> f32 {
    (width / 4.0).clamp(170.0, 300.0).min(width / 2.0)
}

fn file_name(document: &Document) -> String {
    document
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether the explorer or the Extensions view's search box has focus.
fn sidebar_has_focus(ctx: &Context) -> bool {
    ctx.memory(|memory| {
        memory.has_focus(explorer::id()) || memory.has_focus(extensions::search_id())
    })
}

/// Gives keyboard focus to `id`, and draws once more so the widget can lock
/// its keys before the next one arrives.
fn focus(ctx: &Context, id: egui::Id) {
    ctx.memory_mut(|memory| memory.request_focus(id));
    ctx.request_repaint();
}

impl EditorApp {
    /// Opens a file in a tab, or a folder in the explorer.
    ///
    /// # Errors
    /// When the path cannot be read.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_paths(&[path.to_owned()])
    }

    /// Opens files in tabs and the first folder in the explorer, as the TUI
    /// does.
    ///
    /// # Errors
    /// When a path cannot be read.
    pub fn open_paths(paths: &[PathBuf]) -> Result<Self, Error> {
        Self::open_paths_in(Workspace::new(), paths)
    }

    /// [`Self::open_paths`] in a workspace set up by the caller, for
    /// example with its extensions folder.
    ///
    /// # Errors
    /// When a path cannot be read.
    pub fn open_paths_in(mut workspace: Workspace, paths: &[PathBuf]) -> Result<Self, Error> {
        for path in paths {
            if path.is_dir() {
                if workspace.explorer().is_none() {
                    workspace.open_folder(path)?;
                }
            } else {
                workspace.open(path)?;
            }
        }
        Ok(Self::new(workspace))
    }

    /// The window for `workspace` and the documents already open in it.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            views: HashMap::new(),
            explorer: explorer::View::default(),
            strip: tabs::Strip::default(),
            sidebar: true,
            sidebar_width: None,
            message: None,
            error: None,
            notice: None,
            confirm_close: None,
            title: None,
            latency: None,
            extensions: extensions::Panel::default(),
            waker_set: false,
        }
    }

    /// The workspace holding the documents.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The window title: `● ` while the active document has unsaved
    /// changes, then its file name, the folder's name and the app name.
    #[must_use]
    pub fn window_title(&self) -> String {
        let active = self
            .workspace
            .active_document()
            .and_then(|id| self.workspace.document(id));
        let mark = if active.is_some_and(Document::is_dirty) {
            "\u{25cf} "
        } else {
            ""
        };
        let root = self
            .workspace
            .explorer()
            .and_then(|explorer| explorer.root().file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let parts: Vec<String> = [active.map(file_name), root, Some("rustode".to_owned())]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect();
        format!("{mark}{}", parts.join(" - "))
    }

    /// Reports each frame to `latency`, telling whether its input changed
    /// the window; [`crate::run`] does this for its window.
    pub fn time_frames(&mut self, latency: Rc<RefCell<PresentLatency>>) {
        self.latency = Some(latency);
    }

    /// Turns workspace events about files into status bar notices.
    fn note(&mut self) {
        for event in self.workspace.take_events() {
            self.notice = match event {
                Event::Conflict { .. } => {
                    Some("The file changed on disk; the unsaved changes are kept.".to_owned())
                }
                Event::Deleted { .. } => Some("The file was deleted from disk.".to_owned()),
                Event::Renamed { to, .. } => {
                    Some(format!("The file was moved to {}.", to.display()))
                }
                Event::Restored { .. } | Event::Reloaded { .. } => None,
                Event::Extension(event) => match extension_notice(&event) {
                    Some(notice) => Some(notice),
                    None => continue,
                },
                _ => continue,
            };
        }
    }

    /// Asks the workspace to repaint the window when extension results
    /// arrive from its background threads.
    fn wake_on_results(&mut self, ctx: &Context) {
        if !self.waker_set {
            let ctx = ctx.clone();
            self.workspace
                .set_extensions_waker(move || ctx.request_repaint());
            self.waker_set = true;
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = self.window_title();
        if self.title.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = Some(title);
        }
    }

    /// Takes the workbench keys out of this frame's input and runs them.
    /// Any other key clears the last message and the pending close.
    fn workbench_keys(&mut self, ctx: &Context) -> bool {
        let (commands, typed) = ctx.input_mut(|input| {
            let mut commands = Vec::new();
            input.events.retain(|event| match keys::workbench(event) {
                Some(command) => {
                    commands.push(command);
                    false
                }
                None => true,
            });
            let typed = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                )
            });
            (commands, typed)
        });
        if typed || !commands.is_empty() {
            self.message = None;
        }
        if typed
            || commands
                .iter()
                .any(|&command| command != Workbench::CloseTab)
        {
            self.confirm_close = None;
        }
        for &command in &commands {
            self.run_workbench(ctx, command);
        }
        !commands.is_empty()
    }

    fn run_workbench(&mut self, ctx: &Context, command: Workbench) {
        match command {
            Workbench::Save => self.save(),
            Workbench::NextTab => self.switch(ctx, Workspace::next_tab),
            Workbench::PreviousTab => self.switch(ctx, Workspace::previous_tab),
            Workbench::RecentTab => self.switch(ctx, Workspace::recently_used_tab),
            Workbench::LeastRecentTab => self.switch(ctx, Workspace::least_recently_used_tab),
            Workbench::CloseTab => {
                if let Some(id) = self.workspace.active_document() {
                    self.close(ctx, id);
                }
            }
            Workbench::ToggleSidebar => self.toggle_sidebar(ctx),
            Workbench::FocusExplorer => self.focus_explorer(ctx),
            Workbench::FocusNextPart => self.focus_next_part(ctx),
            Workbench::ShowExtensions => self.show_extensions(ctx),
        }
    }

    /// A tab switch, which also focuses the editor.
    fn switch(&mut self, ctx: &Context, switch: fn(&mut Workspace)) {
        switch(&mut self.workspace);
        self.focus_editor(ctx);
    }

    fn focus_editor(&self, ctx: &Context) {
        if self.workspace.active_document().is_some() {
            focus(ctx, editor::id());
        }
    }

    fn save(&mut self) {
        let Some(id) = self.workspace.active_document() else {
            return;
        };
        match self.workspace.save(id) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("Save failed: {err}")),
        }
    }

    /// Closes `id`'s tab; with unsaved changes, only when asked twice.
    fn close(&mut self, ctx: &Context, id: DocumentId) {
        let discard = self.confirm_close == Some(id);
        match self.workspace.close(id, discard) {
            Ok(()) => {
                self.confirm_close = None;
                self.message = None;
                self.views.remove(&id);
                if self.workspace.active_document().is_some() {
                    self.focus_editor(ctx);
                } else if self.sidebar && self.workspace.explorer().is_some() {
                    focus(ctx, explorer::id());
                }
            }
            Err(Error::Dirty(_)) => {
                let _ = self.workspace.activate(id);
                self.confirm_close = Some(id);
                self.message = Some(CLOSE_PROMPT.to_owned());
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    fn toggle_sidebar(&mut self, ctx: &Context) {
        self.sidebar = !self.sidebar;
        if !self.sidebar && sidebar_has_focus(ctx) {
            self.focus_editor(ctx);
        }
    }

    /// Ctrl+Shift+X: shows the sidebar with the Extensions view and focuses
    /// its search box, or, from the search box, goes back to the editor.
    fn show_extensions(&mut self, ctx: &Context) {
        self.sidebar = true;
        self.notice = self.extensions.toggle(ctx, &mut self.workspace);
    }

    /// Ctrl+Shift+E: shows the sidebar and focuses the explorer on the
    /// active file's row.
    fn focus_explorer(&mut self, ctx: &Context) {
        self.sidebar = true;
        self.extensions.shown = false;
        if self.workspace.explorer().is_none() {
            self.message = Some("No folder opened".to_owned());
            return;
        }
        let active = self
            .workspace
            .active_document()
            .and_then(|id| self.workspace.document(id))
            .map(|document| document.path().to_owned());
        if let Some(path) = active {
            self.explorer.select(&self.workspace, &path);
        }
        focus(ctx, explorer::id());
    }

    /// F6: from the editor to the explorer and back, as in the TUI.
    fn focus_next_part(&mut self, ctx: &Context) {
        if sidebar_has_focus(ctx) {
            self.focus_editor(ctx);
        } else if self.sidebar && self.extensions.shown {
            focus(ctx, extensions::search_id());
        } else if self.sidebar {
            self.focus_explorer(ctx);
        }
    }

    /// With nothing focused, the editor takes focus, or else the explorer.
    fn default_focus(&self, ctx: &Context) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        if self.workspace.active_document().is_some() {
            focus(ctx, editor::id());
        } else if self.sidebar && self.workspace.explorer().is_some() {
            focus(ctx, explorer::id());
        }
    }

    fn show_sidebar(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        if self.extensions.shown {
            let refused = ui
                .scope_builder(
                    UiBuilder::new().max_rect(rect).id_salt("rustode-sidebar"),
                    |ui| {
                        ui.set_clip_rect(rect);
                        extensions::show(ui, &mut self.workspace, &mut self.extensions)
                    },
                )
                .inner;
            if refused.is_some() {
                self.notice = refused;
            }
            return false;
        }
        let outcome = ui
            .scope_builder(
                UiBuilder::new().max_rect(rect).id_salt("rustode-sidebar"),
                |ui| explorer::show(ui, &mut self.workspace, &mut self.explorer),
            )
            .inner;
        if let Some(message) = outcome.message {
            self.message = Some(message);
        }
        if outcome.focus_editor {
            self.focus_editor(ui.ctx());
        }
        outcome.changed
    }

    fn show_tabs(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        let action = ui
            .scope_builder(
                UiBuilder::new().max_rect(rect).id_salt("rustode-tabs"),
                |ui| tabs::show(ui, &self.workspace, &mut self.strip),
            )
            .inner;
        match action {
            Some(tabs::Action::Activate(id)) => {
                let _ = self.workspace.activate(id);
                self.focus_editor(ui.ctx());
                true
            }
            Some(tabs::Action::Close(id)) => {
                self.close(ui.ctx(), id);
                true
            }
            None => false,
        }
    }

    fn show_editor(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        let Some(id) = self.workspace.active_document() else {
            empty_group(ui, rect);
            return false;
        };
        let name = self
            .workspace
            .document(id)
            .map(file_name)
            .unwrap_or_default();
        let view = self.views.entry(id).or_default();
        let workspace = &mut self.workspace;
        let outcome = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(rect)
                    .id_salt("rustode-editor-area"),
                |ui| editor::show(ui, workspace, id, view, &name),
            )
            .inner;
        if let Some(error) = outcome.error {
            self.error = Some(error);
        }
        outcome.changed
    }
}

/// The editor area with no tab open: how to open one, as in the TUI.
fn empty_group(ui: &mut egui::Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, EDITOR_BACKGROUND);
    let lines = [
        "No file open",
        "Open one from the explorer: Ctrl+Shift+E or F6",
    ];
    for (row, line) in (0_u8..).zip(lines) {
        let center = rect.center() + Vec2::new(0.0, (f32::from(row) - 0.5) * 20.0);
        let place = Rect::from_center_size(center, Vec2::new(rect.width(), 20.0));
        let text = RichText::new(line)
            .font(FontId::proportional(13.0))
            .color(HINT);
        ui.put(place, Label::new(text).selectable(false).truncate());
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.wake_on_results(&ctx);
        self.workspace.poll_external_changes();
        self.workspace.poll_extensions();
        self.note();
        let mut changed = self.workbench_keys(&ctx);
        self.default_focus(&ctx);
        let rect = ui.max_rect();
        let width = *self
            .sidebar_width
            .get_or_insert_with(|| sidebar_width(rect.width()));
        let areas = areas(rect, self.sidebar.then_some(width));
        if let Some(sidebar) = areas.sidebar {
            changed |= self.show_sidebar(ui, sidebar);
        }
        changed |= self.show_tabs(ui, areas.tabs);
        changed |= self.show_editor(ui, areas.editor);
        self.note();
        let message = self
            .message
            .as_deref()
            .or(self.error.as_deref())
            .or(self.notice.as_deref());
        let active = self.workspace.active_document();
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(areas.status)
                .id_salt("rustode-status"),
            |ui| status::show(ui, &self.workspace, active, message),
        );
        self.update_title(&ctx);
        if let Some(latency) = &self.latency {
            latency.borrow_mut().frame(changed);
        }
    }
}

/// The status bar's notice for an extension event, if it has one.
fn extension_notice(event: &ExtensionEvent) -> Option<String> {
    Some(match event {
        ExtensionEvent::Installed { id, version } => {
            format!("Installed {id} {version} ({}).", NOT_LOADED.to_lowercase())
        }
        ExtensionEvent::Uninstalled { id } => format!("Uninstalled {id}."),
        ExtensionEvent::Failed { id, message } => format!("{id}: {message}"),
        ExtensionEvent::ListFailed { message } => message.clone(),
        _ => return None,
    })
}
