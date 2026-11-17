//! The Open VSX client: search, the latest release of an extension, and
//! downloads checked against the SHA-256 that Open VSX publishes.
//!
//! It speaks Open VSX's own REST API (`/api/-/search`, `/api/{namespace}/{name}`),
//! not the VS Code-compatible gallery API the App uses
//! (`{serviceUrl}/extensionquery`, `src/vs/platform/extensionManagement/common/extensionGalleryManifestService.ts:48-52`,
//! `extensionGalleryService.ts:1452-1512`): the REST API publishes a SHA-256
//! for every file, and it is not the Microsoft Marketplace's protocol, so a
//! changed setting cannot point this client at the Marketplace. Every URL
//! is checked before it is fetched, redirects included, and Marketplace
//! hosts are refused.

use super::{ExtensionError, ExtensionSummary};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use ureq::Agent;
use ureq::http::{Response, Uri};
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

/// Domains of the Microsoft Marketplace and its asset hosts, refused with
/// every subdomain. Written as domains, not full hosts, so that any host
/// under them is refused.
const MARKETPLACE_DOMAINS: [&str; 3] = ["visualstudio.com", "vsassets.io", "vscode-unpkg.net"];

/// Redirects followed per request, each one checked like the first URL.
const MAX_REDIRECTS: usize = 5;
/// The largest API answer read, in bytes.
const MAX_JSON: u64 = 8 << 20;
/// The largest VSIX downloaded, in bytes (rustode's limit; VS Code sets none).
pub(crate) const MAX_VSIX: u64 = 512 << 20;
/// The platform rustode runs on (ADR 0012), as Open VSX names it.
pub(crate) const PLATFORM: &str = "linux-x64";
/// Open VSX's name for an extension that runs everywhere.
pub(crate) const UNIVERSAL: &str = "universal";

/// The latest release of an extension, as Open VSX describes it.
#[derive(Debug, Clone)]
pub(crate) struct Release {
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub target_platform: String,
    pub namespace_display_name: Option<String>,
    pub is_prerelease: bool,
    pub download: String,
    pub sha256: String,
}

/// An Open VSX registry at a base URL.
#[derive(Debug)]
pub(crate) struct Registry {
    base: String,
    plain: OnceLock<Agent>,
    tls: OnceLock<Result<Agent, String>>,
}

/// Why a URL is not fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UrlProblem {
    /// Not an absolute `https` URL, or `http` to a host other than this machine.
    Invalid(String),
    /// A Microsoft Marketplace host.
    Marketplace(String),
}

impl From<UrlProblem> for ExtensionError {
    fn from(problem: UrlProblem) -> Self {
        match problem {
            UrlProblem::Invalid(url) => Self::InvalidRegistry(url),
            UrlProblem::Marketplace(host) => Self::Refused(host),
        }
    }
}

impl std::fmt::Display for UrlProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(url) => write!(
                f,
                "{url}: only https URLs (or http on this machine) are fetched"
            ),
            Self::Marketplace(host) => write!(
                f,
                "{host} is the Microsoft Marketplace; rustode never contacts it"
            ),
        }
    }
}

/// Whether `host` is a Microsoft Marketplace host, ignoring case and a
/// trailing dot.
pub(crate) fn is_marketplace(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    MARKETPLACE_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host == "[::1]" || host.starts_with("127.")
}

/// Checks a URL before it is fetched: absolute, `https` (or `http` to this
/// machine), and not the Marketplace.
pub(crate) fn check_url(url: &str) -> Result<(), UrlProblem> {
    let invalid = || UrlProblem::Invalid(url.to_owned());
    let uri: Uri = url.parse().map_err(|_| invalid())?;
    let host = uri.host().filter(|h| !h.is_empty()).ok_or_else(invalid)?;
    if is_marketplace(host) {
        return Err(UrlProblem::Marketplace(host.to_ascii_lowercase()));
    }
    match uri.scheme_str() {
        Some("https") => Ok(()),
        Some("http") if is_loopback(host) => Ok(()),
        _ => Err(invalid()),
    }
}

