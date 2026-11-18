//! Extension management against Open VSX: search, install, uninstall,
//! enable, disable and the installed list. Phase 1 loads and runs nothing,
//! so every installed extension carries [`NOT_LOADED`] whether it is enabled
//! or not (ADR 0018, rule 3), and the Microsoft Marketplace is never
//! contacted (AGENTS.md §1.2): its hosts are refused before any request,
//! redirects included.
//!
//! Network and disk work never runs on the frontend's thread. Searches run
//! on threads of their own; installs, uninstalls, enablement changes and
//! listings run one at a time on a worker thread, so the extensions folder
//! and the global state file have one writer.
//! Results come back through [`Workspace::poll_extensions`], and an
//! optional waker ([`Workspace::set_extensions_waker`]) tells a frontend
//! that sleeps until input when to poll.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the search text is trimmed and cut at 200 chars, 50 results a page
//! (`src/vs/platform/extensionManagement/common/extensionGalleryService.ts:1173-1203`);
//! ids are `publisher.name`, compared without case
//! (`extensionManagementUtil.ts:80-86`, `extensionManagement.ts:22`); the
//! commands `workbench.extensions.installExtension`, `…uninstallExtension`
//! and `workbench.extensions.search`
//! (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:432-580`);
//! the enablement states and their rules: an extension is enabled unless the
//! user disabled it, an invalid one that the user did not disable is
//! "disabled by invalid extension", its enablement cannot change, and
//! uninstalling an extension forgets that it was disabled
//! (`src/vs/workbench/services/extensionManagement/common/extensionManagement.ts:101-116`;
//! `src/vs/workbench/services/extensionManagement/browser/extensionEnablementService.ts:94`,
//! `:278-304`, `:415-439`, `:454-481`, `:650-668`, `:730-740`, `:892-896`).
//! The storage layout is described in `storage.rs`, the enablement state's
//! in `state.rs`, the manifest model and its checks in `manifest.rs`,
//! extraction in `vsix.rs` and the registry client in `registry.rs`.

mod manifest;
mod registry;
#[cfg(feature = "stand-in")]
pub mod stand_in;
mod state;
mod storage;
mod vsix;

pub use manifest::{ExtensionManifest, ManifestProblem, VSCODE_API_DATE, VSCODE_API_VERSION};
pub use state::{DISABLED_KEY, STATE_FILE};

use crate::{Event, Workspace};
use registry::Registry;
use state::{Identifier, State};
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use storage::{Staged, Store};

/// The notice every installed extension carries in Phase 1.
pub const NOT_LOADED: &str = "Not loaded in this phase";
/// The registry used unless another one is set.
pub const OPEN_VSX: &str = "https://open-vsx.org";
/// Results asked for per search, VS Code's page size.
const PAGE_SIZE: usize = 50;
/// The longest search text sent, in chars, as in VS Code.
const MAX_QUERY: usize = 200;

/// An extension found in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSummary {
    /// The publisher (Open VSX namespace), as published.
    pub namespace: String,
    /// The extension's name, as published.
    pub name: String,
    /// The latest version.
    pub version: String,
    /// The name to show.
    pub display_name: String,
    /// One line about it.
    pub description: String,
    /// Whether Open VSX verified the publisher.
    pub verified: bool,
    /// How many times it was downloaded.
    pub download_count: u64,
}

impl ExtensionSummary {
    /// `publisher.name`, the id to install it with.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }
}

/// Whether an installed extension is enabled, as VS Code's
/// `EnablementState` says it for the whole profile. Workspace-level
/// enablement (`EnabledWorkspace`, `DisabledWorkspace`) is not kept yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnablementState {
    /// Enabled, the default (`EnabledGlobally`). Nothing runs it in Phase 1.
    Enabled,
    /// Disabled by the user everywhere (`DisabledGlobally`).
    DisabledGlobally,
    /// Not disabled by the user, but its manifest fails validation, so it
    /// can be neither run nor enabled or disabled
    /// (`DisabledByInvalidExtension`).
    Invalid,
}

impl EnablementState {
    /// The state of an extension the user disabled or not, valid or not.
    #[must_use]
    pub fn of(disabled: bool, valid: bool) -> Self {
        match (disabled, valid) {
            (true, _) => Self::DisabledGlobally,
            (false, true) => Self::Enabled,
            (false, false) => Self::Invalid,
        }
    }

