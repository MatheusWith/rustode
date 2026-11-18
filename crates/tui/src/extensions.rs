//! The Extensions view in the sidebar, like VS Code's: Ctrl+Shift+X shows
//! it with the search box focused, or, when it already has focus, moves
//! focus back to the editor (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:116-123`;
//! `src/vs/workbench/services/views/browser/viewsService.ts:426-441`). It
//! holds the search box ("Search Extensions in …", `extensionsViewlet.ts:647`),
//! the registry's results with Install / Installing / Installed
//! (`extensionsActions.ts:478`, `:765`, `:2728`), and the Installed list
//! (`extensionsViewlet.ts:350`) with Disable / Enable
//! (`extensionsActions.ts:1741-1850`), each extension's state (Enabled,
//! Disabled, `extensionsActions.ts:2745-2748`, or Invalid), disabled ones
//! dimmed (`extensionsList.ts:185-188`), and Uninstall / Uninstalling
//! (`extensionsActions.ts:939-940`). Every installed extension shows
//! "Not loaded in this phase", enabled or not: nothing runs in Phase 1.
//!
//! Keys: typing edits the search, Enter searches (VS Code searches as you
//! type); Up and Down select an extension; Enter installs the selected
//! result, or disables or enables the selected installed extension (for an
//! invalid one it tells why), Delete uninstalls it; Escape goes back to the
//! editor. All work goes through the workspace, in the background.

use crate::app::{App, Focus};
use crate::keys;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use rustode_workspace::{
    EnablementState, ExtensionEvent, ExtensionOperation, ExtensionSearch, ExtensionSummary,
    InstalledExtension, NOT_LOADED,
};
use unicode_width::UnicodeWidthStr;

/// Width of the sidebar while it shows the Extensions view.
pub(crate) const WIDTH: u16 = 44;
/// The search box's placeholder.
const PLACEHOLDER: &str = "Search Extensions in Open VSX";

/// What the view keeps between frames.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionsView {
    /// Whether the sidebar shows this view (else the explorer).
    pub shown: bool,
    /// The search box's text.
    pub query: String,
    /// The selected extension row; `None` is the search box.
    pub selected: Option<usize>,
    /// The first visible line of the list.
    pub top: usize,
}

/// One extension row of the view.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    Result(&'a ExtensionSummary),
    Installed(&'a InstalledExtension),
}

impl App {
    /// The rows: the search results, then the installed extensions.
    fn extension_rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        if let ExtensionSearch::Done { results, .. } = self.workspace.extension_search() {
            rows.extend(results.iter().map(Row::Result));
        }
        rows.extend(
            self.workspace
                .installed_extensions()
                .iter()
                .map(Row::Installed),
        );
        rows
    }

    /// Ctrl+Shift+X (`workbench.view.extensions`).
    pub(crate) fn show_extensions(&mut self) {
        if self.extensions.shown && self.focus == Focus::Extensions {
            if self.document().is_some() {
                self.focus = Focus::Editor;
            }
            return;
        }
        self.extensions.shown = true;
        self.search.shown = false;
        self.extensions.selected = None;
        self.focus = Focus::Extensions;
        if let Err(err) = self.workspace.refresh_extensions() {
            self.message = Some(err.to_string());
        }
    }

    /// A pasted text goes into the search box.
    pub(crate) fn extensions_paste(&mut self, text: &str) {
        self.extensions
            .query
            .push_str(text.lines().next().unwrap_or_default());
        self.extensions.selected = None;
    }

    /// Keys while the view has focus.
    pub(crate) fn extensions_key(&mut self, key: KeyEvent) {
        if let Some(command) = keys::global(key) {
            self.run_global(command, key);
            return;
        }
        let rows = self.extension_rows().len();
        let last = rows.checked_sub(1);
        let view = &mut self.extensions;
        let typing = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc if self.workspace.active_document().is_some() => {
                self.focus = Focus::Editor;
            }
            KeyCode::Down => {
                view.selected = match (view.selected, last) {
                    (_, None) => None,
                    (None, Some(_)) => Some(0),
                    (Some(row), Some(last)) => Some((row + 1).min(last)),
                };
            }
            KeyCode::Up => view.selected = view.selected.and_then(|row| row.checked_sub(1)),
            KeyCode::Home if view.selected.is_some() => view.selected = Some(0),
            KeyCode::End if view.selected.is_some() => view.selected = last,
            KeyCode::Enter => self.extensions_enter(),
            KeyCode::Delete => self.extensions_uninstall(),
            KeyCode::Backspace => {
                view.query.pop();
                view.selected = None;
            }
            KeyCode::Char(c) if typing => {
                view.query.push(c);
                view.selected = None;
            }
            _ => {}
        }
    }

    /// Enter: searches from the search box, installs a selected result,
    /// disables or enables a selected installed extension.
    fn extensions_enter(&mut self) {
        let Some(row) = self.extensions.selected else {
            let query = self.extensions.query.clone();
            self.workspace.search_extensions(&query);
            return;
        };
        let started = match self.extension_rows().get(row) {
            Some(Row::Result(result))
                if self.workspace.installed_extension(&result.id()).is_none() =>
            {
                let id = result.id();
                self.workspace.install_extension(&id)
            }
            Some(Row::Result(_)) => {
                self.message = Some("Installed; Delete uninstalls it".to_owned());
                return;
            }
            Some(Row::Installed(installed)) => {
                let (id, state) = (installed.id.clone(), installed.enablement);
                if state == EnablementState::Invalid {
                    self.message = Some(format!(
                        "{} is invalid: {}",
                        installed.display_name,
                        installed.first_error().unwrap_or_default()
                    ));
                    return;
                }
                if state.is_enabled() {
                    self.workspace.disable_extension(&id)
                } else {
                    self.workspace.enable_extension(&id)
                }
            }
            None => return,
        };
        if let Err(err) = started {
            self.message = Some(err.to_string());
        }
    }

    /// Delete: uninstalls the selected extension, if it is installed.
    fn extensions_uninstall(&mut self) {
        let Some(row) = self.extensions.selected else {
            return;
        };
        let id = match self.extension_rows().get(row) {
            Some(Row::Installed(installed)) => installed.id.clone(),
            Some(Row::Result(result)) => match self.workspace.installed_extension(&result.id()) {
                Some(installed) => installed.id.clone(),
                None => return,
            },
            None => return,
        };
        if let Err(err) = self.workspace.uninstall_extension(&id) {
            self.message = Some(err.to_string());
        }
    }
}

