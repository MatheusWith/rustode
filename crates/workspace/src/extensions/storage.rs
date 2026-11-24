//! The extensions folder, in VS Code's layout, so that a later extension
//! host finds installed extensions where VS Code would:
//!
//! - each extension in `<folder>/<publisher>.<name>-<version>[-<platform>]/`
//!   holding the VSIX's `extension/` contents (`ExtensionKey.toString`,
//!   `src/vs/platform/extensionManagement/common/extensionManagementUtil.ts:55-57`;
//!   `extractUserExtension`, `src/vs/platform/extensionManagement/node/extensionManagementService.ts:619-698`),
//!   extracted into a `.`-prefixed temporary folder first and renamed into
//!   place, with `__metadata` (install time, platform, size) added to its
//!   `package.json`, whose other fields keep their order, rewritten with
//!   tabs (`extensionsScannerService.ts:39-43`, `:304-311`;
//!   `extensionManagementService.ts:654-663`);
//! - an `extensions.json` index: an array of `{identifier, version,
//!   location, relativeLocation, metadata}` in that key order, where
//!   `location` is a `file` URI (`$mid`, `path`, `scheme`) kept for old
//!   readers and `relativeLocation` the folder name
//!   (`extensionsProfileScannerService.ts:23-29`, `:243-298`, `:394-401`;
//!   `src/vs/base/common/uri.ts:479-513`), with the metadata an install from
//!   the gallery records, in its order (`extensionManagementService.ts:1083-1132`);
//! - listing reads each indexed extension's manifest and checks it
//!   (`manifest.rs`); one whose manifest cannot be read is listed as
//!   invalid with a manifest made up from its index entry, as VS Code
//!   lists it (`extensionsScannerService.ts:677-700`);
//! - uninstalling removes the index entry, then the folder: it is marked in
//!   `.obsolete`, renamed to `<folder>.<hex>.vsctmp`, deleted, and unmarked
//!   (`extensionManagementService.ts:721-756`, `:813-850`). VS Code deletes it
//!   at the next start, once no profile uses it (`extensionsWatcher.ts:105-136`);
//!   rustode loads nothing, so it deletes it at once. A listing first
//!   removes what an interrupted deletion left: `.vsctmp` folders and
//!   folders still marked obsolete (`extensionManagementService.ts:561-567`,
//!   `:959-1000`).

use super::error::Failure;
use super::manifest::{ExtensionManifest, ManifestProblem};
use super::registry::{Release, UNIVERSAL};
use super::state::Identifier;
use super::{EnablementState, InstalledExtension};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const INDEX: &str = "extensions.json";
const OBSOLETE: &str = ".obsolete";
const DELETED: &str = ".vsctmp";

/// A random hex string for temporary names.
pub(crate) fn random_hex() -> String {
    format!("{:016x}", RandomState::new().hash_one(SystemTime::now()))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Writes `bytes` to `path` through a temporary file and a rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let name = path
        .file_name()
        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let temp = path.with_file_name(format!(".{name}.{}.tmp", random_hex()));
    fs::write(&temp, bytes).map_err(|err| Failure::io("write", &temp, err))?;
    fs::rename(&temp, path).map_err(|err| {
        let _ = fs::remove_file(&temp);
        Failure::io("write", path, err)
    })
}

/// The index entry's identifier, as VS Code compares it: lower case.
fn entry_id(entry: &Value) -> Option<String> {
    entry["identifier"]["id"]
        .as_str()
        .map(str::to_ascii_lowercase)
}

/// A `%key%` placeholder resolved from `package.nls.json`, as VS Code
/// localizes manifests (`src/vs/platform/extensionManagement/common/extensionNls.ts`).
fn localized(value: Option<&str>, nls: &Value) -> Option<String> {
    let value = value?;
    let key = value.strip_prefix('%').and_then(|v| v.strip_suffix('%'));
    match key.and_then(|key| nls.get(key)) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Object(message)) => message["message"].as_str().map(str::to_owned),
        _ => Some(value.to_owned()),
    }
}

/// An extracted extension waiting to be put in place.
pub(crate) struct Staged {
    pub release: Release,
    pub folder: PathBuf,
    pub size: u64,
}

