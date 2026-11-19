//! Go to definition, find references, rename and format in the window. What
//! they do is the workspace's (`request_definition`, `request_references`,
//! `rename_symbol`, `format_document`, `locations`); this module draws the
//! two boxes they need, over the editor like the language picker: a list of
//! the places a request found, and a text box for the new name.
//!
//! Keys, as in VS Code (`src/vs/editor/contrib/gotoSymbol/browser/goToCommands.ts:276-296`,
//! `:669-681`; `src/vs/editor/contrib/rename/browser/rename.ts:365-370`;
//! `src/vs/editor/contrib/format/browser/formatActions.ts:218-224`): F12 goes
//! to the definition, Shift+F12 lists the references, F2 renames, Ctrl+Shift+I
//! formats. One location is gone to at once; several are listed (VS Code
//! opens a peek widget), where typing narrows the list by file or text, Up
//! and Down move, Enter or a click goes there and Escape closes. The rename
//! box starts with the word under the cursor; Enter renames. The text boxes
//! and the rows are egui widgets, so they are in the AccessKit tree as text
//! inputs and buttons named after what they show.

use eframe::egui::{
    self, Align2, Color32, Context, FontId, Frame, Id, Key, Modifiers, Order, ScrollArea, Sense,
    Stroke, TextEdit, Vec2,
};
use rustode_workspace::{DocumentId, SymbolLocation, Workspace};

const BACKGROUND: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const BORDER: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DIM: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x66);
const WIDTH: f32 = 520.0;
const ROW_HEIGHT: f32 = 22.0;
const LIST_HEIGHT: f32 = 220.0;

/// The commands this module's keys run, by VS Code id.
pub(crate) const REVEAL_DEFINITION: &str = "editor.action.revealDefinition";
pub(crate) const GO_TO_REFERENCES: &str = "editor.action.goToReferences";
pub(crate) const RENAME: &str = "editor.action.rename";
pub(crate) const FORMAT_DOCUMENT: &str = "editor.action.formatDocument";

/// What the boxes did this frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Still open, or not open.
    None,
    /// Closed without a pick.
    Closed,
    /// A location was gone to in this document.
    Picked(DocumentId),
    /// Something to say in the status bar.
    Message(String),
}

/// Which box is open.
#[derive(Debug)]
enum Mode {
    Locations { selected: usize },
    Rename { document: DocumentId, word: String },
}

/// The boxes' state.
#[derive(Debug, Default)]
pub(crate) struct Navigator {
    mode: Option<Mode>,
    query: String,
    /// Whether the text box still has to take focus.
    focus: bool,
}

/// The command the keys of this frame ask for, taking them out of the input.
pub(crate) fn keys(ctx: &Context) -> Option<&'static str> {
    ctx.input_mut(|input| {
        let none = Modifiers::NONE;
        // The chords with Shift first: a plain F12 would match them too.
        if input.consume_key(Modifiers::SHIFT, Key::F12) {
            Some(GO_TO_REFERENCES)
        } else if input.consume_key(none, Key::F12) {
            Some(REVEAL_DEFINITION)
        } else if input.consume_key(none, Key::F2) {
            Some(RENAME)
        } else if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I) {
            Some(FORMAT_DOCUMENT)
        } else {
            None
        }
    })
}

impl Navigator {
    /// Runs one of the four commands by id on the active document; returns
    /// whether `id` is one of them. F2 opens the rename box; the others ask
    /// the workspace, whose answer comes later as events.
    pub(crate) fn run(&mut self, workspace: &mut Workspace, id: &str) -> Option<Outcome> {
        if !matches!(
            id,
            REVEAL_DEFINITION | GO_TO_REFERENCES | RENAME | FORMAT_DOCUMENT
        ) {
            return None;
        }
        let Some(document) = workspace.active_document() else {
            return Some(Outcome::Message(
                "No text editor active at this time".to_owned(),
            ));
        };
        if id == RENAME {
            return Some(match workspace.rename_target(document) {
                Ok(Some(target)) => {
                    *self = Self {
                        mode: Some(Mode::Rename {
                            document,
                            word: target.text.clone(),
                        }),
                        query: target.text,
                        focus: true,
                    };
                    Outcome::None
                }
                Ok(None) => Outcome::Message("No language server can rename here".to_owned()),
                Err(err) => Outcome::Message(err.to_string()),
            });
        }
        Some(match workspace.execute(id) {
            Ok(true) => Outcome::None,
            Ok(false) => {
                Outcome::Message("No language server can do that for this document".to_owned())
            }
            Err(err) => Outcome::Message(err.to_string()),
        })
    }

