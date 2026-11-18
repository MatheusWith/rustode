//! The status bar: a message on the left; the active document's selection,
//! encoding, line ending and language on the right. In the AccessKit tree
//! it is a status node whose children are labels, one per item, in the
//! order they are drawn, each reading as its text does in VS Code.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the selection and line-ending labels
//! (`src/vs/workbench/browser/parts/editor/editorStatus.ts:338-343`,
//! `:653-675`), each item's accessible name being its text (`:510-512`,
//! `:550-586`), the bar's `status` role
//! (`src/vs/workbench/browser/parts/statusbar/statusbarPart.ts:928`) and
//! 22-point height (`:125`), and Dark Modern's colors
//! (`extensions/theme-defaults/themes/dark_modern.json:90-101`), with the
//! no-folder background while no folder is open (`statusbarPart.ts:770`).
//!
//! **Language servers.** Once a language server is configured, the problem
//! counts come first on the left, a button that toggles the problems panel as VS Code's does
//! (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:583`,
//! `:615-624`), and the document's server shows before the language, with
//! its state unless it runs.
//!
//! **Narrow windows.** The right-hand items sit in priority order from the
//! right edge (selection 100.5, encoding 100.3, line ending 100.2,
//! language 100.1, `editorStatus.ts:517-591`); those that no longer fit
//! drop out from the left, so the highest-priority one goes first, as VS
//! Code's `flex-wrap: wrap` in `row-reverse` order wraps them onto a line
//! the bar's `overflow: hidden` cuts off
//! (`src/vs/workbench/browser/parts/statusbar/media/statusbarpart.css:6-15`,
//! `:49-52`; `statusbarPart.ts:564-569`). The rightmost item always stays,
//! cut short if need be. An item is at most 40% of the window wide
//! (`statusbarpart.css:63`), so a long message is cut short there.
//!
//! **Keyboard.** The bar is one stop for Tab and F6, outlined in
//! `statusBar.focusBorder`; Left/Up and Right/Down move between its items,
//! wrapping, Home and End go to the first and the last, and Escape leaves
//! an item for the bar, then the bar for the editor
//! (`statusbarPart.ts:478`, `:809-818`;
//! `src/vs/workbench/browser/parts/statusbar/statusbarActions.ts:63-123`;
//! `src/vs/workbench/browser/parts/statusbar/statusbarModel.ts:162-207`).
//! The items are not Tab stops (`statusbarItem.ts:71-72`): the focused one
//! is the bar's active descendant, outlined in `statusBarItem.focusBorder`.
//! While Ctrl+M has Tab moving focus, a prominent "Tab Moves Focus" item
//! leads the right-hand items (`editorStatus.ts:442-458`, priority 100.7;
//! `statusbarpart.css:206-209`).

use crate::focus;
use crate::text::status_column;
use eframe::egui::{
    Align, Color32, EventFilter, FontId, Id, Key, Label, Layout, Rect, Response, RichText, Stroke,
    Ui, UiBuilder, Vec2, accesskit, pos2,
};
use rustode_workspace::{Document, DocumentId, Eol, ProblemCounts, ServerState, Workspace};

/// Height of the status bar, in points.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const NO_FOLDER_BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
/// `statusBarItem.prominentBackground`, `#6E768166`.
const PROMINENT: Color32 = Color32::from_rgba_unmultiplied_const(0x6e, 0x76, 0x81, 0x66);
const FONT_SIZE: f32 = 12.0;
/// Space around each item.
const PADDING: f32 = 10.0;
/// The widest an item gets, as a share of the window.
const ITEM_MAX_SHARE: f32 = 0.4;
/// The status bar item shown while Tab moves focus.
pub(crate) const TAB_MOVES_FOCUS: &str = "Tab Moves Focus";

/// The status bar's widget id: it takes keyboard focus.
pub(crate) fn id() -> Id {
    Id::new("rustode-status")
}

/// What the status bar keeps between frames: which item has focus.
#[derive(Debug, Default)]
pub(crate) struct Bar {
    focused: Option<&'static str>,
}

/// One item, as laid out this frame.
struct Item {
    key: &'static str,
    text: String,
    width: f32,
    prominent: bool,
    /// The problem counts, drawn as icons and numbers instead of `text`.
    counts: Option<ProblemCounts>,
}