    /// Whether it counts as enabled.
    #[must_use]
    pub fn is_enabled(self) -> bool {
        self == Self::Enabled
    }

    /// The word for it, as VS Code labels enabled and disabled extensions
    /// (`extensionsActions.ts:2745-2748`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "Enabled",
            Self::DisabledGlobally => "Disabled",
            Self::Invalid => "Invalid",
        }
    }
}

/// An extension in the extensions folder. Nothing loads it in Phase 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    /// `publisher.name`, in lower case.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// The name to show (the manifest's `displayName`, else its `name`).
    pub display_name: String,
    /// The manifest's description.
    pub description: String,
    /// The publisher's display name, as the registry gave it.
    pub publisher_display_name: Option<String>,
    /// The platform of the build: `universal`, `linux-x64`, …
    pub target_platform: Option<String>,
    /// Its folder.
    pub location: PathBuf,
    /// Its package.json; made up from the index when it cannot be read.
    pub manifest: ExtensionManifest,
    /// What validating the manifest found; an error makes it invalid.
    pub problems: Vec<ManifestProblem>,
    /// Whether it is enabled.
    pub enablement: EnablementState,
}

impl InstalledExtension {
    /// Whether its manifest passed validation (warnings allowed).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.problems.iter().any(ManifestProblem::is_error)
    }

    /// Whether it is enabled. Enabled or not, nothing loads it in Phase 1.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enablement.is_enabled()
    }

    /// The first error validation found, if any.
    #[must_use]
    pub fn first_error(&self) -> Option<&str> {
        self.problems
            .iter()
            .find(|p| p.is_error())
            .map(|p| p.message.as_str())
    }

    /// Whether the extension runs: never in Phase 1.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        false
    }

    /// What to show next to it: [`NOT_LOADED`].
    #[must_use]
    pub fn notice(&self) -> &'static str {
        NOT_LOADED
    }
}

/// The state of the last search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExtensionSearch {
    /// No search yet, or an empty one.
    #[default]
    Idle,
    /// Waiting for the registry.
    Searching {
        /// The text searched for.
        query: String,
    },
    /// The registry answered.
    Done {
        /// The text searched for.
        query: String,
        /// The results, most relevant first.
        results: Vec<ExtensionSummary>,
        /// How many extensions match in all.
        total: u64,
    },
    /// The search failed.
    Failed {
        /// The text searched for.
        query: String,
        /// Why.
        message: String,
    },
}

/// An install, uninstall or enablement change waiting to finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionOperation {
    /// Being downloaded and installed.
    Installing,
    /// Being uninstalled.
    Uninstalling,
    /// Being enabled.
    Enabling,
    /// Being disabled.
    Disabling,
}

/// Something that happened to extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtensionEvent {
    /// A search finished; see [`Workspace::extension_search`].
    Searched {
        /// The text searched for.
        query: String,
    },
    /// The installed list was read again.
    Listed,
    /// The installed list could not be read.
    ListFailed {
        /// Why.
        message: String,
    },
    /// An extension was installed (or its version was already there).
    Installed {
        /// `publisher.name`, in lower case.
        id: String,
        /// The installed version.
        version: String,
    },
    /// An extension was uninstalled.
    Uninstalled {
        /// `publisher.name`, in lower case.
        id: String,
    },
    /// A disabled extension was enabled. It still does not load.
    Enabled {
        /// `publisher.name`, in lower case.
        id: String,
    },
    /// An enabled extension was disabled.
    Disabled {
        /// `publisher.name`, in lower case.
        id: String,
    },
    /// The enablement state could not be read: every extension was listed
    /// as enabled, and the file is left unchanged.
    StateFailed {
        /// Why.
        message: String,
    },
    /// An install, uninstall or enablement change failed; nothing changed.
    Failed {
        /// `publisher.name`, in lower case.
        id: String,
        /// Why.
        message: String,
    },
}

