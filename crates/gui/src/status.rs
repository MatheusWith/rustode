//! The status bar: a message on the left; the selection, encoding, line
//! ending and language on the right. Each item is a label in the AccessKit
//! tree, in the order it is drawn.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the selection and line-ending labels
//! (`src/vs/workbench/browser/parts/editor/editorStatus.ts:338-343`,
//! `:653-675`), the bar's 22-point height
//! (`src/vs/workbench/browser/parts/statusbar/statusbarPart.ts:125`) and Dark
//! Modern's colors (`extensions/theme-defaults/themes/dark_modern.json:90-98`).

use crate::text::status_column;
use eframe::egui::{Color32, FontId, Label, Rect, RichText, Stroke, Ui, Vec2, pos2};
use rustode_workspace::{Document, DocumentId, Eol, Workspace};

/// Height of the status bar, in points.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const FONT_SIZE: f32 = 12.0;
/// Space around each item.
const PADDING: f32 = 10.0;

/// Draws the status bar over the whole of `ui`.
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &Workspace,
    document: DocumentId,
    message: Option<&str>,
) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, BORDER));
    let Some(doc) = workspace.document(document) else {
        return;
    };
    let tab_size = workspace.tab_size(document).unwrap_or(4);
    let items = [
        selection_label(doc, tab_size),
        doc.encoding_name().to_owned(),
        eol_label(doc.eol()).to_owned(),
        doc.language_name().to_owned(),
    ];
    let font = FontId::proportional(FONT_SIZE);
    let widths: Vec<f32> = items
        .iter()
        .map(|item| {
            ui.painter()
                .layout_no_wrap(item.clone(), font.clone(), FOREGROUND)
                .size()
                .x
                + PADDING
        })
        .collect();
    let mut x = rect.right() - widths.iter().sum::<f32>() - PADDING / 2.0;
    if let Some(message) = message {
        let left = Rect::from_min_max(rect.min + Vec2::new(PADDING, 0.0), pos2(x, rect.bottom()));
        item(ui, left, message, &font);
    }
    for (text, width) in items.iter().zip(widths) {
        let place = Rect::from_min_size(pos2(x, rect.top()), Vec2::new(width, rect.height()));
        item(ui, place, text, &font);
        x += width;
    }
}

fn item(ui: &mut Ui, rect: Rect, text: &str, font: &FontId) {
    let label = Label::new(RichText::new(text).font(font.clone()).color(FOREGROUND))
        .selectable(false)
        .truncate();
    ui.put(rect, label);
}

/// VS Code's selection label: `Ln 3, Col 5`, with `(4 selected)` for a
/// selection, or `3 selections` for several cursors.
fn selection_label(doc: &Document, tab_size: usize) -> String {
    let selections = doc.selections();
    let selected: usize = selections
        .iter()
        .map(|selection| selection.range().end - selection.range().start)
        .sum();
    if let [_, _, ..] = selections {
        return if selected > 0 {
            format!(
                "{} selections ({selected} characters selected)",
                selections.len()
            )
        } else {
            format!("{} selections", selections.len())
        };
    }
    let head = doc.position_of(doc.primary_selection().head);
    let column = status_column(&doc.line(head.line), head.column, tab_size);
    let position = format!("Ln {}, Col {column}", head.line + 1);
    if selected > 0 {
        format!("{position} ({selected} selected)")
    } else {
        position
    }
}

fn eol_label(eol: Eol) -> &'static str {
    match eol {
        Eol::Lf => "LF",
        Eol::Crlf => "CRLF",
    }
}
