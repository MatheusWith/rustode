//! The Search view in the sidebar, like VS Code's: Ctrl+Shift+F shows it
//! with the search box focused and seeded from a one-line selection
//! (`workbench.action.findInFiles`,
//! `src/vs/workbench/contrib/search/browser/searchActionsFind.ts:203-258`;
//! `searchView.ts:1270-1285`). It holds the search box with the Match Case,
//! Match Whole Word and Use Regular Expression toggles (Alt+C, Alt+W,
//! Alt+R, `searchActionsNav.ts:80-135`), the "files to include" and "files
//! to exclude" fields, the result count, and the results: each file, then
//! its matches, as VS Code's list view shows them
//! (`search.defaultViewMode: list`, `search.contribution.ts:195-204`).
//!
//! Keys: typing edits the focused field and Enter searches (VS Code also
//! searches as you type); Tab and Shift+Tab move between the fields; Down
//! goes to the results, where Up and Down select, Enter opens a match in a
//! tab and moves focus to the editor, Space opens it and keeps focus here,
//! and Enter, Left and Right collapse or expand a file. Escape cancels a
//! running search (`search.action.cancel`, `searchActionsTopBar.ts:42-62`),
//! else goes back to the editor. A click on a result opens it. All work
//! goes through the workspace, in the background.
//!
//! Replace (Ctrl+Shift+H, `workbench.action.replaceInFiles`,
//! `searchActionsNav.ts:396-413`) adds a replace field below the search
//! box. Each match row then shows its line before and after the
//! replacement, `before[match→replacement]after`, where VS Code opens a
//! diff editor per file (a gap). Enter in the replace field asks to confirm
//! Replace All (`searchView.ts:833-837`) and a second Enter does it
//! (`search.action.replaceAll`); on a result row Ctrl+Enter or
//! Ctrl+Shift+1 replaces that match or that file's matches
//! (`search.action.replace`, `search.action.replaceAllInFile`) and Delete
//! dismisses it (`search.action.remove`). The work runs a few files at a
//! time, so the screen keeps drawing; the status line counts the files.

use crate::app::{App, Focus};
use crate::keys;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use rustode_workspace::{FindOptions, FindQuery, ReplaceScope, SearchQuery, SearchRow};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width of the sidebar while it shows the Search view.
pub(crate) const WIDTH: u16 = 44;
/// Lines above the results: the title, the fields and the status.
const HEADER: u16 = 5;

/// The view's text fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Field {
    #[default]
    Query,
    Replace,
    Include,
    Exclude,
}

/// What the view keeps between frames.
#[derive(Debug, Clone, Default)]
pub(crate) struct SearchView {
    /// Whether the sidebar shows this view.
    pub shown: bool,
    pub query: String,
    pub include: String,
    pub exclude: String,
    /// The replace field's text, and whether the field is shown.
    pub replacement: String,
    pub replace_shown: bool,
    /// Whether Replace All was asked and waits for a second Enter.
    pub confirming: bool,
    pub options: FindOptions,
    /// The field that takes typing while no result is selected.
    pub field: Field,
    /// The selected result row; `None` is the focused field.
    pub selected: Option<usize>,
    /// The first visible result row.
    pub top: usize,
    /// Why the last search could not start.
    pub error: Option<String>,
}

impl SearchView {
    /// The lines above the results.
    fn header(&self) -> u16 {
        HEADER + u16::from(self.replace_shown)
    }

    fn field_mut(&mut self) -> &mut String {
        match self.field {
            Field::Query => &mut self.query,
            Field::Replace => &mut self.replacement,
            Field::Include => &mut self.include,
            Field::Exclude => &mut self.exclude,
        }
    }
}

impl App {
    /// Ctrl+Shift+F (`workbench.action.findInFiles`): shows the view with
    /// the search box focused, seeded from the editor's selection.
    pub(crate) fn show_search(&mut self) {
        if let Some(seed) = self.workspace.search_seed(self.search.options.regex) {
            self.search.query = seed;
        }
        self.search.shown = true;
        self.extensions.shown = false;
        self.search.field = Field::Query;
        self.search.selected = None;
        self.search.confirming = false;
        self.focus = Focus::Search;
    }