/// The key of the problem counts item.
const PROBLEMS: &str = "problems";

/// What the bar asks of the window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Escape on the bar: the focus goes to the editor.
    pub(crate) leave: bool,
    /// The problem counts were clicked, or activated with Enter or Space.
    pub(crate) toggle_problems: bool,
}

/// Draws the status bar over the whole of `ui`, for the active document if
/// there is one. Returns what the bar asks of the window: [`Outcome`].
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &Workspace,
    document: Option<DocumentId>,
    message: Option<&str>,
    bar: &mut Bar,
) -> Outcome {
    let rect = ui.max_rect();
    ui.expand_to_include_rect(rect);
    let background = if workspace.explorer().is_some() {
        BACKGROUND
    } else {
        NO_FOLDER_BACKGROUND
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, background);
    painter.hline(
        rect.x_range(),
        rect.top(),
        Stroke::new(focus::hairline(ui.pixels_per_point()), BORDER),
    );
    let font = FontId::proportional(FONT_SIZE);
    let (left, right) = fit(rect, items(ui, workspace, document, message, &font));
    let mut shown = Vec::new();
    for (item, place) in left {
        shown.push((item.key, place, draw(ui, place, &item, &font, true)));
    }
    for (item, place) in right {
        shown.push((item.key, place, draw(ui, place, &item, &font, false)));
    }
    let has_focus = ui.memory(|memory| memory.has_focus(id()));
    let mut outcome = Outcome::default();
    if has_focus {
        outcome = keyboard(ui, bar, &shown);
    } else {
        bar.focused = None;
    }
    outcome.toggle_problems |= shown
        .iter()
        .any(|(key, _, response)| *key == PROBLEMS && response.clicked());
    let focused = shown.iter().find(|(key, ..)| Some(*key) == bar.focused);
    if has_focus {
        focus::outline(&painter, focused.map_or(rect, |(_, place, _)| *place));
    }
    ui.ctx().accesskit_node_builder(id(), |node| {
        node.set_role(accesskit::Role::Status);
        node.set_bounds(crate::a11y::bounds(rect));
        if let Some((_, _, response)) = focused {
            node.set_active_descendant(response.id.accesskit_id());
        }
    });
    outcome
}

