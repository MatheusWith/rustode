//! Why an extension operation failed.
//!
//! Every variant names the item it is about (an extension id, a file, a
//! URL) and, where the user can do something about it, says what. Context
//! is added where it is known: a helper that holds a path puts it in the
//! error, and the job that knows the extension reports it with the id
//! ([`super::ExtensionEvent::Failed`]). The messages are sentences for the
//! user, so they carry no `Debug` output or Rust type names.

use super::ExtensionError;
use super::manifest::ManifestError;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// A failure of the extensions folder, the state file, a VSIX, the registry
/// or an install.
#[derive(Debug)]
pub(crate) enum Failure {
    /// An operation on a file or folder failed.
    Io {
        /// What was being done, e.g. `write`.
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// The extensions index is not a valid index; it is never overwritten.
    InvalidIndex(PathBuf),
    /// The state file is not a valid state file; it is never overwritten.
    InvalidState { path: PathBuf, what: String },
    /// The obsolete-extensions file is unreadable or invalid; it is never
    /// overwritten.
    InvalidObsolete(PathBuf),
    /// The extension is not installed.
    NotInstalled(String),
    /// The extension is invalid, so its enablement cannot change.
    Invalid { name: String, reason: String },
    /// No extensions or storage folder was set, or the registry is refused.
    Setup(ExtensionError),
    /// A JSON value could not be written.
    Serialize(serde_json::Error),
    /// The download is not a zip archive.
    NotAVsix(String),
    /// The VSIX holds too many entries.
    TooManyEntries { count: usize, max: usize },
    /// An entry of the VSIX would leave the extension folder.
    UnsafeEntry(String),
    /// The VSIX expands beyond the limit.
    TooLarge { max: u64 },
    /// The VSIX of `id` has no readable `extension/package.json`.
    NoManifest { id: String },
    /// The VSIX's `package.json` of `id` is not valid.
    BadManifest { id: String, source: ManifestError },
    /// The VSIX holds another extension or version than the one asked for.
    WrongPackage {
        found: String,
        found_version: String,
        id: String,
        version: String,
    },
    /// The request failed before an answer: network, TLS, timeout.
    Network { url: String, reason: String },
    /// The registry answered with an error status.
    Http { url: String, status: u16 },
    /// The registry's answer cannot be read.
    BadAnswer { url: String, reason: String },
    /// A redirect with no `Location`.
    RedirectWithoutLocation(String),
    /// More redirects than are followed.
    TooManyRedirects(String),
    /// The registry answered for another extension.
    WrongExtension {
        url: String,
        found: String,
        id: String,
    },
    /// The release has a version that is not a version.
    BadVersion { id: String, version: String },
    /// The release has no download.
    NoDownload { id: String, version: String },
    /// The release has no SHA-256, so the download cannot be checked.
    NoChecksum { id: String, version: String },
    /// The published SHA-256 is not a SHA-256.
    BadChecksum(String),
    /// The download does not match the published SHA-256.
    ChecksumMismatch {
        url: String,
        actual: String,
        expected: String,
    },
    /// The download could not be saved.
    Save { url: String, source: io::Error },
    /// The extension has no build for this platform.
    NoPlatformBuild {
        id: String,
        platform: &'static str,
        reason: Option<Box<Failure>>,
    },
    /// The thread that runs extension jobs could not be started.
    NoWorker,
    /// The system has no root certificates to check `https` with.
    NoRootCertificates { errors: usize },
}

impl Failure {
    pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for Failure {
    #[allow(clippy::too_many_lines)] // one arm per message
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "cannot {action} {}: {source}", path.display()),
            Self::InvalidIndex(path) => write!(
                f,
                "{} is not a valid extensions index; it was left unchanged. \
                 Fix or delete the file, then try again",
                path.display()
            ),
            Self::InvalidState { path, what } => write!(
                f,
                "{}: {what}; it was left unchanged. Fix or delete the file, then try again",
                path.display()
            ),
            Self::InvalidObsolete(path) => write!(
                f,
                "{} is not a valid list of extension folders to delete; it was left unchanged. \
                 Fix or delete the file, then try again",
                path.display()
            ),
            Self::NotInstalled(id) => write!(f, "{id} is not installed"),
            Self::Invalid { name, reason } => write!(
                f,
                "cannot change the enablement of {name}: it is invalid ({reason})"
            ),
            Self::Setup(err) => err.fmt(f),
            Self::Serialize(err) => write!(f, "cannot write the data: {err}"),
            Self::NotAVsix(reason) => {
                write!(f, "the download is not a VSIX (a zip archive): {reason}")
            }
            Self::TooManyEntries { count, max } => {
                write!(
                    f,
                    "the VSIX has {count} entries; at most {max} are extracted"
                )
            }
            Self::UnsafeEntry(name) => {
                write!(f, "{name}: refused, the path leaves the extension folder")
            }
            Self::TooLarge { max } => write!(f, "the VSIX expands beyond {max} bytes"),
            Self::NoManifest { id } => {
                write!(f, "the VSIX of {id} has no extension/package.json")
            }
            Self::BadManifest { id, source } => {
                write!(
                    f,
                    "the package.json in the VSIX of {id} is not valid: {source}"
                )
            }
            Self::WrongPackage {
                found,
                found_version,
                id,
                version,
            } => write!(
                f,
                "the VSIX holds {found} {found_version}, not {id} {version}"
            ),
            Self::Network { url, reason } => write!(
                f,
                "{url}: {reason}. Check the network connection and the registry's address"
            ),
            Self::Http { url, status } => write!(f, "{url}: HTTP {status}"),
            Self::BadAnswer { url, reason } => write!(f, "{url}: unexpected answer: {reason}"),
            Self::RedirectWithoutLocation(url) => write!(f, "{url}: redirect without a location"),
            Self::TooManyRedirects(url) => write!(f, "{url}: too many redirects"),
            Self::WrongExtension { url, found, id } => {
                write!(f, "{url}: the registry answered for {found}, not {id}")
            }
            Self::BadVersion { id, version } => {
                write!(f, "{id}: the registry gave an invalid version, {version}")
            }
            Self::NoDownload { id, version } => write!(f, "{id} {version} has no download"),
            Self::NoChecksum { id, version } => write!(
                f,
                "{id} {version}: Open VSX published no SHA-256, so the download cannot be checked"
            ),
            Self::BadChecksum(url) => write!(f, "{url}: not a SHA-256"),
            Self::ChecksumMismatch {
                url,
                actual,
                expected,
            } => write!(
                f,
                "{url}: the download's SHA-256 is {actual}, but Open VSX published {expected}. \
                 Try again; if it persists, the registry's file is damaged"
            ),
            Self::Save { url, source } => {
                write!(f, "cannot save the download of {url}: {source}")
            }
            Self::NoPlatformBuild {
                id,
                platform,
                reason,
            } => {
                write!(f, "{id} has no {platform} build")?;
                match reason {
                    Some(reason) => write!(f, ": {reason}"),
                    None => Ok(()),
                }
            }
            Self::NoWorker => f.write_str(
                "cannot start the extensions worker thread; try again, or restart rustode",
            ),
            Self::NoRootCertificates { errors } => write!(
                f,
                "no root certificates found on this system ({errors} errors), so https \
                 registries cannot be checked. Install the system's CA certificates"
            ),
        }
    }
}

impl std::error::Error for Failure {}

impl From<ExtensionError> for Failure {
    fn from(err: ExtensionError) -> Self {
        Self::Setup(err)
    }
}

impl From<serde_json::Error> for Failure {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialize(err)
    }
}