    /// Ctrl+Shift+H (`workbench.action.replaceInFiles`): the same view with
    /// the replace field shown.
    pub(crate) fn show_replace(&mut self) {
        self.show_search();
        self.search.replace_shown = true;
    }

    /// Hands the replace field's text to the workspace, which builds the
    /// previews from it.
    fn sync_replacement(&mut self) {
        let text = if self.search.replace_shown {
            self.search.replacement.as_str()
        } else {
            ""
        };
        self.workspace.set_search_replacement(text);
    }

    /// Starts a search for the fields' text; a running one stops first.
    fn run_search(&mut self) {
        let view = &self.search;
        let find = match FindQuery::new(&view.query, view.options) {
            Ok(find) => find,
            Err(err) => {
                self.search.error = Some(err.to_string());
                return;
            }
        };
        let mut query = SearchQuery::new(find);
        query.include.clone_from(&view.include);
        query.exclude.clone_from(&view.exclude);
        self.search.error = self
            .workspace
            .start_search(query)
            .err()
            .map(|e| e.to_string());
        self.search.selected = None;
        self.search.top = 0;
    }

    /// A pasted text goes into the focused field.
    pub(crate) fn search_paste(&mut self, text: &str) {
        self.search
            .field_mut()
            .push_str(text.lines().next().unwrap_or_default());
        self.search.selected = None;
        self.search.confirming = false;
        self.sync_replacement();
    }

    /// Keys while the view has focus.
    pub(crate) fn search_key(&mut self, key: KeyEvent) {
        let confirmed = self.search.confirming && key.code == KeyCode::Enter;
        self.search.confirming = false;
        if confirmed {
            self.message = None;
            self.start_replace(ReplaceScope::All);
            return;
        }
        if let Some(command) = keys::global(key) {
            self.run_global(command, key);
            return;
        }
        self.search_key_in_view(key);
        self.sync_replacement();
    }