/// Why an extension command was refused before it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    /// Not a `publisher.name` id.
    InvalidId(String),
    /// Not an `https` URL (or `http` on this machine).
    InvalidRegistry(String),
    /// A Microsoft Marketplace host: never contacted.
    Refused(String),
    /// No extensions folder was set.
    NoLocation,
    /// No global storage folder (for the enablement state) was set.
    NoStorageLocation,
    /// The extension is already being installed, uninstalled, enabled or
    /// disabled.
    Busy(String),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(f, "{id:?} is not an extension id (publisher.name)"),
            Self::InvalidRegistry(url) => {
                write!(
                    f,
                    "{url}: the registry must be an https URL (or http on this machine)"
                )
            }
            Self::Refused(host) => {
                write!(
                    f,
                    "{host} is the Microsoft Marketplace; rustode never contacts it"
                )
            }
            Self::NoLocation => f.write_str("no extensions folder is set"),
            Self::NoStorageLocation => {
                f.write_str("no global storage folder is set for the enablement state")
            }
            Self::Busy(id) => write!(f, "{id} is already being changed"),
        }
    }
}

impl std::error::Error for ExtensionError {}

/// rustode's extensions folder: `RUSTODE_EXTENSIONS_DIR`, else
/// `rustode/extensions` under `XDG_DATA_HOME` (when absolute), else under
/// `~/.local/share`; never a VS Code profile's folder. `env` reads a
/// variable, e.g. `|key| std::env::var_os(key)`.
pub fn default_extensions_dir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |key: &str| env(key).filter(|value| !value.is_empty());
    if let Some(dir) = set("RUSTODE_EXTENSIONS_DIR") {
        return Some(PathBuf::from(dir));
    }
    let data = set("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".local").join("share")))?;
    Some(data.join("rustode").join("extensions"))
}

/// rustode's global storage folder, which holds the enablement state
/// ([`STATE_FILE`]): `globalStorage` in the user folder, as VS Code keeps
/// `globalStorage/state.vscdb` in its `User` folder
/// (`src/vs/platform/userDataProfile/common/userDataProfile.ts:196`). The
/// user folder is `RUSTODE_USER_DIR`, else `rustode/User` under
/// `XDG_CONFIG_HOME` (when absolute), else under `~/.config`, where the
/// frontends read `settings.json`; never a VS Code profile's folder.
pub fn default_global_storage_dir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |key: &str| env(key).filter(|value| !value.is_empty());
    let user = match set("RUSTODE_USER_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => set("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".config")))?
            .join("rustode")
            .join("User"),
    };
    Some(user.join("globalStorage"))
}

type Waker = Arc<dyn Fn() + Send + Sync>;

/// Work for the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    List,
    Install(String),
    Uninstall(String),
    Enable(String),
    Disable(String),
}

impl Job {
    /// The extension the job is about, in lower case; empty for a listing.
    fn id(&self) -> String {
        match self {
            Self::List => String::new(),
            Self::Install(id) | Self::Uninstall(id) | Self::Enable(id) | Self::Disable(id) => {
                id.to_ascii_lowercase()
            }
        }
    }
}

/// What a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Done {
    Listed,
    Installed(String),
    Uninstalled,
    /// Whether the enablement changed.
    Enablement(bool),
}

struct Task {
    job: Job,
    dir: PathBuf,
    storage: Option<PathBuf>,
    registry: Arc<Registry>,
    waker: Option<Waker>,
}

enum Outcome {
    Searched {
        seq: u64,
        query: String,
        result: Result<(Vec<ExtensionSummary>, u64), String>,
    },
    Finished {
        job: Job,
        result: Result<Done, String>,
        list: Result<Vec<InstalledExtension>, String>,
        /// Why the enablement state could not be read, if it could not.
        state_problem: Option<String>,
    },
}

/// The workspace's extension state.
#[derive(Default)]
pub(crate) struct Extensions {
    dir: Option<PathBuf>,
    storage: Option<PathBuf>,
    registry: Option<Arc<Registry>>,
    waker: Option<Waker>,
    worker: Option<Sender<Task>>,
    outcomes: Option<(Sender<Outcome>, Receiver<Outcome>)>,
    search: ExtensionSearch,
    search_seq: u64,
    installed: Vec<InstalledExtension>,
    pending: Vec<(String, ExtensionOperation)>,
    jobs: usize,
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("dir", &self.dir)
            .field("storage", &self.storage)
            .field("registry", &self.registry.as_ref().map(|r| r.base()))
            .field("search", &self.search)
            .field("installed", &self.installed.len())
            .field("pending", &self.pending)
            .field("jobs", &self.jobs)
            .finish_non_exhaustive()
    }
}

