//! "Change Language Mode": a list picker over the editor, opened by a click
//! on the status bar's language name, as in VS Code
//! (`src/vs/workbench/browser/parts/editor/editorStatus.ts:577-589`), or by
//! Ctrl+K then M (`:1110-1131`). A text box filters the list, which the
//! workspace builds (`Workspace::language_picks`: "Auto Detect" first, then
//! the languages by name, each with its id in parentheses and its
//! extensions); Up and Down move the selection, Enter or a click picks,
//! Escape closes. The text box and the rows are egui widgets, so they are in
//! the AccessKit tree as a text input and buttons named after the language.
//!
//! The picker is self-contained: the window opens it with [`Picker::open`],
//! or [`Picker::chord`] sees the keys, and calls [`Picker::show`] every
//! frame. It is not VS Code's quick input, which the GUI does not have yet.

use eframe::egui::{
    self, Align2, Color32, Context, FontId, Frame, Id, Key, Modifiers, Order, ScrollArea, Sense,
    Stroke, TextEdit, Vec2,
};
use rustode_workspace::{DocumentId, LanguagePick, Workspace};

const BACKGROUND: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const BORDER: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DIM: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x66);
const WIDTH: f32 = 420.0;
const ROW_HEIGHT: f32 = 22.0;
const LIST_HEIGHT: f32 = 220.0;

/// What the picker did this frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Still open, or not open.
    None,
    /// Closed without a pick.
    Closed,
    /// A language was picked and set; the editor takes focus again.
    Picked,
    /// Picking failed with this message.
    Failed(String),
}

/// The picker's state.
#[derive(Debug, Default)]
pub(crate) struct Picker {
    /// The document whose language it picks, while open.
    document: Option<DocumentId>,
    query: String,
    selected: usize,
    /// Whether the text box still has to take focus.
    focus: bool,
    /// Ctrl+K was pressed and M would complete the chord.
    chord: bool,
}

impl Picker {
    /// Opens the picker for `document`.
    pub(crate) fn open(&mut self, document: DocumentId) {
        *self = Self {
            document: Some(document),
            focus: true,
            ..Self::default()
        };
    }

    pub(crate) fn is_open(&self) -> bool {
        self.document.is_some()
    }

    /// Ctrl+K then M, VS Code's chord: returns whether it completed. The
    /// keys are taken from the frame's events.
    pub(crate) fn chord(&mut self, ctx: &Context) -> bool {
        ctx.input_mut(|input| {
            let mut done = false;
            let mut chord = self.chord;
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    // A typed `m` after the chord's key must not reach the editor.
                    return !(done && matches!(event, egui::Event::Text(t) if t == "m"));
                };
                if *key == Key::K
                    && (modifiers.ctrl || modifiers.command)
                    && !modifiers.alt
                    && !modifiers.shift
                {
                    // Kept for the editor, whose Ctrl+K Ctrl+I shares the
                    // first key.
                    chord = true;
                    return true;
                }
                let second = chord && *key == Key::M && !modifiers.alt && !modifiers.shift;
                chord = false;
                done |= second;
                !second
            });
            self.chord = chord;
            done
        })
    }

    /// Draws the picker if it is open and handles its keys.
    pub(crate) fn show(&mut self, ctx: &Context, workspace: &mut Workspace) -> Outcome {
        let Some(document) = self.document else {
            return Outcome::None;
        };
        let Ok(picks) = workspace.language_picks(document, &self.query) else {
            self.document = None;
            return Outcome::Closed;
        };
        self.selected = self.selected.min(picks.len().saturating_sub(1));
        let (down, up, enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.document = None;
            return Outcome::Closed;
        }
        if down {
            self.selected = (self.selected + 1).min(picks.len().saturating_sub(1));
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        let mut chosen = enter.then(|| picks.get(self.selected)).flatten().cloned();
        if let Some(clicked) = self.draw(ctx, &picks) {
            chosen = Some(clicked);
        }
        let Some(pick) = chosen else {
            return Outcome::None;
        };
        self.document = None;
        match workspace.set_language(document, pick.id) {
            Ok(()) => Outcome::Picked,
            Err(err) => Outcome::Failed(err.to_string()),
        }
    }

    /// The window: the filter box and the rows; returns a clicked row.
    fn draw(&mut self, ctx: &Context, picks: &[LanguagePick]) -> Option<LanguagePick> {
        let mut clicked = None;
        egui::Area::new(Id::new("rustode-language-picker"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 28.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(BACKGROUND)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        let before = self.query.clone();
                        let edit = ui.add(
                            TextEdit::singleline(&mut self.query)
                                .hint_text("Select Language Mode")
                                .desired_width(f32::INFINITY),
                        );
                        if std::mem::take(&mut self.focus) {
                            edit.request_focus();
                        }
                        if self.query != before {
                            self.selected = 0;
                        }
                        ScrollArea::vertical()
                            .max_height(LIST_HEIGHT)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (index, pick) in picks.iter().enumerate() {
                                    if row(ui, pick, index == self.selected) {
                                        clicked = Some(pick.clone());
                                    }
                                }
                            });
                    });
            });
        clicked
    }
}

/// One row: the language's name, `(id)`, and its extensions on the right.
/// Returns whether it was clicked.
fn row(ui: &mut egui::Ui, pick: &LanguagePick, selected: bool) -> bool {
    let size = Vec2::new(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let name = if pick.description.is_empty() {
        pick.label.to_owned()
    } else {
        format!("{} {}", pick.label, pick.description)
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if selected {
        ui.painter().rect_filled(rect, 0.0, SELECTED);
    }
    let font = FontId::proportional(13.0);
    let painter = ui.painter();
    let label = painter.text(
        rect.left_center() + Vec2::new(6.0, 0.0),
        Align2::LEFT_CENTER,
        pick.label,
        font.clone(),
        FOREGROUND,
    );
    painter.text(
        egui::pos2(label.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        &pick.description,
        font,
        DIM,
    );
    painter.text(
        rect.right_center() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        short(&pick.extensions, 24),
        FontId::proportional(11.0),
        DIM,
    );
    response.clicked()
}

/// `text` cut to `max` chars, with an ellipsis.
fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}
