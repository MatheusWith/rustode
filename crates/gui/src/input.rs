//! What the quick input (`quick_input.rs`) and the find widget (`find.rs`)
//! share: the floating panel they are drawn on, their one-line text fields
//! and the keys they take before the text fields see them.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the fields are input boxes in Dark Modern's `input.background`,
//! `input.border`, `input.foreground` and `input.placeholderForeground`,
//! outlined in `focusBorder` while they have focus
//! (`extensions/theme-defaults/themes/dark_modern.json:48`, `:51-54`), and
//! the panels carry the default `widget.shadow`
//! (`src/vs/platform/theme/common/colors/editorColors.ts:339-341`).

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, Event, EventFilter, FontId, Id, LayerId, Order,
    Rect, Response, RichText, Shape, Stroke, StrokeKind, TextEdit, Ui, UiBuilder,
    epaint::{RectShape, Shadow},
    text::{CCursor, CCursorRange},
    text_edit::TextEditState,
};

pub(crate) const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const INPUT_BACKGROUND: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
const INPUT_BORDER: Color32 = Color32::from_rgb(0x3c, 0x3c, 0x3c);
const PLACEHOLDER: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
pub(crate) const FOCUS_BORDER: Color32 = Color32::from_rgb(0x00, 0x78, 0xd4);
/// `widget.shadow`: black at 36 %.
const SHADOW: Color32 = Color32::from_black_alpha(92);
/// The text size of the fields.
pub(crate) const FONT_SIZE: f32 = 13.0;

/// Where the caret goes when a field takes focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caret {
    /// The whole text selected, so typing replaces it.
    SelectAll,
    /// After the last char.
    End,
}

/// How a floating panel looks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Panel {
    pub(crate) radius: u8,
    pub(crate) background: Color32,
    pub(crate) border: Color32,
}

impl Panel {
    /// The panel's shadow, background and border over `rect`.
    pub(crate) fn shapes(self, rect: Rect) -> [Shape; 2] {
        let shadow = Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: SHADOW,
        };
        let radius = CornerRadius::same(self.radius);
        [
            shadow.as_shape(rect, radius).into(),
            RectShape::new(
                rect,
                radius,
                self.background,
                Stroke::new(1.0, self.border),
                StrokeKind::Inside,
            )
            .into(),
        ]
    }

    /// Paints the panel over `rect`.
    pub(crate) fn paint(self, ui: &Ui, rect: Rect) {
        ui.painter().extend(self.shapes(rect));
    }
}

/// The look of one text field.
pub(crate) struct Field<'a> {
    pub(crate) id: Id,
    /// The whole box; the text is laid out in `text`.
    pub(crate) rect: Rect,
    pub(crate) text: Rect,
    pub(crate) hint: &'a str,
    pub(crate) radius: u8,
    /// Draws the border in the error color.
    pub(crate) error: Option<Color32>,
}

/// A one-line text field in `field.rect`; with `caret`, it takes focus
/// first. Enter does nothing in it: its owner reads Enter.
pub(crate) fn text_field(
    ui: &mut Ui,
    text: &mut String,
    field: &Field<'_>,
    caret: Option<Caret>,
) -> Response {
    let ctx = ui.ctx().clone();
    if let Some(caret) = caret {
        place_caret(&ctx, field.id, text, caret);
        ctx.memory_mut(|memory| memory.request_focus(field.id));
    }
    let focused = ctx.memory(|memory| memory.has_focus(field.id));
    let border = match (field.error, focused) {
        (Some(color), _) => color,
        (None, true) => FOCUS_BORDER,
        (None, false) => INPUT_BORDER,
    };
    ui.painter().rect(
        field.rect,
        CornerRadius::same(field.radius),
        INPUT_BACKGROUND,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let font = FontId::proportional(FONT_SIZE);
    let edit = TextEdit::singleline(text)
        .id(field.id)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(6, 0)))
        .font(font.clone())
        .text_color(FOREGROUND)
        .hint_text(RichText::new(field.hint).font(font).color(PLACEHOLDER))
        .return_key(None)
        .vertical_align(Align::Center)
        .min_size(field.text.size())
        .desired_width(field.text.width());
    let response = ui.put(field.text, edit);
    if response.has_focus() {
        // The owner reads Tab, the arrows and Escape: egui must not move
        // the focus on them (this overrides the filter `TextEdit` set).
        ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                field.id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }
    response
}

/// Selects the field's text, or puts the caret at its end.
fn place_caret(ctx: &Context, id: Id, text: &str, caret: Caret) {
    let end = CCursor::new(text.chars().count());
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    let range = match caret {
        Caret::SelectAll => CCursorRange::two(CCursor::new(0), end),
        Caret::End => CCursorRange::one(end),
    };
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}

/// Draws `contents` on a foreground layer over `rect`, above the workbench:
/// it paints over it and takes the pointer there. Moving the layer to the
/// top each frame registers it for hit tests and keeps the last one shown
/// on top. (An `egui::Area` would hide its first frame to measure itself.)
pub(crate) fn overlay<R>(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let layer = LayerId::new(Order::Foreground, id);
    ui.ctx().move_to_top(layer);
    ui.scope_builder(
        UiBuilder::new().layer_id(layer).max_rect(rect).id_salt(id),
        contents,
    )
    .inner
}

/// Whether a pointer button went down or clicked inside `rect` this
/// frame: egui then takes the focus from a field, but a click on a panel's
/// own buttons or rows must leave it there.
pub(crate) fn clicked_inside(ctx: &Context, rect: Rect) -> bool {
    ctx.input(|input| {
        (input.pointer.any_pressed() || input.pointer.any_click())
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| rect.contains(pos))
    })
}

/// Takes out of this frame's input the events `key` maps to something,
/// and returns what they map to, in order.
pub(crate) fn take_keys<T>(ctx: &Context, mut key: impl FnMut(&Event) -> Option<T>) -> Vec<T> {
    ctx.input_mut(|input| {
        let mut taken = Vec::new();
        input.events.retain(|event| match key(event) {
            Some(action) => {
                taken.push(action);
                false
            }
            None => true,
        });
        taken
    })
}

/// Drops the text an Alt+letter press typed along with its key event
/// (winit sends both), so a toggle key does not also type its letter.
pub(crate) fn drop_alt_text(ctx: &Context, letters: &[char]) {
    ctx.input_mut(|input| {
        input.events.retain(|event| match event {
            Event::Text(text) => {
                let mut chars = text.chars();
                !matches!(
                    (chars.next(), chars.next()),
                    (Some(c), None) if letters.contains(&c.to_ascii_lowercase())
                )
            }
            _ => true,
        });
    });
}