    fn search_key_in_view(&mut self, key: KeyEvent) {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.search_escape(),
            KeyCode::Char(c) if alt && !ctrl => self.toggle_search_option(c),
            KeyCode::Tab => self.move_field(true),
            KeyCode::BackTab => self.move_field(false),
            _ if self.search.selected.is_some() => self.results_key(key),
            KeyCode::Enter if self.search.field == Field::Replace => self.search_replace_all(),
            KeyCode::Enter => self.run_search(),
            KeyCode::Down if !self.workspace.search_results().files().is_empty() => {
                self.search.selected = Some(0);
            }
            KeyCode::Backspace => {
                self.search.field_mut().pop();
            }
            KeyCode::Char(c) if !ctrl => self.search.field_mut().push(c),
            _ => {}
        }
    }

    /// Escape: cancels a running search, else goes back to the editor.
    fn search_escape(&mut self) {
        if self.workspace.cancel_search() {
            return;
        }
        if self.document().is_some() {
            self.focus = Focus::Editor;
        }
    }

    /// Alt+C, Alt+W, Alt+R; a search already made runs again.
    fn toggle_search_option(&mut self, c: char) {
        let options = &mut self.search.options;
        match c.to_ascii_lowercase() {
            'c' => options.match_case = !options.match_case,
            'w' => options.whole_word = !options.whole_word,
            'r' => options.regex = !options.regex,
            _ => return,
        }
        if self.workspace.search_results().query().is_some() {
            self.run_search();
        }
    }

    /// Tab and Shift+Tab: the next or previous field.
    fn move_field(&mut self, forward: bool) {
        let fields = if self.search.replace_shown {
            vec![Field::Query, Field::Replace, Field::Include, Field::Exclude]
        } else {
            vec![Field::Query, Field::Include, Field::Exclude]
        };
        let at = fields
            .iter()
            .position(|f| *f == self.search.field)
            .unwrap_or(0);
        let next = (if forward {
            at + 1
        } else {
            at + fields.len() - 1
        }) % fields.len();
        self.search.field = fields[next];
        self.search.selected = None;
    }

    /// Keys while a result row is selected.
    fn results_key(&mut self, key: KeyEvent) {
        let code = key.code;
        let rows = self.workspace.search_results().rows();
        let last = rows.len().saturating_sub(1);
        let Some(row) = self.search.selected.map(|r| r.min(last)) else {
            return;
        };
        let page = usize::from(self.hits.search.map_or(1, |a| a.height.max(1)));
        match code {
            KeyCode::Up if row == 0 => self.search.selected = None,
            KeyCode::Up => self.search.selected = Some(row - 1),
            KeyCode::Down => self.search.selected = Some((row + 1).min(last)),
            KeyCode::PageUp => self.search.selected = Some(row.saturating_sub(page)),
            KeyCode::PageDown => self.search.selected = Some((row + page).min(last)),
            KeyCode::Home => self.search.selected = Some(0),
            KeyCode::End => self.search.selected = Some(last),
            KeyCode::Enter | KeyCode::Char('1' | '!')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.search_replace_selected(false);
            }
            KeyCode::Delete => self.search_remove(),
            KeyCode::Enter => self.open_search_row(rows.get(row).copied(), false),
            KeyCode::Char(' ') => self.open_search_row(rows.get(row).copied(), true),
            KeyCode::Left => match rows.get(row) {
                Some(&SearchRow::File {
                    file,
                    expanded: true,
                }) => {
                    self.workspace.toggle_search_file(file);
                }
                Some(&SearchRow::Match { file, .. }) => {
                    self.search.selected = rows.iter().position(|r| {
                        *r == SearchRow::File {
                            file,
                            expanded: true,
                        }
                    });
                }
                _ => {}
            },
            KeyCode::Right => {
                if let Some(&SearchRow::File {
                    file,
                    expanded: false,
                }) = rows.get(row)
                {
                    self.workspace.toggle_search_file(file);
                }
            }
            KeyCode::Char(c) => {
                self.search.selected = None;
                self.search.field_mut().push(c);
            }
            _ => {}
        }
    }

    /// The selected result row, as the workspace's indexes.
    fn selected_row(&self) -> Option<SearchRow> {
        let rows = self.workspace.search_results().rows();
        let row = self.search.selected?.min(rows.len().saturating_sub(1));
        rows.get(row).copied()
    }

    /// `search.action.replaceAll`: asks to confirm, as VS Code does.
    pub(crate) fn search_replace_all(&mut self) {
        match self.workspace.replace_confirmation(ReplaceScope::All) {
            Some(question) => {
                self.message = Some(question);
                self.search.confirming = true;
            }
            None => self.message = Some("There is nothing to replace".to_owned()),
        }
    }

    /// `search.action.replace` on a match, `search.action.replaceAllInFile`
    /// on a file; with `file`, a match row stands for its file.
    pub(crate) fn search_replace_selected(&mut self, file: bool) {
        let scope = match self.selected_row() {
            Some(SearchRow::Match { file: at, index }) if !file => {
                ReplaceScope::Match { file: at, index }
            }
            Some(SearchRow::Match { file: at, .. } | SearchRow::File { file: at, .. }) => {
                ReplaceScope::File(at)
            }
            None => {
                self.message = Some("Select a match or a file to replace".to_owned());
                return;
            }
        };
        self.start_replace(scope);
    }

    fn start_replace(&mut self, scope: ReplaceScope) {
        self.sync_replacement();
        if let Err(err) = self.workspace.start_replace(scope) {
            self.message = Some(err.to_string());
        }
    }

    /// `search.action.remove`: the selected match or file leaves the results.
    pub(crate) fn search_remove(&mut self) {
        let removed = match self.selected_row() {
            Some(SearchRow::Match { file, index }) => {
                self.workspace.dismiss_search_match(file, index)
            }
            Some(SearchRow::File { file, .. }) => self.workspace.dismiss_search_file(file),
            None => false,
        };
        if removed {
            let rows = self.workspace.search_results().rows().len();
            self.search.selected = self.search.selected.map(|r| r.min(rows.saturating_sub(1)));
            if rows == 0 {
                self.search.selected = None;
            }
        }
    }

    /// `rustode.search.undoReplace`.
    pub(crate) fn search_undo_replace(&mut self) {
        self.message = Some(match self.workspace.undo_replace() {
            Ok(files) => format!("Undid the replace in {files} file(s)"),
            Err(err) => err.to_string(),
        });
    }

    /// A replace ended: say how.
    pub(crate) fn search_replaced(&mut self) {
        let replacement = self.workspace.search_replacement().to_owned();
        let Some(outcome) = self.workspace.take_replace_outcome() else {
            return;
        };
        self.message = Some(match outcome {
            Ok(done) if done.unsaved.is_empty() => done.message(&replacement),
            Ok(done) => format!(
                "{} Not saved: {}",
                done.message(&replacement),
                done.unsaved.join("; ")
            ),
            Err(err) => err.to_string(),
        });
        self.search.selected = None;
    }

    /// Enter or Space on a row, or a click: a file collapses or expands, a
    /// match opens in a tab; Enter moves focus to the editor.
    fn open_search_row(&mut self, row: Option<SearchRow>, keep_focus: bool) {
        match row {
            Some(SearchRow::File { file, .. }) => {
                self.workspace.toggle_search_file(file);
            }
            Some(SearchRow::Match { file, index }) => {
                match self.workspace.open_search_match(file, index) {
                    Ok(_) if !keep_focus => self.focus = Focus::Editor,
                    Ok(_) => {}
                    Err(err) => self.message = Some(err.to_string()),
                }
            }
            None => {}
        }
    }

    /// A click on the `offset`th visible result row.
    pub(crate) fn search_click(&mut self, offset: usize) {
        self.focus = Focus::Search;
        let row = self.search.top + offset;
        let rows = self.workspace.search_results().rows();
        if let Some(found) = rows.get(row).copied() {
            self.search.selected = Some(row);
            self.open_search_row(Some(found), true);
        }
    }
}