impl Extensions {
    fn registry(&mut self) -> Arc<Registry> {
        Arc::clone(self.registry.get_or_insert_with(|| {
            Arc::new(
                Registry::new(OPEN_VSX).unwrap_or_else(|_| unreachable!("Open VSX's URL is valid")),
            )
        }))
    }

    fn sender(&mut self) -> Sender<Outcome> {
        self.outcomes.get_or_insert_with(mpsc::channel).0.clone()
    }

    /// Sends `job` to the worker, starting it on first use.
    fn queue(&mut self, job: Job) -> Result<(), ExtensionError> {
        let dir = self.dir.clone().ok_or(ExtensionError::NoLocation)?;
        let task = Task {
            job,
            dir,
            storage: self.storage.clone(),
            registry: self.registry(),
            waker: self.waker.clone(),
        };
        let outcomes = self.sender();
        let worker = self.worker.get_or_insert_with(|| spawn_worker(outcomes));
        if let Err(mpsc::SendError(task)) = worker.send(task) {
            // The worker is gone (it panicked): start another one.
            let worker = spawn_worker(self.sender());
            if let Err(mpsc::SendError(task)) = worker.send(task) {
                // No worker thread could start: report the job as failed.
                let _ = self.sender().send(Outcome::Finished {
                    job: task.job,
                    result: Err("cannot start the extensions worker thread".to_owned()),
                    list: Ok(self.installed.clone()),
                    state_problem: None,
                });
            }
            self.worker = Some(worker);
        }
        self.jobs += 1;
        Ok(())
    }

    fn operation(&self, id: &str) -> Option<ExtensionOperation> {
        self.pending
            .iter()
            .find(|(pending, _)| pending == id)
            .map(|(_, operation)| *operation)
    }

    /// Applies one outcome; returns the events it makes.
    fn apply(&mut self, outcome: Outcome) -> Vec<ExtensionEvent> {
        let mut events = Vec::new();
        match outcome {
            Outcome::Searched { seq, query, result } => {
                if seq != self.search_seq {
                    return events;
                }
                self.search = match result {
                    Ok((results, total)) => ExtensionSearch::Done {
                        query: query.clone(),
                        results,
                        total,
                    },
                    Err(message) => ExtensionSearch::Failed {
                        query: query.clone(),
                        message,
                    },
                };
                events.push(ExtensionEvent::Searched { query });
            }
            Outcome::Finished {
                job,
                result,
                list,
                state_problem,
            } => {
                self.jobs = self.jobs.saturating_sub(1);
                let id = job.id();
                self.pending.retain(|(pending, _)| *pending != id);
                events.extend(job_event(&job, id, result));
                match list {
                    Ok(list) => {
                        self.installed = list;
                        events.push(ExtensionEvent::Listed);
                    }
                    Err(message) => events.push(ExtensionEvent::ListFailed { message }),
                }
                if let Some(message) = state_problem {
                    events.push(ExtensionEvent::StateFailed { message });
                }
            }
        }
        events
    }
}

/// The event a finished job reports, if any.
fn job_event(job: &Job, id: String, result: Result<Done, String>) -> Option<ExtensionEvent> {
    Some(match (job, result) {
        (Job::List, _) | (_, Ok(Done::Enablement(false) | Done::Listed)) => return None,
        (_, Err(message)) => ExtensionEvent::Failed { id, message },
        (_, Ok(Done::Installed(version))) => ExtensionEvent::Installed { id, version },
        (_, Ok(Done::Uninstalled)) => ExtensionEvent::Uninstalled { id },
        (Job::Enable(_), Ok(Done::Enablement(true))) => ExtensionEvent::Enabled { id },
        (_, Ok(Done::Enablement(true))) => ExtensionEvent::Disabled { id },
    })
}

fn spawn_worker(outcomes: Sender<Outcome>) -> Sender<Task> {
    let (sender, tasks) = mpsc::channel::<Task>();
    let spawned = std::thread::Builder::new()
        .name("rustode-extensions".to_owned())
        .spawn(move || {
            for task in tasks {
                let outcome = run(&task);
                if outcomes.send(outcome).is_err() {
                    break;
                }
                if let Some(waker) = &task.waker {
                    waker();
                }
            }
        });
    if let Err(err) = spawned {
        tracing::error!(%err, "cannot start the extensions worker");
    }
    sender
}

