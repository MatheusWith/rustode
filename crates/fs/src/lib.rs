//! File I/O, encodings, line endings, file watching and workspace search.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::unimplemented,
        clippy::todo
    )
)]

mod encoding;
mod eol;
mod files;
mod io;
mod search;
mod watch;

pub use encoding::{Decoded, EncodeError, Encoding, decode, decode_owned, encode};
pub use eol::{Eol, EolInfo, detect_eol, normalize_eol};
pub use files::list_files;
pub use io::{DirEntry, list_dir, read_file, write_file};
pub use search::{
    FileMatches, GlobError, IgnoreFiles, OpenText, PREVIEW_BEFORE, PREVIEW_CHARS, Search,
    SearchRequest, SearchUpdate, StartError, TextMatch, TextMatcher, Waker,
};
pub use watch::{FsChange, Watcher};
