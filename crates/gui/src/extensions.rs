//! The Extensions view in the sidebar, in place of the explorer, like VS
//! Code's: Ctrl+Shift+X shows it with the search box focused, or,
//! when the search box has focus, gives focus back to the editor
//! (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:116-123`;
//! `src/vs/workbench/services/views/browser/viewsService.ts:426-441`). It
//! holds the search box (`extensionsViewlet.ts:647`), the registry's
//! results with Install / Installing / Installed buttons
//! (`extensionsActions.ts:478`, `:765`, `:2728`) and the Installed list
//! (`extensionsViewlet.ts:350`) with Uninstall / Uninstalling
//! (`extensionsActions.ts:939-940`). Every installed extension shows "Not
//! loaded in this phase". Colors are Dark Modern's sidebar, input and
//! button colors (`extensions/theme-defaults/themes/dark_modern.json:15-17`,
//! `:28`, `:51-54`, `:83-89`).
//!
//! Every widget is an egui widget, so each is in the AccessKit tree: the
//! search box is a text input, names and notices are labels, actions are
//! buttons. All work goes through the workspace, in the background.

use eframe::egui::{
    self, Align, Button, Color32, FontId, Id, Key, Label, Layout, RichText, ScrollArea, Stroke,
    TextEdit, Ui, Vec2,
};
use rustode_workspace::{
    ExtensionOperation, ExtensionSearch, ExtensionSummary, NOT_LOADED, Workspace,
};

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DESCRIPTION: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const INPUT: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
const BUTTON: Color32 = Color32::from_rgb(0x00, 0x78, 0xd4);
const BUTTON_TEXT: Color32 = Color32::WHITE;
const FONT_SIZE: f32 = 13.0;
/// The search box's placeholder.
const PLACEHOLDER: &str = "Search Extensions in Open VSX";

/// What the panel keeps between frames.
#[derive(Debug, Default)]
pub(crate) struct Panel {
    /// Whether the sidebar shows this view instead of the explorer.
    pub shown: bool,
    /// The search box's text.
    query: String,
    /// Focus the search box on the next frame.
    focus_search: bool,
}

/// The id of the search box.
pub(crate) fn search_id() -> Id {
    Id::new("rustode-extensions-search")
}

impl Panel {
    /// Ctrl+Shift+X: shows the panel and focuses its search box, or gives
    /// focus back to the editor when the search box has it.
    pub(crate) fn toggle(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
    ) -> Option<String> {
        if self.shown && ctx.memory(|m| m.has_focus(search_id())) {
            ctx.memory_mut(|m| m.request_focus(crate::editor::id()));
            return None;
        }
        self.shown = true;
        self.focus_search = true;
        workspace
            .refresh_extensions()
            .err()
            .map(|err| err.to_string())
    }
}

fn text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::proportional(FONT_SIZE))
        .color(color)
}

fn label(ui: &mut Ui, rich: RichText) {
    ui.add(Label::new(rich).selectable(false).truncate());
}

/// A button with Dark Modern's colors; disabled ones show a state.
fn button(ui: &mut Ui, caption: &str, enabled: bool) -> bool {
    let button = Button::new(text(caption, BUTTON_TEXT))
        .fill(if enabled { BUTTON } else { INPUT })
        .stroke(Stroke::NONE);
    ui.add_enabled(enabled, button).clicked()
}

/// Draws the panel over the whole of `ui`; returns a message for the
/// status bar when a command was refused.
pub(crate) fn show(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel) -> Option<String> {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, BORDER));
    let mut message = None;
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(12.0, 8.0))),
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            label(ui, text("EXTENSIONS", FOREGROUND).strong());
            search_box(ui, workspace, panel);
            if let Some(status) = search_status(workspace) {
                label(ui, text(status, DESCRIPTION));
            }
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                message = list(ui, workspace);
            });
        },
    );
    message
}

/// The search box: Enter searches.
fn search_box(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel) {
    let response = ui.add(
        TextEdit::singleline(&mut panel.query)
            .id(search_id())
            .hint_text(text(PLACEHOLDER, DESCRIPTION))
            .font(FontId::proportional(FONT_SIZE))
            .text_color(FOREGROUND)
            .background_color(INPUT)
            .return_key(None)
            .desired_width(f32::INFINITY),
    );
    if panel.focus_search {
        panel.focus_search = false;
        response.request_focus();
    }
    if response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        workspace.search_extensions(&panel.query);
    }
}

/// The line under the search box.
fn search_status(workspace: &Workspace) -> Option<String> {
    Some(match workspace.extension_search() {
        ExtensionSearch::Idle => return None,
        ExtensionSearch::Searching { .. } => "Searching…".to_owned(),
        ExtensionSearch::Done { results, total, .. } if results.is_empty() => {
            format!("No extensions found ({total})")
        }
        ExtensionSearch::Done { results, total, .. } => {
            format!("Open VSX: {} of {total}", results.len())
        }
        ExtensionSearch::Failed { message, .. } => format!("Search failed: {message}"),
    })
}

/// The results, then the installed extensions.
fn list(ui: &mut Ui, workspace: &mut Workspace) -> Option<String> {
    let results: Vec<ExtensionSummary> = match workspace.extension_search() {
        ExtensionSearch::Done { results, .. } => results.clone(),
        _ => Vec::new(),
    };
    let mut install = None;
    for result in &results {
        let id = result.id();
        let installed = workspace.installed_extension(&id).is_some();
        let (caption, enabled) = match workspace.extension_operation(&id) {
            Some(ExtensionOperation::Installing) => ("Installing", false),
            Some(ExtensionOperation::Uninstalling) => ("Uninstalling", false),
            None if installed => ("Installed", false),
            None => ("Install", true),
        };
        let detail = if installed {
            NOT_LOADED.to_owned()
        } else {
            format!("{} · {}", result.namespace, result.version)
        };
        if row(ui, &result.display_name, &detail, caption, enabled) {
            install = Some(id);
        }
    }
    ui.add_space(6.0);
    let installed = workspace.installed_extensions().to_vec();
    label(
        ui,
        text(format!("INSTALLED · {}", installed.len()), FOREGROUND).strong(),
    );
    if installed.is_empty() {
        label(ui, text("No extensions installed", DESCRIPTION));
    }
    let mut uninstall = None;
    for extension in &installed {
        let caption = match workspace.extension_operation(&extension.id) {
            Some(ExtensionOperation::Uninstalling) => "Uninstalling",
            Some(ExtensionOperation::Installing) => "Installing",
            None => "Uninstall",
        };
        let detail = format!("{} · {}", extension.version, extension.notice());
        if row(
            ui,
            &extension.display_name,
            &detail,
            caption,
            caption == "Uninstall",
        ) {
            uninstall = Some(extension.id.clone());
        }
    }
    let result = match (install, uninstall) {
        (Some(id), _) => workspace.install_extension(&id),
        (None, Some(id)) => workspace.uninstall_extension(&id),
        (None, None) => Ok(()),
    };
    result.err().map(|err| err.to_string())
}

/// One extension: its name with an action button, then a detail line.
/// Returns whether the button was clicked.
fn row(ui: &mut Ui, name: &str, detail: &str, caption: &str, enabled: bool) -> bool {
    ui.add_space(4.0);
    let clicked = ui
        .horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clicked = button(ui, caption, enabled);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    label(ui, text(name, FOREGROUND).strong());
                });
                clicked
            })
            .inner
        })
        .inner;
    label(ui, text(detail, DESCRIPTION));
    clicked
}
