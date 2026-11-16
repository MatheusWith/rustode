//! Line-ending detection and conversion.

/// A line ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Eol {
    /// `\n`, the default for files without line breaks.
    #[default]
    Lf,
    /// `\r\n`.
    Crlf,
}

impl Eol {
    /// The characters of this line ending.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }
}

/// What a text's line endings look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EolInfo {
    /// The most frequent line ending (CRLF wins ties, LF if there are none).
    pub dominant: Eol,
    /// Whether both kinds occur.
    pub mixed: bool,
}

/// Counts LF and CRLF line endings.
#[must_use]
pub fn detect_eol(text: &str) -> EolInfo {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let dominant = if crlf > 0 && crlf >= lf {
        Eol::Crlf
    } else {
        Eol::Lf
    };
    EolInfo {
        dominant,
        mixed: crlf > 0 && lf > 0,
    }
}

/// Rewrites every line ending as `eol`.
#[must_use]
pub fn normalize_eol(text: &str, eol: Eol) -> String {
    let lf_only = text.replace("\r\n", "\n");
    match eol {
        Eol::Lf => lf_only,
        Eol::Crlf => lf_only.replace('\n', "\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_and_normalization() {
        assert_eq!(
            detect_eol(""),
            EolInfo {
                dominant: Eol::Lf,
                mixed: false
            }
        );
        assert_eq!(
            detect_eol("a\r\nb\n"),
            EolInfo {
                dominant: Eol::Crlf,
                mixed: true
            }
        );
        assert_eq!(
            detect_eol("a\nb\nc\r\n"),
            EolInfo {
                dominant: Eol::Lf,
                mixed: true
            }
        );
        assert_eq!(normalize_eol("a\r\nb\nc", Eol::Crlf), "a\r\nb\r\nc");
        assert_eq!(normalize_eol("a\r\nb\nc", Eol::Lf), "a\nb\nc");
    }
}