/// What a listing found.
pub(crate) struct Scanned {
    pub(crate) list: Vec<InstalledExtension>,
    /// Why the `.obsolete` file could not be used, if it could not.
    pub(crate) obsolete_problem: Option<Failure>,
}

/// The extensions folder.
pub(crate) struct Store<'a> {
    pub dir: &'a Path,
}

impl Store<'_> {
    /// A path in the folder for temporary data, hidden like VS Code's
    /// `.<uuid>` extraction folders.
    pub(crate) fn temp(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!(".{}{suffix}", random_hex()))
    }

    /// The index entries; an empty list when there is no index yet.
    fn read_index(&self) -> Result<Vec<Value>, Failure> {
        let path = self.dir.join(INDEX);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(Failure::io("read", &path, err)),
        };
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Array(entries))
                if entries
                    .iter()
                    .all(|e| entry_id(e).is_some() && e["version"].is_string()) =>
            {
                Ok(entries)
            }
            _ => Err(Failure::InvalidIndex(path)),
        }
    }

    fn write_index(&self, entries: &[Value]) -> Result<(), Failure> {
        let bytes = serde_json::to_vec(entries)?;
        write_atomic(&self.dir.join(INDEX), &bytes)
    }

    /// Where an entry's extension lives: `relativeLocation` in the folder,
    /// else the path of its `file` URI.
    fn location(&self, entry: &Value) -> Option<PathBuf> {
        if let Some(relative) = entry["relativeLocation"].as_str().filter(|r| !r.is_empty()) {
            return super::vsix::safe_path(relative)
                .filter(|p| p.components().count() == 1)
                .map(|p| self.dir.join(p));
        }
        let location = &entry["location"];
        (location["scheme"].as_str() == Some("file"))
            .then(|| location["path"].as_str().map(PathBuf::from))
            .flatten()
    }

    /// Whether `path` is a folder directly inside the extensions folder.
    fn is_ours(&self, path: &Path) -> bool {
        path.parent() == Some(self.dir)
    }

    /// The folders marked obsolete; none when there is no file. A file that
    /// cannot be read or is not a map of names to `true` is an error: it is
    /// never replaced.
    fn obsolete(&self) -> Result<Map<String, Value>, Failure> {
        let path = self.dir.join(OBSOLETE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(err) => return Err(Failure::io("read", &path, err)),
        };
        if text.trim().is_empty() {
            return Ok(Map::new());
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(marked)) if marked.values().all(Value::is_boolean) => Ok(marked),
            _ => Err(Failure::InvalidObsolete(path)),
        }
    }

    fn set_obsolete(&self, key: &str, obsolete: bool) -> Result<(), Failure> {
        let mut marked = self.obsolete()?;
        if obsolete {
            marked.insert(key.to_owned(), Value::Bool(true));
        } else if marked.remove(key).is_none() {
            return Ok(());
        }
        let path = self.dir.join(OBSOLETE);
        if marked.is_empty() {
            return match fs::remove_file(&path) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => {
                    Err(Failure::io("delete", &path, err))
                }
                _ => Ok(()),
            };
        }
        let bytes = serde_json::to_vec(&marked)?;
        write_atomic(&path, &bytes)
    }

    /// Deletes an extension folder the way VS Code does: marked obsolete,
    /// renamed aside, deleted, unmarked. An interruption leaves it marked
    /// for the next listing to finish.
    fn delete_folder(&self, folder: &Path) -> Result<(), Failure> {
        let Some(key) = folder.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return Ok(());
        };
        self.set_obsolete(&key, true)?;
        let aside = folder.with_file_name(format!("{key}.{}{DELETED}", random_hex()));
        fs::rename(folder, &aside).map_err(|err| Failure::io("delete", folder, err))?;
        fs::remove_dir_all(&aside).map_err(|err| Failure::io("delete", &aside, err))?;
        self.set_obsolete(&key, false)
    }

    /// Removes what interrupted deletions left behind. An unusable
    /// `.obsolete` file is returned, so the listing can tell the user; it is
    /// never rewritten, and a change that needs it is refused.
    fn clean_up(&self, entries: &[Value]) -> Option<Failure> {
        self.remove_deleted_folders();
        let used: Vec<PathBuf> = entries.iter().filter_map(|e| self.location(e)).collect();
        let result = self.obsolete().map(|marked| {
            for key in marked.keys() {
                if let Err(err) = self.finish_obsolete(key, &used) {
                    tracing::warn!(%key, %err, "cannot remove an obsolete extension");
                }
            }
        });
        result.err().inspect(|err| {
            tracing::warn!(%err, "cannot finish deleting obsolete extensions");
        })
    }

    /// Deletes the `.vsctmp` folders left by interrupted deletions
    /// (`removeTemporarilyDeletedFolders`).
    fn remove_deleted_folders(&self) {
        let Ok(children) = fs::read_dir(self.dir) else {
            return;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_dir()
                && child.file_name().to_string_lossy().ends_with(DELETED)
                && let Err(err) = fs::remove_dir_all(&path)
            {
                tracing::warn!(path = %path.display(), %err, "cannot remove a deleted extension");
            }
        }
    }

    /// Deletes a folder marked obsolete unless the index still uses it;
    /// unmarks it once it is gone.
    fn finish_obsolete(&self, key: &str, used: &[PathBuf]) -> Result<(), Failure> {
        let Some(relative) = super::vsix::safe_path(key) else {
            return Ok(());
        };
        let folder = self.dir.join(relative);
        if !folder.exists() {
            self.set_obsolete(key, false)
        } else if used.contains(&folder) {
            Ok(())
        } else {
            self.delete_folder(&folder)
        }
    }

    /// The installed extensions, sorted by name, after cleaning up, with
    /// their enablement given the globally `disabled` ones. An extension
    /// whose manifest cannot be read is listed as invalid, as VS Code lists
    /// it (`extensionsScannerService.ts:677-700`).
    ///
    /// The second value is the problem with the `.obsolete` file, if it is
    /// unusable: the listing still works, and the file is left unchanged.
    pub(crate) fn scan(&self, disabled: &[Identifier]) -> Result<Scanned, Failure> {
        let entries = self.read_index()?;
        let obsolete_problem = self.clean_up(&entries);
        let mut list: Vec<InstalledExtension> = entries
            .iter()
            .filter_map(|entry| {
                let location = self.location(entry)?;
                Some(read_extension(entry, &location, disabled))
            })
            .collect();
        list.sort_by_key(|e| (e.display_name.to_lowercase(), e.id.clone()));
        Ok(Scanned {
            list,
            obsolete_problem,
        })
    }

    /// The installed extension `id` and its identifier, if it is installed.
    pub(crate) fn find(
        &self,
        id: &str,
        disabled: &[Identifier],
    ) -> Result<Option<(InstalledExtension, Identifier)>, Failure> {
        let entries = self.read_index()?;
        Ok(entries.iter().find_map(|entry| {
            let identifier = identifier(entry).filter(|i| i.id == id)?;
            let location = self.location(entry)?;
            Some((read_extension(entry, &location, disabled), identifier))
        }))
    }

    /// The installed version of `id`, if any; fails when the index is invalid.
    pub(crate) fn installed_version(&self, id: &str) -> Result<Option<String>, Failure> {
        let entries = self.read_index()?;
        Ok(entries
            .iter()
            .find(|e| {
                entry_id(e).as_deref() == Some(id) && self.location(e).is_some_and(|l| l.is_dir())
            })
            .and_then(|e| e["version"].as_str().map(str::to_owned)))
    }

    /// Puts a staged extension in place and indexes it, replacing any other
    /// version of it.
    pub(crate) fn add(&self, staged: &Staged) -> Result<(), Failure> {
        let release = &staged.release;
        let id = format!("{}.{}", release.namespace, release.name).to_ascii_lowercase();
        let mut entries = self.read_index()?;
        // Refused before anything changes if the obsolete list cannot be used.
        self.obsolete()?;
        check_manifest(&staged.folder, &id, &release.version)?;
        write_manifest_metadata(&staged.folder, release, staged.size)?;
        let key = format!("{id}-{}-{}", release.version, release.target_platform);
        let target = self.dir.join(&key);
        if target.exists() {
            self.delete_folder(&target)?;
        }
        fs::rename(&staged.folder, &target)
            .map_err(|err| Failure::io("install into", &target, err))?;
        let (old, kept): (Vec<Value>, Vec<Value>) = entries
            .drain(..)
            .partition(|e| entry_id(e).as_deref() == Some(id.as_str()));
        entries = kept;
        entries.push(new_entry(&id, release, &target, &key, !old.is_empty()));
        self.write_index(&entries)?;
        for folder in old.iter().filter_map(|e| self.location(e)) {
            if folder != target && self.is_ours(&folder) && folder.exists() {
                self.delete_folder(&folder)?;
            }
        }
        Ok(())
    }

    /// Removes `id` from the index, then deletes its folders.
    pub(crate) fn remove(&self, id: &str) -> Result<(), Failure> {
        let entries = self.read_index()?;
        // Refused before the index changes if the obsolete list cannot be used.
        self.obsolete()?;
        let (removed, kept): (Vec<Value>, Vec<Value>) = entries
            .into_iter()
            .partition(|e| entry_id(e).as_deref() == Some(id));
        if removed.is_empty() {
            return Err(Failure::NotInstalled(id.to_owned()));
        }
        self.write_index(&kept)?;
        let used: Vec<PathBuf> = kept.iter().filter_map(|e| self.location(e)).collect();
        for folder in removed.iter().filter_map(|e| self.location(e)) {
            if self.is_ours(&folder) && folder.exists() && !used.contains(&folder) {
                self.delete_folder(&folder)?;
            }
        }
        Ok(())
    }
}