/// Runs one job on the worker thread.
fn run(task: &Task) -> Outcome {
    let store = Store { dir: &task.dir };
    let state = task.storage.as_deref().map(|dir| State { dir });
    let result = match &task.job {
        Job::List => std::fs::create_dir_all(&task.dir)
            .map(|()| Done::Listed)
            .map_err(|err| format!("{}: {err}", task.dir.display())),
        Job::Install(id) => install(&store, &task.registry, id).map(Done::Installed),
        Job::Uninstall(id) => uninstall(&store, state.as_ref(), &id.to_ascii_lowercase()),
        Job::Enable(id) => set_enablement(&store, state.as_ref(), &id.to_ascii_lowercase(), false),
        Job::Disable(id) => set_enablement(&store, state.as_ref(), &id.to_ascii_lowercase(), true),
    };
    if let Err(message) = &result {
        tracing::warn!(job = ?task.job, %message, "extension job failed");
    }
    let (disabled, state_problem) = match state.as_ref().map(State::disabled) {
        Some(Ok(disabled)) => (disabled, None),
        Some(Err(message)) => (Vec::new(), Some(message)),
        None => (Vec::new(), None),
    };
    Outcome::Finished {
        job: task.job.clone(),
        result,
        list: store.scan(&disabled),
        state_problem,
    }
}

/// Uninstalls `id`, then forgets that it was disabled, as VS Code resets
/// the enablement of an uninstalled extension.
fn uninstall(store: &Store<'_>, state: Option<&State<'_>>, id: &str) -> Result<Done, String> {
    store.remove(id)?;
    let identifier = Identifier {
        id: id.to_owned(),
        uuid: None,
    };
    if let Some(Err(err)) = state.map(|state| state.set_disabled(&identifier, false)) {
        tracing::warn!(%id, %err, "cannot reset the enablement of an uninstalled extension");
    }
    Ok(Done::Uninstalled)
}

/// Disables or enables the installed extension `id` for the whole profile;
/// refused for an extension that is not installed or is invalid.
fn set_enablement(
    store: &Store<'_>,
    state: Option<&State<'_>>,
    id: &str,
    disable: bool,
) -> Result<Done, String> {
    let state = state.ok_or_else(|| ExtensionError::NoStorageLocation.to_string())?;
    let disabled = state.disabled()?;
    let Some((extension, identifier)) = store.find(id, &disabled)? else {
        return Err(format!("{id} is not installed"));
    };
    if extension.enablement == EnablementState::Invalid {
        return Err(format!(
            "cannot change the enablement of {}: it is invalid ({})",
            extension.display_name,
            extension.first_error().unwrap_or_default()
        ));
    }
    state
        .set_disabled(&identifier, disable)
        .map(Done::Enablement)
}

/// Downloads, checks, extracts and indexes the latest release of `id`.
/// Returns the installed version.
fn install(store: &Store<'_>, registry: &Registry, id: &str) -> Result<String, String> {
    std::fs::create_dir_all(store.dir).map_err(|err| format!("{}: {err}", store.dir.display()))?;
    let (namespace, name) = id.split_once('.').unwrap_or((id, ""));
    let release = registry.release(namespace, name)?;
    if store
        .installed_version(&id.to_ascii_lowercase())?
        .as_deref()
        == Some(release.version.as_str())
    {
        return Ok(release.version);
    }
    let download = store.temp(".vsix");
    let folder = store.temp("");
    let result = (|| {
        let mut file =
            File::create(&download).map_err(|err| format!("{}: {err}", download.display()))?;
        registry.download(&release, &mut file)?;
        drop(file);
        let size = vsix::extract(&download, &folder)?;
        let version = release.version.clone();
        store.add(&Staged {
            release,
            folder: folder.clone(),
            size,
        })?;
        Ok(version)
    })();
    let _ = std::fs::remove_file(&download);
    if folder.exists() {
        let _ = std::fs::remove_dir_all(&folder);
    }
    result
}

