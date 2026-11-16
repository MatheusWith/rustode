//! File I/O, encodings, line endings, file watching and workspace search.

mod encoding;
mod eol;
mod io;

pub use encoding::{Decoded, EncodeError, Encoding, decode, encode};
pub use eol::{Eol, EolInfo, detect_eol, normalize_eol};
pub use io::{read_file, write_file};
