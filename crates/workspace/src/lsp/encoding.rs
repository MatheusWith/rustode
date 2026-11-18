//! LSP positions count a line's code units in the negotiated position
//! encoding: UTF-16 unless the server picks another one from the client's
//! `general.positionEncodings` (LSP 3.17, `PositionEncodingKind`). Documents
//! count chars, so columns are converted both ways, line by line.

/// The encodings rustode offers, most preferred first: UTF-32 counts chars,
/// as the buffer does; UTF-16 is the one every server supports.
pub(crate) const OFFERED: [&str; 3] = ["utf-32", "utf-8", "utf-16"];

/// A position encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PositionEncoding {
    /// Bytes of UTF-8.
    Utf8,
    /// UTF-16 code units: the LSP default.
    #[default]
    Utf16,
    /// Chars.
    Utf32,
}

impl PositionEncoding {
    /// The encoding a server named in `positionEncoding`; UTF-16 when it
    /// named none or one rustode did not offer.
    pub(crate) fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("utf-8") => Self::Utf8,
            Some("utf-32") => Self::Utf32,
            _ => Self::Utf16,
        }
    }

    fn units(self, c: char) -> u64 {
        let units = match self {
            Self::Utf8 => c.len_utf8(),
            Self::Utf16 => c.len_utf16(),
            Self::Utf32 => 1,
        };
        u64::try_from(units).unwrap_or(u64::MAX)
    }

    /// The char column `units` code units into `line`. A position inside a
    /// char goes to that char's start; past the end, to the end (as LSP asks:
    /// "it defaults back to the line length").
    pub(crate) fn column(self, line: &str, units: u64) -> usize {
        let mut used = 0;
        for (column, c) in line.chars().enumerate() {
            used += self.units(c);
            if used > units {
                return column;
            }
        }
        line.chars().count()
    }

    /// The code units before char column `column` of `line`.
    pub(crate) fn units_before(self, line: &str, column: usize) -> u64 {
        line.chars().take(column).map(|c| self.units(c)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_convert_both_ways_in_every_encoding() {
        let line = "aé😀b";
        for (encoding, after_emoji) in [
            (PositionEncoding::Utf8, 7),
            (PositionEncoding::Utf16, 4),
            (PositionEncoding::Utf32, 3),
        ] {
            assert_eq!(encoding.units_before(line, 3), after_emoji, "{encoding:?}");
            assert_eq!(encoding.column(line, after_emoji), 3, "{encoding:?}");
            assert_eq!(encoding.column(line, 99), 4, "past the end: {encoding:?}");
        }
        assert_eq!(
            PositionEncoding::Utf16.column(line, 3),
            2,
            "inside a surrogate pair"
        );
        assert_eq!(PositionEncoding::from_name(None), PositionEncoding::Utf16);
        assert_eq!(
            PositionEncoding::from_name(Some("utf-8")),
            PositionEncoding::Utf8
        );
    }
}