/// `id` checked and trimmed; its case is kept for the registry's URLs.
fn checked_id(id: &str) -> Result<String, ExtensionError> {
    let id = id.trim();
    match id.split_once('.') {
        Some((publisher, name))
            if registry::is_id_part(publisher) && registry::is_id_part(name) =>
        {
            Ok(id.to_owned())
        }
        _ => Err(ExtensionError::InvalidId(id.to_owned())),
    }
}

impl Workspace {
    /// Sets the extensions folder. Nothing is read until a command needs it.
    pub fn set_extensions_location(&mut self, dir: &Path) {
        self.extensions.dir = Some(dir.to_owned());
    }

    /// The extensions folder, if one is set.
    #[must_use]
    pub fn extensions_location(&self) -> Option<&Path> {
        self.extensions.dir.as_deref()
    }

    /// Sets the global storage folder, which holds the enablement state
    /// ([`STATE_FILE`]). Without one, every extension lists as enabled and
    /// enablement cannot change.
    pub fn set_global_storage_location(&mut self, dir: &Path) {
        self.extensions.storage = Some(dir.to_owned());
    }

    /// The global storage folder, if one is set.
    #[must_use]
    pub fn global_storage_location(&self) -> Option<&Path> {
        self.extensions.storage.as_deref()
    }

    /// Sets the registry's base URL ([`OPEN_VSX`] by default).
    ///
    /// # Errors
    /// When the URL is not `https` (or `http` on this machine), or is a
    /// Microsoft Marketplace host.
    pub fn set_extension_registry(&mut self, url: &str) -> Result<(), ExtensionError> {
        self.extensions.registry = Some(Arc::new(Registry::new(url)?));
        Ok(())
    }

    /// The registry's base URL.
    #[must_use]
    pub fn extension_registry(&self) -> &str {
        self.extensions
            .registry
            .as_ref()
            .map_or(OPEN_VSX, |r| r.base())
    }