/// The entry's identifier as VS Code reads it: the id in lower case, and the
/// gallery uuid if the entry has one (`extensionsProfileScannerService.ts:270-276`).
fn identifier(entry: &Value) -> Option<Identifier> {
    let id = entry_id(entry)?;
    let uuid = entry["metadata"]["id"]
        .as_str()
        .or_else(|| entry["identifier"]["uuid"].as_str())
        .map(str::to_owned);
    Some(Identifier { id, uuid })
}

/// The manifest in `location`, or the one VS Code makes up from the index
/// entry when it cannot be read, with the reason
/// (`extensionsScannerService.ts:681-700`, `:769-798`).
fn read_manifest(id: &str, entry: &Value, location: &Path) -> (ExtensionManifest, Option<String>) {
    let path = location.join("package.json");
    let read = fs::read_to_string(&path)
        .map_err(|err| Failure::io("read", &path, err).to_string())
        .and_then(|text| {
            ExtensionManifest::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
        });
    match read {
        Ok(manifest) => (manifest, None),
        Err(reason) => {
            let (publisher, name) = id.split_once('.').unwrap_or(("", id));
            let mut fields = Map::new();
            fields.insert("name".into(), name.into());
            fields.insert("publisher".into(), publisher.into());
            fields.insert("version".into(), entry["version"].clone());
            fields.insert("engines".into(), serde_json::json!({ "vscode": "" }));
            (ExtensionManifest::from_fields(fields), Some(reason))
        }
    }
}

