//! Detecting, decoding and encoding text files without losing bytes.

use std::fmt;

/// A text encoding rustode can read and write losslessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// UTF-8, with or without a byte order mark.
    Utf8,
    /// UTF-16 little endian.
    Utf16Le,
    /// UTF-16 big endian.
    Utf16Be,
    /// Windows-1252, the superset of Latin-1 used when bytes are not valid UTF-8.
    Windows1252,
}

impl Encoding {
    /// The name VS Code uses in `files.encoding` and the status bar.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Utf16Le => "utf16le",
            Self::Utf16Be => "utf16be",
            Self::Windows1252 => "windows1252",
        }
    }

    fn bom(self) -> &'static [u8] {
        match self {
            Self::Utf8 => &[0xEF, 0xBB, 0xBF],
            Self::Utf16Le => &[0xFF, 0xFE],
            Self::Utf16Be => &[0xFE, 0xFF],
            Self::Windows1252 => &[],
        }
    }
}

/// The result of decoding a file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The text, line endings untouched.
    pub text: String,
    /// The detected encoding.
    pub encoding: Encoding,
    /// Whether the file started with a byte order mark.
    pub bom: bool,
}

/// Decodes bytes: a BOM decides first, then valid UTF-8, then Windows-1252,
/// which maps every byte, so decoding never loses data.
#[must_use]
pub fn decode(bytes: &[u8]) -> Decoded {
    for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
        if let Some(rest) = bytes.strip_prefix(encoding.bom())
            && let Some(text) = decode_without_bom(rest, encoding)
        {
            return Decoded {
                text,
                encoding,
                bom: true,
            };
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Decoded {
            text: text.to_owned(),
            encoding: Encoding::Utf8,
            bom: false,
        };
    }
    let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
    Decoded {
        text: text.into_owned(),
        encoding: Encoding::Windows1252,
        bom: false,
    }
}

/// Like [`decode`], but takes the bytes by value so valid UTF-8 (the common
/// case) becomes the text without a copy, halving peak memory on large files.
#[must_use]
pub fn decode_owned(mut bytes: Vec<u8>) -> Decoded {
    let utf8_bom = Encoding::Utf8.bom();
    let bom = bytes.starts_with(utf8_bom);
    let utf16 = !bom
        && (bytes.starts_with(Encoding::Utf16Le.bom())
            || bytes.starts_with(Encoding::Utf16Be.bom()));
    if utf16 {
        return decode(&bytes);
    }
    if bom {
        bytes.drain(..utf8_bom.len());
    }
    match String::from_utf8(bytes) {
        Ok(text) => Decoded {
            text,
            encoding: Encoding::Utf8,
            bom,
        },
        Err(err) => {
            let mut bytes = err.into_bytes();
            if bom {
                bytes.splice(..0, utf8_bom.iter().copied());
            }
            decode(&bytes)
        }
    }
}

fn decode_without_bom(bytes: &[u8], encoding: Encoding) -> Option<String> {
    let codec = match encoding {
        Encoding::Utf8 => encoding_rs::UTF_8,
        Encoding::Utf16Le => encoding_rs::UTF_16LE,
        Encoding::Utf16Be => encoding_rs::UTF_16BE,
        Encoding::Windows1252 => encoding_rs::WINDOWS_1252,
    };
    let (text, had_errors) = codec.decode_without_bom_handling(bytes);
    (!had_errors).then(|| text.into_owned())
}

/// Text that the target encoding cannot represent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError {
    /// The target encoding.
    pub encoding: Encoding,
    /// The first character that cannot be encoded.
    pub character: char,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.encoding {
            Encoding::Windows1252 => "windows-1252",
            other => other.label(),
        };
        write!(f, "{:?} cannot be written in {name}", self.character)
    }
}

impl std::error::Error for EncodeError {}

/// Encodes text, adding the BOM when asked. Refuses lossy conversions.
///
/// # Errors
/// When a character has no representation in the target encoding.
pub fn encode(text: &str, encoding: Encoding, bom: bool) -> Result<Vec<u8>, EncodeError> {
    let mut out = if bom {
        encoding.bom().to_vec()
    } else {
        Vec::new()
    };
    match encoding {
        Encoding::Utf8 => out.extend_from_slice(text.as_bytes()),
        Encoding::Utf16Le => text
            .encode_utf16()
            .for_each(|u| out.extend(u.to_le_bytes())),
        Encoding::Utf16Be => text
            .encode_utf16()
            .for_each(|u| out.extend(u.to_be_bytes())),
        Encoding::Windows1252 => out.extend(encode_windows_1252(text)?),
    }
    Ok(out)
}

fn encode_windows_1252(text: &str) -> Result<Vec<u8>, EncodeError> {
    // encoding_rs only encodes to UTF-8-compatible targets losslessly via its
    // replacement path, so map each character back through the decode table.
    text.chars()
        .map(|character| {
            windows_1252_byte(character).ok_or(EncodeError {
                encoding: Encoding::Windows1252,
                character,
            })
        })
        .collect()
}

fn windows_1252_byte(character: char) -> Option<u8> {
    let mut buf = [0u8; 4];
    let (bytes, _, had_errors) = encoding_rs::WINDOWS_1252.encode(character.encode_utf8(&mut buf));
    match (&bytes[..], had_errors) {
        ([byte], false) => Some(*byte),
        _ => undefined_windows_1252_byte(character),
    }
}

/// The five bytes Windows-1252 leaves undefined decode to the matching C1
/// control characters; encoding maps them back so such files round-trip.
fn undefined_windows_1252_byte(character: char) -> Option<u8> {
    let code = u32::from(character);
    [0x81, 0x8D, 0x8F, 0x90, 0x9D]
        .contains(&code)
        .then(|| u8::try_from(code).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_windows_1252_byte_round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        let decoded = decode(&bytes);
        assert_eq!(decoded.encoding, Encoding::Windows1252);
        assert_eq!(
            encode(&decoded.text, Encoding::Windows1252, false),
            Ok(bytes)
        );
    }

    #[test]
    fn invalid_utf16_after_a_bom_falls_back() {
        // A lone high surrogate after a UTF-16LE BOM is not valid UTF-16.
        let decoded = decode(&[0xFF, 0xFE, 0x00, 0xD8]);
        assert_eq!(decoded.encoding, Encoding::Windows1252);
        assert!(!decoded.bom);
    }

    #[test]
    fn decode_owned_agrees_with_decode() {
        let cases: [&[u8]; 6] = [
            b"plain",
            b"\xEF\xBB\xBFwith bom",
            b"\xEF\xBB\xBF\xFFbad after bom",
            b"caf\xE9",
            &[0xFF, 0xFE, b'a', 0],
            b"",
        ];
        for bytes in cases {
            assert_eq!(decode_owned(bytes.to_vec()), decode(bytes), "{bytes:?}");
        }
    }
}