/// The status bar's message for an extension event, if it has one.
pub(crate) fn message(event: &ExtensionEvent) -> Option<String> {
    Some(match event {
        ExtensionEvent::Installed { id, version } => {
            format!("Installed {id} {version} ({})", NOT_LOADED.to_lowercase())
        }
        ExtensionEvent::Uninstalled { id } => format!("Uninstalled {id}"),
        ExtensionEvent::Enabled { id } => {
            format!("Enabled {id} ({})", NOT_LOADED.to_lowercase())
        }
        ExtensionEvent::Disabled { id } => format!("Disabled {id}"),
        ExtensionEvent::Failed { id, message } => format!("{id}: {message}"),
        ExtensionEvent::ListFailed { message } | ExtensionEvent::StateFailed { message } => {
            message.clone()
        }
        _ => return None,
    })
}

/// The action shown on a row: what Enter does, or the state of the work
/// under way.
fn action(app: &App, row: Row<'_>) -> &'static str {
    let id = match row {
        Row::Result(result) => result.id(),
        Row::Installed(installed) => installed.id.clone(),
    };
    match app.workspace.extension_operation(&id) {
        Some(ExtensionOperation::Installing) => "Installing",
        Some(ExtensionOperation::Uninstalling) => "Uninstalling",
        Some(ExtensionOperation::Enabling) => "Enabling",
        Some(ExtensionOperation::Disabling) => "Disabling",
        None => match row {
            Row::Result(_) if app.workspace.installed_extension(&id).is_some() => "Installed",
            Row::Result(_) => "Install",
            Row::Installed(installed) => match installed.enablement {
                EnablementState::Enabled => "Disable",
                EnablementState::DisabledGlobally => "Enable",
                EnablementState::Invalid => "",
            },
        },
    }
}

/// The two lines of a row: the name (and an installed extension's version)
/// with its action at the right, then a detail line, which for an
/// installed extension is its state and the not-loaded notice.
fn row_lines(app: &App, row: Row<'_>, width: usize) -> [String; 2] {
    let (name, detail) = match row {
        Row::Result(result) => {
            let installed = app.workspace.installed_extension(&result.id()).is_some();
            let detail = if installed {
                NOT_LOADED.to_owned()
            } else {
                format!("{} · {}", result.namespace, result.version)
            };
            (result.display_name.clone(), detail)
        }
        Row::Installed(installed) => (
            format!("{} {}", installed.display_name, installed.version),
            format!("{} · {}", installed.enablement.label(), installed.notice()),
        ),
    };
    let action = action(app, row);
    let room = width.saturating_sub(action.width() + 3);
    let name = truncate(&name, room);
    let gap = width.saturating_sub(name.width() + action.width() + 2);
    [
        format!(" {name}{} {action}", " ".repeat(gap)),
        truncate(&format!("   {detail}"), width),
    ]
}

/// Whether a row is drawn dimmed: an installed extension that is not
/// enabled, as VS Code dims disabled ones.
fn dimmed(row: Row<'_>) -> bool {
    matches!(row, Row::Installed(installed) if !installed.is_enabled())
}

fn truncate(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if out.width() + c.to_string().width() > width {
            break;
        }
        out.push(c);
    }
    out
}