fn truncate(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

fn to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The spans of one row, at most `width` columns wide.
fn row_spans(app: &App, row: SearchRow, width: usize) -> Vec<Span<'static>> {
    let theme = app.theme;
    let results = app.workspace.search_results();
    match row {
        SearchRow::File { file, expanded } => {
            let Some(result) = results.files().get(file) else {
                return Vec::new();
            };
            let marker = if expanded { '▾' } else { '▸' };
            let count = result.matches.len().to_string();
            let name = truncate(
                &format!(" {marker} {}", result.label),
                width.saturating_sub(count.width() + 2),
            );
            let room = width.saturating_sub(name.width() + count.width() + 2);
            let description = truncate(&format!("  {}", result.description), room);
            let gap = room - description.width();
            vec![
                Span::raw(name),
                Span::styled(description, theme.description),
                Span::raw(" ".repeat(gap)),
                Span::styled(format!(" {count} "), theme.description),
            ]
        }
        SearchRow::Match { file, index } => {
            let Some(found) = results.files().get(file).and_then(|f| f.matches.get(index)) else {
                return Vec::new();
            };
            let preview = found.preview();
            let mut left = width;
            let mut spans = Vec::new();
            let replacement = if app.search.replace_shown {
                app.workspace.search_replacement_text(file, index)
            } else {
                None
            };
            let mut parts = vec![
                (format!("     {}", preview.before), Style::new()),
                (preview.inside, theme.highlight),
            ];
            if let Some(text) = replacement {
                parts.push(("→".to_owned(), theme.description));
                parts.push((text, theme.quick_selected));
            }
            parts.push((preview.after, Style::new()));
            for (text, style) in parts {
                let text = truncate(&text.replace('\t', " "), left);
                left -= text.width();
                spans.push(Span::styled(text, style));
            }
            spans
        }
    }
}

/// The status line: a start error, the count, or why there is none.
fn status(app: &App) -> Option<(String, Style)> {
    let theme = app.theme;
    if let Some(error) = &app.search.error {
        return Some((error.clone(), theme.description));
    }
    if let Some(progress) = app.workspace.replace_progress() {
        return Some((
            format!("Replacing… {}/{}", progress.done, progress.total),
            theme.description,
        ));
    }
    let results = app.workspace.search_results();
    if let Some(message) = results.message() {
        return Some((message, theme.description));
    }
    results
        .is_running()
        .then(|| ("Searching…".to_owned(), theme.description))
}

/// A field's line: its label, then its text, or the placeholder.
fn field_line(label: &str, text: &str, width: usize, theme: &crate::theme::Theme) -> Line<'static> {
    if text.is_empty() {
        return Line::from(Span::styled(
            truncate(&format!(" {label}"), width),
            theme.description,
        ));
    }
    let label = format!(" {label}: ");
    let text = truncate(text, width.saturating_sub(label.width()));
    Line::from(vec![
        Span::styled(label, theme.description),
        Span::raw(text),
    ])
}