/// `location` (a redirect target) made absolute against `base`.
fn resolve(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    if let Some(network) = location.strip_prefix("//") {
        return format!("{scheme}://{network}");
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if location.starts_with('/') {
        return format!("{scheme}://{authority}{location}");
    }
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    let directory = path.rsplit_once('/').map_or(authority, |(dir, _)| dir);
    format!("{scheme}://{directory}/{location}")
}

/// `text` percent-encoded for a query string (RFC 3986 unreserved chars kept).
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `bytes` in lower-case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// An extension identifier part as VS Code accepts it: a letter or digit,
/// then letters, digits and dashes (`EXTENSION_IDENTIFIER_PATTERN`,
/// `src/vs/platform/extensionManagement/common/extensionManagement.ts:22`).
pub(crate) fn is_id_part(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A version safe to put in a folder name: `major.minor.patch` with an
/// optional pre-release and build part.
pub(crate) fn is_version(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-+.".contains(&b))
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(rename = "totalSize", default)]
    total_size: u64,
    #[serde(default)]
    extensions: Vec<RawEntry>,
}

#[derive(Deserialize)]
struct RawEntry {
    namespace: String,
    name: String,
    version: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    verified: bool,
    #[serde(rename = "downloadCount", default)]
    download_count: u64,
}

#[derive(Deserialize)]
struct RawExtension {
    namespace: String,
    name: String,
    version: String,
    #[serde(rename = "targetPlatform")]
    target_platform: Option<String>,
    #[serde(rename = "namespaceDisplayName")]
    namespace_display_name: Option<String>,
    #[serde(rename = "preRelease", default)]
    pre_release: bool,
    #[serde(default)]
    files: HashMap<String, String>,
}

impl Registry {
    /// A registry at `base`, e.g. `https://open-vsx.org`.
    pub(crate) fn new(base: &str) -> Result<Self, ExtensionError> {
        let base = base.trim().trim_end_matches('/');
        check_url(base)?;
        Ok(Self {
            base: base.to_owned(),
            plain: OnceLock::new(),
            tls: OnceLock::new(),
        })
    }

    /// The base URL.
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    /// Searches by text, most relevant first; returns the results and how
    /// many there are in all.
    pub(crate) fn search(
        &self,
        text: &str,
        size: usize,
    ) -> Result<(Vec<ExtensionSummary>, u64), String> {
        let url = format!(
            "{}/api/-/search?query={}&size={size}&offset=0&sortBy=relevance&sortOrder=desc",
            self.base,
            encode(text)
        );
        let raw: RawSearch = self.json(&url)?;
        let results = raw
            .extensions
            .into_iter()
            .filter(|e| is_id_part(&e.namespace) && is_id_part(&e.name))
            .map(|e| ExtensionSummary {
                display_name: e
                    .display_name
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| e.name.clone()),
                namespace: e.namespace,
                name: e.name,
                version: e.version,
                description: e.description.unwrap_or_default(),
                verified: e.verified,
                download_count: e.download_count,
            })
            .collect();
        Ok((results, raw.total_size))
    }

    /// The latest release for this platform: the latest one when it is
    /// universal or for linux-x64, else the latest linux-x64 build.
    pub(crate) fn release(&self, namespace: &str, name: &str) -> Result<Release, String> {
        let latest = self.release_at(
            &format!("{}/api/{namespace}/{name}", self.base),
            namespace,
            name,
        )?;
        if latest.target_platform == UNIVERSAL || latest.target_platform == PLATFORM {
            return Ok(latest);
        }
        let url = format!("{}/api/{namespace}/{name}/{PLATFORM}", self.base);
        let release = self
            .release_at(&url, namespace, name)
            .map_err(|err| format!("{namespace}.{name} has no {PLATFORM} build: {err}"))?;
        if release.target_platform == UNIVERSAL || release.target_platform == PLATFORM {
            Ok(release)
        } else {
            Err(format!("{namespace}.{name} has no {PLATFORM} build"))
        }
    }

    fn release_at(&self, url: &str, namespace: &str, name: &str) -> Result<Release, String> {
        let raw: RawExtension = self.json(url)?;
        let id = format!("{namespace}.{name}");
        if !raw.namespace.eq_ignore_ascii_case(namespace) || !raw.name.eq_ignore_ascii_case(name) {
            return Err(format!(
                "{url}: the registry answered for {}.{}, not {id}",
                raw.namespace, raw.name
            ));
        }
        if !is_version(&raw.version) {
            return Err(format!("{id}: invalid version {:?}", raw.version));
        }
        let mut files = raw.files;
        let download = files
            .remove("download")
            .ok_or_else(|| format!("{id} {} has no download", raw.version))?;
        let sha256 = files.remove("sha256").ok_or_else(|| {
            format!(
                "{id} {}: Open VSX published no SHA-256, so the download cannot be checked",
                raw.version
            )
        })?;
        Ok(Release {
            namespace: raw.namespace,
            name: raw.name,
            version: raw.version,
            target_platform: raw.target_platform.unwrap_or_else(|| UNIVERSAL.to_owned()),
            namespace_display_name: raw.namespace_display_name,
            is_prerelease: raw.pre_release,
            download,
            sha256,
        })
    }

    /// Downloads `release` into `out` and checks it against the SHA-256 that
    /// the registry publishes. Returns the size in bytes.
    pub(crate) fn download(&self, release: &Release, out: &mut impl Write) -> Result<u64, String> {
        let expected = self.text(&release.sha256)?;
        let expected = expected
            .split_whitespace()
            .next()
            .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| format!("{}: not a SHA-256", release.sha256))?
            .to_ascii_lowercase();
        let mut response = self.get(&release.download, "application/octet-stream")?;
        let mut reader = response.body_mut().with_config().limit(MAX_VSIX).reader();
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        let mut size = 0;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|err| format!("{}: {err}", release.download))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])
                .map_err(|err| err.to_string())?;
            size += read as u64;
        }
        let actual = hex(&hasher.finalize());
        if actual == expected {
            Ok(size)
        } else {
            Err(format!(
                "{}: the download's SHA-256 is {actual}, but Open VSX published {expected}",
                release.download
            ))
        }
    }

    fn text(&self, url: &str) -> Result<String, String> {
        let mut response = self.get(url, "text/plain")?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_JSON)
            .read_to_string()
            .map_err(|err| format!("{url}: {err}"))
    }

    fn json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, String> {
        let text = {
            let mut response = self.get(url, "application/json")?;
            response
                .body_mut()
                .with_config()
                .limit(MAX_JSON)
                .read_to_string()
                .map_err(|err| format!("{url}: {err}"))?
        };
        serde_json::from_str(&text).map_err(|err| format!("{url}: unexpected answer: {err}"))
    }

    /// GET with redirects followed by hand, each target checked first.
    fn get(&self, url: &str, accept: &str) -> Result<Response<ureq::Body>, String> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            check_url(&url).map_err(|problem| problem.to_string())?;
            let response = self
                .agent(&url)?
                .get(&url)
                .header("Accept", accept)
                .call()
                .map_err(|err| format!("{url}: {err}"))?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| format!("{url}: redirect without a location"))?;
                url = resolve(&url, location);
                continue;
            }
            if !status.is_success() {
                return Err(format!("{url}: HTTP {}", status.as_u16()));
            }
            return Ok(response);
        }
        Err(format!("{url}: too many redirects"))
    }

    /// The agent for `url`: TLS for `https`, plain for `http` on loopback.
    fn agent(&self, url: &str) -> Result<&Agent, String> {
        if url.starts_with("http://") {
            return Ok(self
                .plain
                .get_or_init(|| config().proxy(None).build().into()));
        }
        self.tls
            .get_or_init(tls_agent)
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// Timeouts, no automatic redirects, statuses read by hand.
fn config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(600)))
        .user_agent(concat!("rustode/", env!("CARGO_PKG_VERSION")))
}