    /// Calls `waker` (from another thread) whenever extension results are
    /// ready, so a frontend that sleeps until input knows to poll.
    pub fn set_extensions_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.extensions.waker = Some(Arc::new(waker));
    }

    /// Reads the installed list again, in the background.
    ///
    /// # Errors
    /// When no extensions folder is set.
    pub fn refresh_extensions(&mut self) -> Result<(), ExtensionError> {
        self.extensions.queue(Job::List)
    }

    /// Searches the registry in the background; an empty text clears the
    /// search. A newer search replaces the results of an older one.
    pub fn search_extensions(&mut self, text: &str) {
        let query: String = text.trim().chars().take(MAX_QUERY).collect();
        self.extensions.search_seq += 1;
        if query.is_empty() {
            self.extensions.search = ExtensionSearch::Idle;
            return;
        }
        self.extensions.search = ExtensionSearch::Searching {
            query: query.clone(),
        };
        let seq = self.extensions.search_seq;
        let registry = self.extensions.registry();
        let outcomes = self.extensions.sender();
        let waker = self.extensions.waker.clone();
        let spawned = std::thread::Builder::new()
            .name("rustode-extension-search".to_owned())
            .spawn(move || {
                let result = registry.search(&query, PAGE_SIZE);
                if outcomes
                    .send(Outcome::Searched { seq, query, result })
                    .is_ok()
                    && let Some(waker) = waker
                {
                    waker();
                }
            });
        if let Err(err) = spawned {
            self.extensions.search = ExtensionSearch::Failed {
                query: text.trim().to_owned(),
                message: err.to_string(),
            };
        }
    }

    /// Installs the latest release of `id` (`publisher.name`) in the
    /// background.
    ///
    /// # Errors
    /// When `id` is not an extension id, no folder is set, or the
    /// extension is already being installed or uninstalled.
    pub fn install_extension(&mut self, id: &str) -> Result<(), ExtensionError> {
        self.start(id, ExtensionOperation::Installing)
    }

    /// Uninstalls `id` in the background.
    ///
    /// # Errors
    /// As [`Workspace::install_extension`].
    pub fn uninstall_extension(&mut self, id: &str) -> Result<(), ExtensionError> {
        self.start(id, ExtensionOperation::Uninstalling)
    }

    /// Enables the installed extension `id` for the whole profile, in the
    /// background. Enabled or not, nothing loads it in Phase 1.
    ///
    /// # Errors
    /// As [`Workspace::install_extension`], and when no global storage
    /// folder is set.
    pub fn enable_extension(&mut self, id: &str) -> Result<(), ExtensionError> {
        self.start(id, ExtensionOperation::Enabling)
    }

    /// Disables the installed extension `id` for the whole profile, in the
    /// background.
    ///
    /// # Errors
    /// As [`Workspace::enable_extension`].
    pub fn disable_extension(&mut self, id: &str) -> Result<(), ExtensionError> {
        self.start(id, ExtensionOperation::Disabling)
    }

    fn start(&mut self, id: &str, operation: ExtensionOperation) -> Result<(), ExtensionError> {
        let id = checked_id(id)?;
        let key = id.to_ascii_lowercase();
        if self.extensions.operation(&key).is_some() {
            return Err(ExtensionError::Busy(key));
        }
        let changes_enablement = matches!(
            operation,
            ExtensionOperation::Enabling | ExtensionOperation::Disabling
        );
        if changes_enablement && self.extensions.storage.is_none() {
            return Err(ExtensionError::NoStorageLocation);
        }
        let job = match operation {
            ExtensionOperation::Installing => Job::Install(id),
            ExtensionOperation::Uninstalling => Job::Uninstall(id),
            ExtensionOperation::Enabling => Job::Enable(id),
            ExtensionOperation::Disabling => Job::Disable(id),
        };
        self.extensions.queue(job)?;
        self.extensions.pending.push((key, operation));
        Ok(())
    }

    /// Takes the results that arrived and reports them as events. Returns
    /// whether anything changed. Never blocks.
    pub fn poll_extensions(&mut self) -> bool {
        let mut changed = false;
        loop {
            let Some((_, outcomes)) = &self.extensions.outcomes else {
                return changed;
            };
            let Ok(outcome) = outcomes.try_recv() else {
                return changed;
            };
            changed = true;
            let events = self.extensions.apply(outcome);
            self.events.extend(events.into_iter().map(Event::Extension));
        }
    }

    /// The last search.
    #[must_use]
    pub fn extension_search(&self) -> &ExtensionSearch {
        &self.extensions.search
    }

    /// The installed extensions, sorted by name, as last read.
    #[must_use]
    pub fn installed_extensions(&self) -> &[InstalledExtension] {
        &self.extensions.installed
    }

    /// The installed extension with this id, ignoring case.
    #[must_use]
    pub fn installed_extension(&self, id: &str) -> Option<&InstalledExtension> {
        self.extensions
            .installed
            .iter()
            .find(|e| e.id.eq_ignore_ascii_case(id))
    }

    /// The install, uninstall or enablement change under way for `id`, if
    /// any.
    #[must_use]
    pub fn extension_operation(&self, id: &str) -> Option<ExtensionOperation> {
        self.extensions.operation(&id.to_ascii_lowercase())
    }

    /// Whether a search, install, uninstall, enablement change or listing
    /// is under way.
    #[must_use]
    pub fn extensions_busy(&self) -> bool {
        self.extensions.jobs > 0
            || matches!(self.extensions.search, ExtensionSearch::Searching { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_publisher_dot_name() {
        assert_eq!(checked_id(" Acme.Hello "), Ok("Acme.Hello".to_owned()));
        for bad in [
            "acme", "acme.", ".hello", "a.b.c", "../x.y", "a/b.c", "a.b/../c",
        ] {
            assert!(
                matches!(checked_id(bad), Err(ExtensionError::InvalidId(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn errors_read_as_sentences() {
        let host = ExtensionError::Refused("example.vsassets.io".into()).to_string();
        assert!(host.contains("never contacts it"), "{host}");
        assert!(ExtensionError::NoLocation.to_string().contains("folder"));
        assert!(
            ExtensionError::NoStorageLocation
                .to_string()
                .contains("global storage")
        );
        assert!(
            ExtensionError::Busy("a.b".into())
                .to_string()
                .contains("a.b")
        );
        assert!(
            ExtensionError::InvalidRegistry("ftp://x".into())
                .to_string()
                .contains("https")
        );
        assert!(
            ExtensionError::InvalidId("x".into())
                .to_string()
                .contains("publisher.name")
        );
    }
}
