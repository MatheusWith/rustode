//! `file:` URIs for paths, as LSP names documents: the path's bytes, with
//! every byte other than unreserved characters and `/` percent-encoded
//! (RFC 3986). Linux only, as rustode is in Phase 1.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

/// The URI of an absolute path.
pub(crate) fn from_path(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

/// The path of a `file:` URI; `None` for other schemes.
pub(crate) fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(OsString::from_vec(decoded)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_uris() {
        let path = Path::new("/tmp/a b/é#1.rs");
        let uri = from_path(path);
        assert_eq!(uri, "file:///tmp/a%20b/%C3%A9%231.rs");
        assert_eq!(to_path(&uri).as_deref(), Some(path));
        assert_eq!(
            to_path("file://localhost/x%2").as_deref(),
            Some(Path::new("/x%2"))
        );
        assert_eq!(to_path("untitled:1"), None);
        assert_eq!(to_path("file://host/x"), None);
    }
}