/// The items to show: the problem counts and the message on the left, then
/// the right-hand items left to right, each measured.
fn items(
    ui: &Ui,
    workspace: &Workspace,
    document: Option<DocumentId>,
    message: Option<&str>,
    font: &FontId,
) -> Vec<Item> {
    let mut texts: Vec<(&'static str, String)> = Vec::new();
    if let Some(message) = message {
        texts.push(("message", message.to_owned()));
    }
    if focus::tab_moves_focus(ui.ctx()) {
        texts.push(("tab-moves-focus", TAB_MOVES_FOCUS.to_owned()));
    }
    if let Some((id, doc)) = document.and_then(|id| workspace.document(id).map(|doc| (id, doc))) {
        let tab_size = workspace.tab_size(id).unwrap_or(4);
        texts.push(("selection", selection_label(doc, tab_size)));
        texts.push(("encoding", doc.encoding_name().to_owned()));
        texts.push(("eol", eol_label(doc.eol()).to_owned()));
        if let Some(server) = workspace.language_server(id) {
            texts.push((
                "server",
                match server.state {
                    ServerState::Running => server.name,
                    state => format!("{} ({})", server.name, state.label()),
                },
            ));
        }
        texts.push(("language", doc.language_name().to_owned()));
    }
    let mut items: Vec<Item> = texts
        .into_iter()
        .map(|(key, text)| {
            let width = ui
                .painter()
                .layout_no_wrap(text.clone(), font.clone(), FOREGROUND)
                .size()
                .x
                + PADDING;
            Item {
                key,
                prominent: key == "tab-moves-focus",
                text,
                width,
                counts: None,
            }
        })
        .collect();
    if !workspace.language_servers().is_empty() {
        let counts = workspace.problem_counts();
        items.insert(
            0,
            Item {
                key: PROBLEMS,
                text: String::new(),
                width: crate::problems::counts_width(ui, counts),
                prominent: false,
                counts: Some(counts),
            },
        );
    }
    items
}

/// An item and where it goes.
type Placed = (Item, Rect);

/// Where the items go in `rect`: on the left the problem counts, then the
/// message, at most 40% of the bar wide; on the right the items that fit,
/// from the right edge. The counts item is the last to go: it drops out
/// only when the bar is too narrow to show it whole.
fn fit(rect: Rect, mut items: Vec<Item>) -> (Vec<Placed>, Vec<Placed>) {
    let max = rect.width() * ITEM_MAX_SHARE;
    let mut left = Vec::new();
    let mut start = rect.left() + PADDING;
    if items.first().is_some_and(|item| item.key == PROBLEMS) {
        let item = items.remove(0);
        let place = Rect::from_min_size(
            pos2(rect.left() + PADDING / 2.0, rect.top()),
            Vec2::new(item.width, rect.height()),
        );
        if place.right() <= rect.right() {
            start += item.width;
            left.push((item, place));
        }
    }
    let message = if items.first().is_some_and(|item| item.key == "message") {
        let mut item = items.remove(0);
        item.width = (item.width - PADDING).min(max);
        Some(item)
    } else {
        None
    };
    let message_end = message.as_ref().map_or(start, |item| start + item.width);
    let mut right = Vec::new();
    let mut x = rect.right() - PADDING / 2.0;
    for mut item in items.into_iter().rev() {
        item.width = item.width.min(max);
        let room = x - message_end;
        if item.width > room {
            if right.is_empty() {
                item.width = room.max(0.0);
            } else {
                break;
            }
        }
        x -= item.width;
        let place = Rect::from_min_size(pos2(x, rect.top()), Vec2::new(item.width, rect.height()));
        right.push((item, place));
    }
    right.reverse();
    if let Some(item) = message {
        let place = Rect::from_min_max(
            pos2(start, rect.top()),
            pos2(x.min(message_end), rect.bottom()),
        );
        left.push((item, place));
    }
    (left, right)
}

/// One item as a label, centered in `rect`, or from its left edge.
fn draw(ui: &mut Ui, rect: Rect, item: &Item, font: &FontId, from_left: bool) -> Response {
    if let Some(counts) = item.counts {
        return crate::problems::paint_counts(ui, rect, counts);
    }
    if item.prominent {
        ui.painter().rect_filled(rect, 0.0, PROMINENT);
    }
    let label = Label::new(
        RichText::new(&item.text)
            .font(font.clone())
            .color(FOREGROUND),
    )
    .selectable(false)
    .truncate();
    if from_left {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| ui.add(label),
        )
        .inner
    } else {
        ui.put(rect, label)
    }
}

/// The bar's keys while it has focus: Escape leaves it, and Enter or Space
/// on the problem counts toggles the panel.
fn keyboard(ui: &Ui, bar: &mut Bar, shown: &[(&'static str, Rect, Response)]) -> Outcome {
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id(),
            EventFilter {
                tab: false,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });
    let keys: Vec<&'static str> = shown.iter().map(|(key, ..)| *key).collect();
    let mut outcome = Outcome::default();
    let pressed = ui.input(|input| {
        [
            Key::ArrowLeft,
            Key::ArrowUp,
            Key::ArrowRight,
            Key::ArrowDown,
            Key::Home,
            Key::End,
            Key::Escape,
            Key::Enter,
            Key::Space,
        ]
        .into_iter()
        .filter(|&key| input.key_pressed(key))
        .collect::<Vec<_>>()
    });
    for key in pressed {
        let index = bar
            .focused
            .and_then(|focused| keys.iter().position(|&shown| shown == focused));
        let last = keys.len().checked_sub(1);
        let target = match key {
            Key::ArrowLeft | Key::ArrowUp => match index {
                Some(index) if index > 0 => Some(index - 1),
                _ => last,
            },
            Key::ArrowRight | Key::ArrowDown => match index {
                Some(index) if Some(index) != last => Some(index + 1),
                _ => last.map(|_| 0),
            },
            Key::Home => last.map(|_| 0),
            Key::End => last,
            Key::Enter | Key::Space => {
                outcome.toggle_problems |= bar.focused == Some(PROBLEMS);
                index
            }
            _ => {
                outcome.leave |= index.is_none();
                None
            }
        };
        bar.focused = target.map(|index| keys[index]);
    }
    outcome
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