/// An installed extension from its index entry and package.json, checked
/// as VS Code's scanner checks it.
fn read_extension(entry: &Value, location: &Path, disabled: &[Identifier]) -> InstalledExtension {
    let identifier = identifier(entry).unwrap_or(Identifier {
        id: String::new(),
        uuid: None,
    });
    let (manifest, unreadable) = read_manifest(&identifier.id, entry, location);
    let problems = match unreadable {
        Some(reason) => vec![ManifestProblem {
            severity: crate::Severity::Error,
            message: reason,
        }],
        None => manifest.validate(location),
    };
    let nls = fs::read_to_string(location.join("package.nls.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    let metadata = &entry["metadata"];
    let valid = !problems.iter().any(ManifestProblem::is_error);
    let is_disabled = disabled.iter().any(|d| d.same(&identifier));
    InstalledExtension {
        version: entry["version"]
            .as_str()
            .or(manifest.version())
            .unwrap_or_default()
            .to_owned(),
        display_name: localized(manifest.display_name(), &nls)
            .filter(|d| !d.is_empty())
            .or_else(|| manifest.name().map(str::to_owned))
            .unwrap_or_else(|| identifier.id.clone()),
        description: localized(manifest.description(), &nls).unwrap_or_default(),
        publisher_display_name: metadata["publisherDisplayName"].as_str().map(str::to_owned),
        target_platform: metadata["targetPlatform"].as_str().map(str::to_owned),
        location: location.to_owned(),
        enablement: EnablementState::of(is_disabled, valid),
        manifest,
        problems,
        id: identifier.id,
    }
}

/// The extracted package.json must name the extension and version asked
/// for, as VS Code checks the gallery's identity against the manifest
/// (`extensionManagementService.ts:309-311`).
fn check_manifest(folder: &Path, id: &str, version: &str) -> Result<(), Failure> {
    let path = folder.join("package.json");
    let text = fs::read_to_string(&path).map_err(|_| Failure::NoManifest { id: id.to_owned() })?;
    let manifest = ExtensionManifest::parse(&text).map_err(|source| Failure::BadManifest {
        id: id.to_owned(),
        source,
    })?;
    let found = format!(
        "{}.{}",
        manifest.publisher().unwrap_or_default(),
        manifest.name().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let found_version = manifest.version().unwrap_or_default();
    if found != id || found_version != version {
        return Err(Failure::WrongPackage {
            found,
            found_version: found_version.to_owned(),
            id: id.to_owned(),
            version: version.to_owned(),
        });
    }
    Ok(())
}

/// Adds `__metadata` to the extracted package.json, keeping what is there:
/// the fields stay in their order, `__metadata` is merged into or added
/// last, and the file is written with tabs, as `updateManifestMetadata`
/// writes it (`JSON.stringify(manifest, null, '\t')`,
/// `extensionsScannerService.ts:304-311`; keys in the order of
/// `extensionManagementService.ts:654-657`).
fn write_manifest_metadata(folder: &Path, release: &Release, size: u64) -> Result<(), Failure> {
    let path = folder.join("package.json");
    let id = format!("{}.{}", release.namespace, release.name);
    let text = fs::read_to_string(&path).map_err(|err| Failure::io("read", &path, err))?;
    let mut manifest: Value = serde_json::from_str(&text).map_err(|err| Failure::BadManifest {
        id: id.clone(),
        source: super::manifest::ManifestError::NotJson(err),
    })?;
    let Some(object) = manifest.as_object_mut() else {
        return Err(Failure::BadManifest {
            id,
            source: super::manifest::ManifestError::NotAnObject,
        });
    };
    let metadata = object
        .entry("__metadata")
        .or_insert_with(|| Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = Value::Object(Map::new());
    }
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert("installedTimestamp".into(), now_millis().into());
        metadata.insert(
            "targetPlatform".into(),
            release.target_platform.clone().into(),
        );
        metadata.insert("size".into(), size.into());
    }
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    manifest.serialize(&mut serializer)?;
    fs::write(&path, bytes).map_err(|err| Failure::io("write", &path, err))
}

/// The metadata VS Code records for an install from the gallery, in its
/// order; the fields it leaves undefined are left out, and the gallery's
/// `id` and `publisherId` (uuids) are unknown to Open VSX's REST API.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // one field per flag VS Code records
struct EntryMetadata<'a> {
    installed_timestamp: u64,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher_display_name: Option<&'a str>,
    target_platform: &'a str,
    updated: bool,
    private: bool,
    is_pre_release_version: bool,
    has_pre_release_version: bool,
    pre_release: bool,
}

fn new_entry(id: &str, release: &Release, folder: &Path, key: &str, updated: bool) -> Value {
    let metadata = EntryMetadata {
        installed_timestamp: now_millis(),
        source: "gallery",
        publisher_display_name: release.namespace_display_name.as_deref(),
        target_platform: if release.target_platform.is_empty() {
            UNIVERSAL
        } else {
            &release.target_platform
        },
        updated,
        private: false,
        is_pre_release_version: release.is_prerelease,
        has_pre_release_version: release.is_prerelease,
        pre_release: release.is_prerelease,
    };
    serde_json::json!({
        "identifier": { "id": id },
        "version": release.version,
        "location": {
            "$mid": 1,
            "path": folder.to_string_lossy(),
            "scheme": "file",
        },
        "relativeLocation": key,
        "metadata": metadata,
    })
}