    /// The workspace found places to pick from, or closed its list.
    pub(crate) fn locations_changed(&mut self, workspace: &Workspace) -> Option<String> {
        let Some(list) = workspace.locations() else {
            if matches!(self.mode, Some(Mode::Locations { .. })) {
                self.mode = None;
            }
            return None;
        };
        let message = format!(
            "{} of '{}': {}",
            list.kind.title(),
            list.word,
            list.locations.len()
        );
        *self = Self {
            mode: Some(Mode::Locations { selected: 0 }),
            query: String::new(),
            focus: true,
        };
        Some(message)
    }

    /// Draws the open box and handles its keys.
    pub(crate) fn show(&mut self, ctx: &Context, workspace: &mut Workspace) -> Outcome {
        let Some(mode) = self.mode.take() else {
            return Outcome::None;
        };
        let (down, up, enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            workspace.dismiss_locations();
            return Outcome::Closed;
        }
        match mode {
            Mode::Rename { document, word } => self.rename(ctx, workspace, document, word, enter),
            Mode::Locations { selected } => {
                self.locations(ctx, workspace, selected, (down, up, enter))
            }
        }
    }

    fn rename(
        &mut self,
        ctx: &Context,
        workspace: &mut Workspace,
        document: DocumentId,
        word: String,
        enter: bool,
    ) -> Outcome {
        if enter {
            return match workspace.rename_symbol(document, &self.query) {
                Ok(true) => Outcome::Closed,
                Ok(false) => Outcome::Message("Choose another name to rename to".to_owned()),
                Err(err) => Outcome::Message(err.to_string()),
            };
        }
        self.window::<()>(ctx, "rustode-rename", |this, ui| {
            let hint = format!("New name for '{word}', Enter to rename");
            let edit = ui.add(
                TextEdit::singleline(&mut this.query)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut this.focus) {
                edit.request_focus();
            }
            None
        });
        self.mode = Some(Mode::Rename { document, word });
        Outcome::None
    }

    fn locations(
        &mut self,
        ctx: &Context,
        workspace: &mut Workspace,
        mut selected: usize,
        (down, up, enter): (bool, bool, bool),
    ) -> Outcome {
        let Some(list) = workspace.locations() else {
            return Outcome::Closed;
        };
        let shown = list.filtered(&self.query);
        selected = selected.min(shown.len().saturating_sub(1));
        if down {
            selected = (selected + 1).min(shown.len().saturating_sub(1));
        }
        if up {
            selected = selected.saturating_sub(1);
        }
        let mut chosen = enter
            .then(|| shown.get(selected).map(|(i, _)| *i))
            .flatten();
        let before = self.query.clone();
        let rows: Vec<(usize, SymbolLocation)> =
            shown.iter().map(|(i, l)| (*i, (*l).clone())).collect();
        let clicked = self.window(ctx, "rustode-locations", |this, ui| {
            let edit = ui.add(
                TextEdit::singleline(&mut this.query)
                    .hint_text("Filter by file or text")
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut this.focus) {
                edit.request_focus();
            }
            let mut clicked = None;
            ScrollArea::vertical()
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (row_index, (index, location)) in rows.iter().enumerate() {
                        if row(ui, location, row_index == selected) {
                            clicked = Some(*index);
                        }
                    }
                });
            clicked
        });
        if self.query != before {
            selected = 0;
        }
        chosen = clicked.or(chosen);
        let Some(index) = chosen else {
            self.mode = Some(Mode::Locations { selected });
            return Outcome::None;
        };
        match workspace.pick_location(index) {
            Ok(Some(id)) => Outcome::Picked(id),
            Ok(None) => Outcome::Closed,
            Err(err) => Outcome::Message(err.to_string()),
        }
    }

    /// The window over the editor: a framed area holding `add`'s widgets.
    fn window<R>(
        &mut self,
        ctx: &Context,
        name: &'static str,
        add: impl FnOnce(&mut Self, &mut egui::Ui) -> Option<R>,
    ) -> Option<R> {
        let mut result = None;
        egui::Area::new(Id::new(name))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 28.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(BACKGROUND)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        result = add(self, ui);
                    });
            });
        result
    }
}

/// One row: the line's text and its file and line on the right. Returns
/// whether it was clicked.
fn row(ui: &mut egui::Ui, location: &SymbolLocation, selected: bool) -> bool {
    let size = Vec2::new(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let place = place(location);
    let name = format!("{} {place}", location.preview);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if selected {
        ui.painter().rect_filled(rect, 0.0, SELECTED);
    }
    let painter = ui.painter();
    painter.text(
        rect.left_center() + Vec2::new(6.0, 0.0),
        Align2::LEFT_CENTER,
        &location.preview,
        FontId::proportional(13.0),
        FOREGROUND,
    );
    painter.text(
        rect.right_center() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        &place,
        FontId::proportional(11.0),
        DIM,
    );
    response.clicked()
}

/// How a location shows: its file and line, e.g. `main.rs:12`.
fn place(location: &SymbolLocation) -> String {
    let name = location
        .path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    format!("{name}:{}", location.start.line + 1)
}
