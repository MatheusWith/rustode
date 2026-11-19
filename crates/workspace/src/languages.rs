//! Language modes: what the picker lists, picking one, and following the
//! `files.associations` setting.
//!
//! VS Code's "Change Language Mode" (`ChangeLanguageAction`,
//! `src/vs/workbench/browser/parts/editor/editorStatus.ts:1110-1330`) lists
//! "Auto Detect" (only when the document is not empty, `:1215-1218`), then
//! every registered language sorted by name, ignoring case
//! (`getSortedRegisteredLanguageNames`,
//! `src/vs/editor/common/services/languagesRegistry.ts:285-297`). Each pick
//! has the language's name as label and `(id)` as description, with
//! ` - Configured Language` after the current one (`:1171-1181`), and its
//! extensions as detail (`:1185`). The query matches the label and the
//! description (`matchOnDescription`, `:1221`). The "Configure File
//! Association" and "Configure language based settings" entries and the
//! marketplace search entry are not offered: they open settings and the
//! extensions view, which the frontends reach by their own commands.
//!
//! "Auto Detect" detects again from the file name and the first line
//! (`:1258-1263`); VS Code falls back to guessing from the content, which
//! rustode does not do. A language picked from the list stays until another
//! pick, whatever `files.associations` says (`setLanguageId`, `:1294`).

use crate::commands::match_label;
use crate::language::{self, Associations};
use crate::{DocumentId, Error, Event, Workspace};

/// One entry of the language picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePick {
    /// The language id to pass to [`Workspace::set_language`]; `None` is
    /// "Auto Detect".
    pub id: Option<&'static str>,
    /// The language's name (`Python`), or `Auto Detect`.
    pub label: &'static str,
    /// `(python)`, with ` - Configured Language` for the current language;
    /// empty for "Auto Detect".
    pub description: String,
    /// The language's extensions, separated by spaces.
    pub extensions: String,
    /// Whether this is the document's language.
    pub current: bool,
    /// Matched chars of the label, for highlighting.
    pub positions: Vec<usize>,
}

impl Workspace {
    /// The user's `files.associations`.
    pub(crate) fn read_associations(&self) -> Associations {
        Associations::from_setting(self.config.value("files.associations"))
    }

    /// After the settings loaded: documents whose language the user did not
    /// pick follow the new associations.
    pub(crate) fn associations_changed(&mut self) {
        self.associations = self.read_associations();
        let mut changed = Vec::new();
        for (index, slot) in self.documents.iter_mut().enumerate() {
            let Some(document) = slot else { continue };
            let before = document.language_id();
            document.redetect_language(&self.associations);
            if document.language_id() != before {
                changed.push(DocumentId(index));
            }
        }
        for id in changed {
            self.language_changed(id);
        }
    }

    /// After a document's language changed: its language server follows and
    /// the frontends are told.
    fn language_changed(&mut self, id: DocumentId) {
        self.lsp_closing(id);
        self.lsp_opened(id);
        self.events.push(Event::LanguageChanged { id });
    }

    /// The entries of the language picker for document `id` that match
    /// `query` (every one for an empty query): "Auto Detect" first when
    /// the document is not empty, then the languages sorted by name.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn language_picks(&self, id: DocumentId, query: &str) -> Result<Vec<LanguagePick>, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let current = document.language_id();
        let mut picks = Vec::new();
        if document.editor().buffer().len_chars() > 0 {
            picks.push(LanguagePick {
                id: None,
                label: "Auto Detect",
                description: String::new(),
                extensions: String::new(),
                current: false,
                positions: Vec::new(),
            });
        }
        let mut languages: Vec<_> = language::all().collect();
        languages.sort_by_key(|l| l.name.to_lowercase());
        picks.extend(languages.into_iter().map(|l| {
            let is_current = l.id == current;
            LanguagePick {
                id: Some(l.id),
                label: l.name,
                description: if is_current {
                    format!("({}) - Configured Language", l.id)
                } else {
                    format!("({})", l.id)
                },
                extensions: l.extensions.join(" "),
                current: is_current,
                positions: Vec::new(),
            }
        }));
        Ok(picks
            .into_iter()
            .filter_map(|mut pick| {
                let text = format!("{} {}", pick.label, pick.description);
                let positions = match_label(&text, query)?;
                pick.positions = positions
                    .into_iter()
                    .take_while(|&p| p < pick.label.chars().count())
                    .collect();
                Some(pick)
            })
            .collect())
    }

    /// Changes document `id`'s language and highlights it again: `Some(id)`
    /// is a language picked from the list, `None` is "Auto Detect", which
    /// detects again from the file name and the first line.
    ///
    /// # Errors
    /// When there is no such document, or no such language.
    pub fn set_language(&mut self, id: DocumentId, language: Option<&str>) -> Result<(), Error> {
        let associations = self.read_associations();
        let document = self.document_mut(id)?;
        let before = document.language_id();
        if let Some(language_id) = language {
            let found = language::by_id(language_id)
                .ok_or_else(|| Error::UnknownLanguage(language_id.to_owned()))?;
            document.set_language(found, true);
        } else {
            let first_line = document.editor().buffer().line_text(0);
            let detected = language::detect(document.path(), &first_line, &associations);
            document.set_language(detected, false);
        }
        if document.language_id() != before {
            self.language_changed(id);
        }
        Ok(())
    }
}