/// An HTTPS agent trusting the system's root certificates.
fn tls_agent() -> Result<Agent, String> {
    let roots = rustls_native_certs::load_native_certs();
    if roots.certs.is_empty() {
        return Err(format!(
            "no root certificates found on this system ({} errors)",
            roots.errors.len()
        ));
    }
    let certs: Vec<Certificate<'static>> = roots
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::Specific(Arc::new(certs)))
        .build();
    Ok(config().tls_config(tls).build().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_targets_are_made_absolute() {
        let base = "https://a.example/api/x/y?q=1";
        assert_eq!(resolve(base, "https://b.example/f"), "https://b.example/f");
        assert_eq!(resolve(base, "//c.example/f"), "https://c.example/f");
        assert_eq!(resolve(base, "/f/g"), "https://a.example/f/g");
        assert_eq!(resolve(base, "z"), "https://a.example/api/x/z");
        assert_eq!(resolve("https://a.example", "z"), "https://a.example/z");
    }

    #[test]
    fn query_text_is_percent_encoded() {
        assert_eq!(encode("rust analyzer"), "rust%20analyzer");
        assert_eq!(encode("a&b=c/é"), "a%26b%3Dc%2F%C3%A9");
        assert_eq!(encode("A-z_0.9~"), "A-z_0.9~");
    }

    #[test]
    fn versions_and_ids_are_checked() {
        assert!(is_version("1.2.3") && is_version("0.10.0-next.1+build.5"));
        assert!(!is_version("1.2") && !is_version("1.2.3/..") && !is_version("../1.2.3"));
        assert!(is_id_part("rust-lang") && is_id_part("0x"));
        assert!(!is_id_part("-x") && !is_id_part("") && !is_id_part("a.b") && !is_id_part(".."));
    }

    #[test]
    fn https_and_loopback_http_are_fetched() {
        assert_eq!(check_url("https://open-vsx.org"), Ok(()));
        assert_eq!(check_url("http://127.0.0.1:8080/x"), Ok(()));
        assert_eq!(check_url("http://localhost/x"), Ok(()));
        assert_eq!(check_url("http://[::1]:9/x"), Ok(()));
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
    }

    #[test]
    fn other_urls_and_the_marketplace_are_not() {
        assert!(matches!(
            check_url("http://example.org"),
            Err(UrlProblem::Invalid(_))
        ));
        assert!(matches!(
            check_url("ftp://example.org"),
            Err(UrlProblem::Invalid(_))
        ));
        assert!(matches!(
            check_url("not a url"),
            Err(UrlProblem::Invalid(_))
        ));
        assert!(matches!(
            check_url("/relative"),
            Err(UrlProblem::Invalid(_))
        ));
        assert!(matches!(
            check_url("https://Publisher.Gallery.VSAssets.IO./x"),
            Err(UrlProblem::Marketplace(host)) if host == "publisher.gallery.vsassets.io."
        ));
        assert!(is_marketplace("VisualStudio.com"));
        assert!(!is_marketplace("notvisualstudio.com"));
    }
}