/// The title, the search box with its toggles, and the two glob fields.
fn header_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let theme = app.theme;
    let view = &app.search;
    let toggle = |on: bool, label: &str| {
        let style = if on {
            theme.quick_selected
        } else {
            theme.description
        };
        Span::styled(format!("{label} "), style)
    };
    let toggles = vec![
        toggle(view.options.match_case, "Aa"),
        toggle(view.options.whole_word, "ab"),
        toggle(view.options.regex, ".*"),
    ];
    let query_width = width.saturating_sub(10);
    let query = field_line("Search", &view.query, query_width, &theme);
    let used = query.width();
    let mut query_spans = query.spans;
    query_spans.push(Span::raw(" ".repeat(query_width.saturating_sub(used) + 1)));
    query_spans.extend(toggles);
    let mut lines = vec![
        Line::styled(" SEARCH", theme.sidebar_header),
        Line::from(query_spans),
    ];
    if view.replace_shown {
        lines.push(field_line("Replace", &view.replacement, width, &theme));
    }
    lines.push(field_line("files to include", &view.include, width, &theme));
    lines.push(field_line("files to exclude", &view.exclude, width, &theme));
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
    let header = app.search.header();
    if inner.height < header + 1 {
        return;
    }
    let width = usize::from(inner.width);
    let focused = app.focus == Focus::Search;
    let lines = header_lines(app, width);
    let buffer = frame.buffer_mut();
    for (offset, line) in (0_u16..).zip(lines) {
        let y = inner.y + offset;
        if (1..header - 1).contains(&offset) {
            buffer.set_style(
                Rect {
                    y,
                    height: 1,
                    ..inner
                },
                theme.quick,
            );
        }
        buffer.set_line(inner.x, y, &line, inner.width);
    }
    if let Some((text, style)) = status(app) {
        let warning = app
            .workspace
            .search_results()
            .warning()
            .map_or(String::new(), |w| format!(" · {w}"));
        let text = truncate(&format!(" {text}{warning}"), width);
        frame.buffer_mut().set_line(
            inner.x,
            inner.y + header - 1,
            &Line::styled(text, style),
            inner.width,
        );
    }
    let list = Rect {
        y: inner.y + header,
        height: inner.height - header,
        ..inner
    };
    app.hits.search = Some(list);
    draw_rows(app, frame, list, focused);
    if focused && app.search.selected.is_none() && app.quick.is_none() {
        let below = u16::from(app.search.replace_shown);
        let (y, label, text) = match app.search.field {
            Field::Query => (1, "Search", &app.search.query),
            Field::Replace => (2, "Replace", &app.search.replacement),
            Field::Include => (2 + below, "files to include", &app.search.include),
            Field::Exclude => (3 + below, "files to exclude", &app.search.exclude),
        };
        let prefix = if text.is_empty() {
            1
        } else {
            label.width() + 3
        };
        let x = inner.x + to_u16(prefix + text.width());
        frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y + y));
    }
}

/// The result rows that fit in `list`, scrolled to the selected one.
fn draw_rows(app: &mut App, frame: &mut Frame, list: Rect, focused: bool) {
    let rows = app.workspace.search_results().rows();
    let height = usize::from(list.height);
    match app.search.selected {
        Some(selected) if selected < app.search.top => app.search.top = selected,
        Some(selected) if selected >= app.search.top + height => {
            app.search.top = selected + 1 - height;
        }
        Some(_) => {}
        None => app.search.top = app.search.top.min(rows.len().saturating_sub(height)),
    }
    let width = usize::from(list.width);
    for (offset, (index, row)) in rows
        .iter()
        .enumerate()
        .skip(app.search.top)
        .take(height)
        .enumerate()
    {
        let mut line = Line::from(row_spans(app, *row, width));
        if focused && app.search.selected == Some(index) {
            line = line.patch_style(app.theme.explorer_selected);
        }
        let y = list.y + to_u16(offset);
        let buffer = frame.buffer_mut();
        if focused && app.search.selected == Some(index) {
            buffer.set_style(
                Rect {
                    y,
                    height: 1,
                    ..list
                },
                app.theme.explorer_selected,
            );
        }
        buffer.set_line(list.x, y, &line, list.width);
    }
}
