//! The comparison of a disk conflict, in the terminal.
//!
//! The workspace computes the diff
//! ([`Workspace::conflict_compare`](rustode_workspace::Workspace::conflict_compare);
//! behavior studied in VS Code and reimplemented, see
//! `crates/workspace/src/compare.rs` and `ATTRIBUTION.md`); this view only
//! draws it. VS Code shows the two texts side by side. A terminal is often
//! 80 columns, too narrow for two columns of code, so rustode shows one
//! column, the unified form: each line carries both line numbers (the
//! file's, then the unsaved text's) and a mark, `-` for a line only in the
//! file on disk and `+` for a line only in the unsaved text. Colors only
//! decorate the marks.
//!
//! It replaces the editor area while open and is read-only. Keys: Up, Down,
//! `PageUp`, `PageDown`, `Home` and `End` scroll; `n` and `p` go to the next and
//! previous change; `o` runs Overwrite (VS Code's "Use your changes and
//! overwrite file contents") and `r` runs Revert ("Discard your changes and
//! revert to file contents"), each closing the comparison; Esc or `q`
//! closes it and keeps the conflict. The conflict commands are the ones the
//! prompt runs, so a user's `keybindings.json` and the palette reach them
//! as well.

use crate::app::App;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use rustode_workspace::{ConflictCompare, DiffKind, DocumentId};

/// The open comparison.
#[derive(Debug)]
pub(crate) struct CompareView {
    model: ConflictCompare,
    /// The first line shown.
    top: usize,
    /// The change the user last jumped to, an index into `model.changes`.
    current: usize,
    /// How many lines the last frame had room for.
    page: usize,
}

impl App {
    /// Shows the comparison for `id`, replacing the editor area.
    pub(crate) fn open_compare(&mut self, id: DocumentId) {
        match self.workspace.conflict_compare(id) {
            Ok(model) => {
                self.conflict = None;
                self.message = None;
                let top = model.changes.first().map_or(0, |c| c.saturating_sub(2));
                self.compare = Some(CompareView {
                    model,
                    top,
                    current: 0,
                    page: 10,
                });
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// Closes the comparison once its conflict is gone, however that
    /// happened (a choice, the palette, a reload).
    pub(crate) fn refresh_compare(&mut self) {
        let Some(view) = &self.compare else { return };
        if self
            .workspace
            .document(view.model.document)
            .is_none_or(|d| !d.has_conflict())
        {
            self.compare = None;
        }
    }

    /// A key while the comparison is open; everything else is ignored.
    pub(crate) fn compare_key(&mut self, key: KeyEvent) {
        let Some(view) = self.compare.as_mut() else {
            return;
        };
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        let last = view.model.lines.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.compare = None,
            KeyCode::Up => view.top = view.top.saturating_sub(1),
            KeyCode::Down => view.top = (view.top + 1).min(last),
            KeyCode::PageUp => view.top = view.top.saturating_sub(view.page),
            KeyCode::PageDown => view.top = (view.top + view.page).min(last),
            KeyCode::Home => view.top = 0,
            KeyCode::End => view.top = last,
            KeyCode::Char('n') => self.jump_change(true),
            KeyCode::Char('p') => self.jump_change(false),
            KeyCode::Char('o') => {
                self.compare = None;
                self.run_conflict_command("workbench.files.action.saveIgnoreModifiedSince");
            }
            KeyCode::Char('r') => {
                self.compare = None;
                self.run_conflict_command("workbench.files.action.revertLocalChanges");
            }
            _ => {}
        }
    }

    /// Scrolls to the next or previous change, and says when there is none.
    fn jump_change(&mut self, forward: bool) {
        let Some(view) = self.compare.as_mut() else {
            return;
        };
        let count = view.model.changes.len();
        let target = if forward {
            Some(view.current + 1).filter(|n| *n < count)
        } else {
            view.current.checked_sub(1)
        };
        match target {
            Some(index) => {
                view.current = index;
                view.top = view.model.changes[index].saturating_sub(1);
                self.message = None;
            }
            None => {
                self.message = Some(
                    if count == 0 {
                        "There are no changes to go to"
                    } else if forward {
                        "This is the last change"
                    } else {
                        "This is the first change"
                    }
                    .to_owned(),
                );
            }
        }
    }
}

/// Draws the comparison in `area`: a title row, the lines, a key hint row.
pub(crate) fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let theme = app.theme;
    let Some(view) = app.compare.as_mut() else {
        return;
    };
    frame.buffer_mut().set_style(area, theme.editor);
    let body = usize::from(area.height.saturating_sub(2));
    view.page = body.max(1);
    let notice_rows = usize::from(view.model.notice.is_some());
    let room = body.saturating_sub(notice_rows);
    view.top = view
        .top
        .min(view.model.lines.len().saturating_sub(room.max(1)));
    let buffer = frame.buffer_mut();
    let model = &view.model;
    let change = if model.changes.is_empty() {
        String::new()
    } else {
        format!("  change {}/{}", view.current + 1, model.changes.len())
    };
    let head = format!(
        " {}  +{} -{}{change}",
        model.title, model.added, model.removed
    );
    buffer.set_line(
        area.x,
        area.y,
        &Line::styled(head, theme.sidebar_header),
        area.width,
    );
    let mut y = area.y + 1;
    if let Some(notice) = &model.notice {
        buffer.set_line(
            area.x,
            y,
            &Line::styled(format!(" {notice}"), theme.description),
            area.width,
        );
        y += 1;
    }
    for line in model.lines.iter().skip(view.top).take(room) {
        let (mark, style) = match line.kind {
            DiffKind::Same => (' ', theme.editor),
            DiffKind::Removed => ('-', theme.diff[1]),
            DiffKind::Added => ('+', theme.diff[0]),
        };
        let number = |n: Option<usize>| n.map_or_else(|| " ".repeat(5), |n| format!("{n:>5}"));
        let gutter = format!(" {} {} ", number(line.disk), number(line.mine));
        let row = Line::from(vec![
            Span::styled(gutter, theme.gutter),
            Span::styled(format!("{mark} {}", line.text), style),
        ]);
        buffer.set_line(area.x, y, &row, area.width);
        y += 1;
    }
    let hint = " n/p next/previous change   o Overwrite   r Revert   Esc close";
    let last = area.y + area.height.saturating_sub(1);
    buffer.set_line(
        area.x,
        last,
        &Line::styled(hint, theme.description),
        area.width,
    );
}
