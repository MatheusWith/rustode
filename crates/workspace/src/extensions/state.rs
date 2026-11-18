//! Which extensions are disabled, kept the way VS Code keeps it, so a later
//! extension host can take the state over by copying it.
//!
//! VS Code keeps the globally disabled extensions in its profile storage
//! under the key `extensionsIdentifiers/disabled`
//! (`DISABLED_EXTENSIONS_STORAGE_PATH`,
//! `src/vs/platform/extensionManagement/common/extensionManagement.ts:667`),
//! as the JSON text of a list of `{id, uuid}` identifiers; the key is removed
//! when the list is empty, and ids compare by uuid when both have one, else
//! without case (`src/vs/platform/extensionManagement/common/extensionEnablementService.ts:51-92`,
//! `:108-163`; `areSameExtensions`, `extensionManagementUtil.ts:20-28`).
//! That storage is a SQLite table of key and value text,
//! `<profile>/globalStorage/state.vscdb` (`src/vs/platform/storage/electron-main/storageMain.ts:292-299`;
//! `src/vs/base/parts/storage/node/storage.ts:343`).
//!
//! rustode keeps the same rows in a JSON object instead: `state.json` in its
//! global storage folder, each key mapped to the very text VS Code would
//! store, so a migration is one row per key. Other keys are kept as they
//! are. A file that cannot be read is reported and never overwritten.

use super::storage::write_atomic;
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The file in the global storage folder.
pub const STATE_FILE: &str = "state.json";
/// VS Code's storage key for the globally disabled extensions.
pub const DISABLED_KEY: &str = "extensionsIdentifiers/disabled";

/// An extension identifier as VS Code stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Identifier {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

impl Identifier {
    /// `areSameExtensions`: by uuid when both have one, else by id
    /// without case.
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (&self.uuid, &other.uuid) {
            (Some(a), Some(b)) => a == b,
            _ => self.id.eq_ignore_ascii_case(&other.id),
        }
    }
}

/// The global storage folder.
pub(crate) struct State<'a> {
    pub dir: &'a Path,
}

impl State<'_> {
    fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    fn invalid(&self, what: &str) -> String {
        format!("{}: {what}; it was left unchanged", self.path().display())
    }

    /// Every row; none when there is no file yet.
    fn rows(&self) -> Result<Map<String, Value>, String> {
        let path = self.path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(err) => return Err(format!("{}: {err}", path.display())),
        };
        if text.trim().is_empty() {
            return Ok(Map::new());
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(rows)) if rows.values().all(Value::is_string) => Ok(rows),
            _ => Err(self.invalid("not a valid state file")),
        }
    }

    /// The globally disabled extensions.
    pub(crate) fn disabled(&self) -> Result<Vec<Identifier>, String> {
        self.disabled_in(&self.rows()?)
    }

    fn disabled_in(&self, rows: &Map<String, Value>) -> Result<Vec<Identifier>, String> {
        let Some(text) = rows.get(DISABLED_KEY).and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        let list: Vec<Value> = serde_json::from_str(text)
            .map_err(|_| self.invalid(&format!("{DISABLED_KEY} is not a list")))?;
        list.iter()
            .map(|entry| {
                let id = entry["id"].as_str()?.to_owned();
                let uuid = entry["uuid"].as_str().map(str::to_owned);
                Some(Identifier { id, uuid })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| self.invalid(&format!("{DISABLED_KEY} holds an entry without an id")))
    }

    /// Adds `identifier` to the disabled list, or removes it. Returns
    /// whether the list changed.
    pub(crate) fn set_disabled(
        &self,
        identifier: &Identifier,
        disabled: bool,
    ) -> Result<bool, String> {
        let mut rows = self.rows()?;
        let mut list = self.disabled_in(&rows)?;
        let present = list.iter().any(|e| e.same(identifier));
        if present == disabled {
            return Ok(false);
        }
        if disabled {
            list.push(identifier.clone());
        } else {
            list.retain(|e| !e.same(identifier));
        }
        if list.is_empty() {
            rows.remove(DISABLED_KEY);
        } else {
            let text = serde_json::to_string(&list).map_err(|err| err.to_string())?;
            rows.insert(DISABLED_KEY.to_owned(), Value::String(text));
        }
        fs::create_dir_all(self.dir).map_err(|err| format!("{}: {err}", self.dir.display()))?;
        let bytes = serde_json::to_vec(&rows).map_err(|err| err.to_string())?;
        write_atomic(&self.path(), &bytes)?;
        Ok(true)
    }
}