fn to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The search state's line under the search box.
fn search_status(app: &App) -> Option<String> {
    Some(match app.workspace.extension_search() {
        ExtensionSearch::Idle => return None,
        ExtensionSearch::Searching { .. } => "Searching…".to_owned(),
        ExtensionSearch::Done { results, total, .. } if results.is_empty() => {
            format!("No extensions found ({total})")
        }
        ExtensionSearch::Done { results, total, .. } => {
            format!("OPEN VSX · {} of {total}", results.len())
        }
        ExtensionSearch::Failed { message, .. } => format!("Search failed: {message}"),
    })
}

/// The list's lines, each with the row it belongs to.
fn list_lines(app: &App, width: usize) -> Vec<(String, Option<usize>, Style)> {
    let theme = app.theme;
    let rows = app.extension_rows();
    let results = rows.iter().filter(|r| matches!(r, Row::Result(_))).count();
    let mut lines = Vec::new();
    if let Some(status) = search_status(app) {
        lines.push((
            truncate(&format!(" {status}"), width),
            None,
            theme.description,
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        if index == results {
            let count = rows.len() - results;
            lines.push((format!(" INSTALLED · {count}"), None, theme.sidebar_header));
        }
        let [first, second] = row_lines(app, *row, width);
        let style = if dimmed(*row) {
            theme.description
        } else {
            Style::new()
        };
        lines.push((first, Some(index), style));
        lines.push((second, Some(index), theme.description));
    }
    if rows.len() == results {
        lines.push((" INSTALLED · 0".to_owned(), None, theme.sidebar_header));
        lines.push((
            "   No extensions installed".to_owned(),
            None,
            theme.description,
        ));
    }
    lines
}

/// Draws the view in the sidebar's `area`.
pub(crate) fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let theme = app.theme;
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(theme.separator);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let width = usize::from(inner.width);
    let focused = app.focus == Focus::Extensions;
    let buffer = frame.buffer_mut();
    buffer.set_line(
        inner.x,
        inner.y,
        &Line::styled(" EXTENSIONS", theme.sidebar_header),
        inner.width,
    );
    let search = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    };
    buffer.set_style(search, theme.quick);
    let text = if app.extensions.query.is_empty() {
        Span::styled(format!(" {PLACEHOLDER}"), theme.description)
    } else {
        Span::raw(truncate(
            &format!(" {}", app.extensions.query),
            width.saturating_sub(1),
        ))
    };
    buffer.set_line(search.x, search.y, &Line::from(text), search.width);
    let lines = list_lines(app, width);
    let height = usize::from(inner.height - 2);
    if let Some(selected) = app.extensions.selected {
        let first = lines
            .iter()
            .position(|(_, row, _)| *row == Some(selected))
            .unwrap_or(0);
        if first < app.extensions.top {
            app.extensions.top = first.saturating_sub(1);
        } else if first + 2 > app.extensions.top + height {
            app.extensions.top = (first + 2).saturating_sub(height);
        }
    } else {
        app.extensions.top = 0;
    }
    let buffer = frame.buffer_mut();
    for (offset, (text, row, style)) in lines
        .iter()
        .skip(app.extensions.top)
        .take(height)
        .enumerate()
    {
        let mut style = *style;
        if focused && row.is_some() && *row == app.extensions.selected {
            style = style.patch(theme.explorer_selected);
        }
        let padded = format!("{text:<width$}");
        buffer.set_line(
            inner.x,
            inner.y + 2 + to_u16(offset),
            &Line::styled(padded, style),
            inner.width,
        );
    }
    if focused && app.extensions.selected.is_none() && app.quick.is_none() {
        let x = inner.x + 1 + to_u16(app.extensions.query.width());
        frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), search.y));
    }
}
