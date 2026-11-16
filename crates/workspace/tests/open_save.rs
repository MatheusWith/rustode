//! Workspace seam: open a file, read its state, save it back.

use rustode_workspace::{Encoding, Eol, Workspace};
use std::path::PathBuf;

fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-ws-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut out = if big_endian {
        vec![0xFE, 0xFF]
    } else {
        vec![0xFF, 0xFE]
    };
    for unit in text.encode_utf16() {
        out.extend(if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        });
    }
    out
}

/// Opens, checks the detected state and text, saves, and expects identical bytes.
fn round_trip(
    name: &str,
    bytes: &[u8],
    encoding: Encoding,
    bom: bool,
    eol: (Eol, bool),
    text: &str,
) {
    let path = temp_file(name, bytes);
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    let doc = ws.document(id).expect("document");
    assert_eq!(doc.encoding(), encoding, "{name}: encoding");
    assert_eq!(doc.has_bom(), bom, "{name}: bom");
    assert_eq!((doc.eol(), doc.has_mixed_eol()), eol, "{name}: eol");
    assert_eq!(doc.text(), text, "{name}: text");
    std::fs::write(&path, b"overwritten").expect("clobber");
    ws.save(id).expect("save");
    assert_eq!(
        std::fs::read(&path).expect("read back"),
        bytes,
        "{name}: bytes after save"
    );
}

#[test]
fn utf8_with_and_without_bom_round_trips() {
    round_trip(
        "plain.txt",
        "olá\nmundo\n".as_bytes(),
        Encoding::Utf8,
        false,
        (Eol::Lf, false),
        "olá\nmundo\n",
    );
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend("a\r\nb".as_bytes());
    round_trip(
        "bom.txt",
        &bom,
        Encoding::Utf8,
        true,
        (Eol::Crlf, false),
        "a\r\nb",
    );
}

#[test]
fn utf16_little_and_big_endian_round_trip() {
    round_trip(
        "le.txt",
        &utf16("ação\r\n", false),
        Encoding::Utf16Le,
        true,
        (Eol::Crlf, false),
        "ação\r\n",
    );
    round_trip(
        "be.txt",
        &utf16("ação\n", true),
        Encoding::Utf16Be,
        true,
        (Eol::Lf, false),
        "ação\n",
    );
}

#[test]
fn invalid_utf8_falls_back_to_windows_1252_without_loss() {
    // "café" in Latin-1, plus 0x81, which windows-1252 leaves undefined.
    let bytes = [b'c', b'a', b'f', 0xE9, 0x81, b'\n'];
    round_trip(
        "latin1.txt",
        &bytes,
        Encoding::Windows1252,
        false,
        (Eol::Lf, false),
        "caf\u{e9}\u{81}\n",
    );
}

#[test]
fn mixed_line_endings_are_reported_and_kept_byte_for_byte() {
    round_trip(
        "mixed.txt",
        b"a\r\nb\nc\r\nd",
        Encoding::Utf8,
        false,
        (Eol::Crlf, true),
        "a\r\nb\nc\r\nd",
    );
}

#[test]
fn a_file_without_line_breaks_uses_lf() {
    round_trip(
        "one-line.txt",
        b"x",
        Encoding::Utf8,
        false,
        (Eol::Lf, false),
        "x",
    );
    round_trip(
        "empty.txt",
        b"",
        Encoding::Utf8,
        false,
        (Eol::Lf, false),
        "",
    );
}

#[test]
fn changing_encoding_and_eol_rewrites_the_file() {
    let path = temp_file("convert.txt", b"a\nb\r\nc");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    ws.set_eol(id, Eol::Crlf).expect("set eol");
    ws.set_encoding(id, Encoding::Utf16Le, true)
        .expect("set encoding");
    ws.save(id).expect("save");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        utf16("a\r\nb\r\nc", false)
    );
    let doc = ws.document(id).expect("document");
    assert_eq!((doc.eol(), doc.has_mixed_eol()), (Eol::Crlf, false));
    assert_eq!((doc.encoding(), doc.has_bom()), (Encoding::Utf16Le, true));
}

#[test]
fn text_that_the_target_encoding_cannot_represent_is_refused() {
    let path = temp_file("lossy.txt", "日本".as_bytes());
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    let err = ws
        .set_encoding(id, Encoding::Windows1252, false)
        .expect_err("cannot encode");
    assert!(err.to_string().contains("windows-1252"), "{err}");
    assert_eq!(ws.document(id).expect("doc").encoding(), Encoding::Utf8);
}

#[test]
fn opening_a_missing_file_is_an_error_with_the_path() {
    let mut ws = Workspace::new();
    let err = ws
        .open(std::path::Path::new("/nonexistent/rustode/x.txt"))
        .expect_err("missing");
    assert!(
        err.to_string().contains("/nonexistent/rustode/x.txt"),
        "{err}"
    );
}
