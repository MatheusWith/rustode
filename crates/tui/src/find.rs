//! The find and replace widget, following VS Code's find widget
//! (`src/vs/editor/contrib/find/browser/findController.ts`): Ctrl+F finds
//! (:572), Ctrl+H replaces (:1010), Enter or F3 goes to the next match and
//! Shift+Enter or Shift+F3 to the previous one (:783-809), Escape closes
//! (:1078); in the replace field Enter replaces one match (:1171) and
//! Ctrl+Alt+Enter or Ctrl+Enter replaces all (:1182, :1195). Alt+C, Alt+W
//! and Alt+R toggle Match Case, Whole Word and Regular Expression
//! (`findModel.ts:39-50`). Typing searches from where the widget opened
//! (`findModel.ts:234-236`), and the status reads "3 of 12" or "No results"
//! (`findWidget.ts:78-79`, `:459-486`).

use crate::app::App;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustode_workspace::{FindOptions, FindQuery, MATCHES_LIMIT};

/// The widget's fields and toggles. It stays around when closed, so F3
/// repeats the last search.
#[derive(Debug, Clone, Default)]
pub(crate) struct FindWidget {
    pub query: String,
    pub replacement: String,
    pub options: FindOptions,
    /// Whether the replace field is shown.
    pub replace_mode: bool,
    /// Whether the replace field has the keyboard.
    pub in_replace: bool,
    /// Where the search starts while typing: the selection start on opening.
    pub origin: usize,
}

impl App {
    /// Ctrl+F (or Ctrl+H with `replace`): shows the widget, seeded with a
    /// selection that sits on one line, and searches from the selection.
    pub(crate) fn open_find(&mut self, replace: bool) {
        let Some(id) = self.document() else { return };
        let Some(document) = self.workspace.document(id) else {
            return;
        };
        let selection = document.primary_selection().range();
        let (start, end) = (
            document.position_of(selection.start),
            document.position_of(selection.end),
        );
        let seed = (start.line == end.line && start.column < end.column).then(|| {
            let text: String = document
                .line(start.line)
                .chars()
                .skip(start.column)
                .take(end.column - start.column)
                .collect();
            text
        });
        let widget = self.find.get_or_insert_with(FindWidget::default);
        if let Some(seed) = seed {
            widget.query = seed;
        }
        widget.origin = selection.start;
        widget.replace_mode |= replace;
        widget.in_replace = replace;
        self.find_visible = true;
        self.search_from_origin();
    }

    /// Keys while the widget is shown; returns whether it used the key.
    pub(crate) fn find_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let in_replace = self.find.as_ref().is_some_and(|f| f.in_replace);
        match key.code {
            KeyCode::Esc => self.find_visible = false,
            KeyCode::Enter if in_replace && ctrl => self.replace_all(),
            KeyCode::Enter if in_replace => self.replace_one(),
            KeyCode::Enter | KeyCode::F(3) => self.find_step(!shift),
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(find) = self.find.as_mut().filter(|f| f.replace_mode) {
                    find.in_replace = !find.in_replace;
                }
            }
            KeyCode::Char(c) if alt && !ctrl => return self.toggle_find_option(c),
            KeyCode::Char('h') if ctrl => self.open_find(true),
            KeyCode::Char('f') if ctrl => self.open_find(false),
            KeyCode::Backspace if !ctrl => self.edit_field(None),
            KeyCode::Char(c) if !ctrl => self.edit_field(Some(c)),
            _ => return false,
        }
        true
    }

    /// Alt+C, Alt+W, Alt+R.
    fn toggle_find_option(&mut self, c: char) -> bool {
        let Some(find) = self.find.as_mut() else {
            return false;
        };
        match c.to_ascii_lowercase() {
            'c' => find.options.match_case = !find.options.match_case,
            'w' => find.options.whole_word = !find.options.whole_word,
            'r' => find.options.regex = !find.options.regex,
            _ => return false,
        }
        self.search_from_origin();
        true
    }

    /// Types into (or, with `None`, deletes from) the field with the keyboard;
    /// the find field searches again as it changes.
    fn edit_field(&mut self, c: Option<char>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let field = if find.in_replace {
            &mut find.replacement
        } else {
            &mut find.query
        };
        match c {
            Some(c) => field.push(c),
            None => drop(field.pop()),
        }
        if !find.in_replace {
            self.search_from_origin();
        }
    }

    fn query(&self) -> Option<Result<FindQuery, String>> {
        let find = self.find.as_ref()?;
        Some(FindQuery::new(&find.query, find.options).map_err(|e| e.to_string()))
    }

    fn search_from_origin(&mut self) {
        let (Some(id), Some(origin)) = (self.document(), self.find.as_ref().map(|f| f.origin))
        else {
            return;
        };
        if let Some(Ok(query)) = self.query() {
            let result = self
                .workspace
                .set_cursor(id, origin)
                .and_then(|()| self.workspace.find_next(id, &query).map(|_| ()));
            if let Err(err) = result {
                self.message = Some(err.to_string());
            }
        }
    }

    /// F3 / Enter (forward) and Shift+F3 / Shift+Enter: the next or previous
    /// match, wrapping; works with the widget closed too, using the last query.
    pub(crate) fn find_step(&mut self, forward: bool) {
        let (Some(id), Some(Ok(query))) = (self.document(), self.query()) else {
            return;
        };
        let result = if forward {
            self.workspace.find_next(id, &query)
        } else {
            self.workspace.find_previous(id, &query)
        };
        if let Err(err) = result {
            self.message = Some(err.to_string());
        }
    }

    fn replace_one(&mut self) {
        let (Some(id), Some(Ok(query))) = (self.document(), self.query()) else {
            return;
        };
        let replacement = self
            .find
            .as_ref()
            .map_or_else(Default::default, |f| f.replacement.clone());
        if let Err(err) = self.workspace.replace_one(id, &query, &replacement) {
            self.message = Some(err.to_string());
        }
    }

    fn replace_all(&mut self) {
        let (Some(id), Some(Ok(query))) = (self.document(), self.query()) else {
            return;
        };
        let replacement = self
            .find
            .as_ref()
            .map_or_else(Default::default, |f| f.replacement.clone());
        match self.workspace.replace_all(id, &query, &replacement) {
            Ok(count) => self.message = Some(format!("Replaced {count} occurrence(s)")),
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// "3 of 12", "?" when the selection is not on a match, "No results", or
    /// the regex error, as VS Code's widget shows it.
    pub(crate) fn find_status(&self) -> String {
        let (Some(id), Some(query)) = (self.document(), self.query()) else {
            return String::new();
        };
        let query = match query {
            Ok(query) => query,
            Err(err) => return err,
        };
        if query.pattern().is_empty() {
            return String::new();
        }
        let matches = match self.workspace.find_all(id, &query) {
            Ok(matches) => matches,
            Err(err) => return err.to_string(),
        };
        if matches.is_empty() {
            return "No results".to_owned();
        }
        let total = if matches.len() >= MATCHES_LIMIT {
            format!("{MATCHES_LIMIT}+")
        } else {
            matches.len().to_string()
        };
        let selected = self
            .workspace
            .document(id)
            .map(|d| d.primary_selection().range());
        match matches.iter().position(|m| Some(*m) == selected) {
            Some(index) => format!("{} of {total}", index + 1),
            None => format!("? of {total}"),
        }
    }
}
