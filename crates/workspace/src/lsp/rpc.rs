//! JSON-RPC 2.0 messages framed as LSP frames them over stdio: a
//! `Content-Length` header, a blank line, then that many bytes of JSON
//! (LSP 3.17, "Base Protocol").

use serde_json::Value;
use std::io::{self, BufRead};

/// Messages larger than this are refused rather than allocated.
const MAX_MESSAGE: usize = 256 << 20;

/// `message` with its header, ready to write.
pub(crate) fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    framed.extend_from_slice(body.as_bytes());
    framed
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Reads one message. `Ok(None)` at the end of the stream; an
/// [`io::ErrorKind::InvalidData`] error for a message that is not JSON, after
/// which the next message can still be read.
pub(crate) fn read(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let Some(length) = read_header(input)? else {
        return Ok(None);
    };
    if length > MAX_MESSAGE {
        return Err(io::Error::other(format!(
            "a {length}-byte message is too large"
        )));
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|err| invalid(err.to_string()))
}

/// The `Content-Length` of the next message; `None` at the end of the stream.
fn read_header(input: &mut impl BufRead) -> io::Result<Option<usize>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return match length {
                None => Ok(None),
                Some(_) => Err(io::ErrorKind::UnexpectedEof.into()),
            };
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            if length.is_some() {
                return Ok(length);
            }
            continue;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let parsed = value
                .trim()
                .parse()
                .map_err(|_| invalid(format!("bad header {header:?}")))?;
            length = Some(parsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_round_trip_and_other_headers_are_ignored() {
        let message = json!({"jsonrpc": "2.0", "id": 1, "result": "é"});
        let mut stream = frame(&message);
        stream.extend_from_slice(b"Content-Type: x\r\ncontent-length: 2\r\n\r\n{}");
        let mut input = io::Cursor::new(stream);
        assert_eq!(read(&mut input).expect("first"), Some(message));
        assert_eq!(read(&mut input).expect("second"), Some(json!({})));
        assert_eq!(read(&mut input).expect("end"), None);
    }

    #[test]
    fn bad_messages_are_errors() {
        let mut bad_json = io::Cursor::new(b"Content-Length: 3\r\n\r\nnot".to_vec());
        let err = read(&mut bad_json).expect_err("not JSON");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let mut bad_header = io::Cursor::new(b"Content-Length: x\r\n\r\n".to_vec());
        assert!(read(&mut bad_header).is_err());
        let mut cut = io::Cursor::new(b"Content-Length: 9\r\n".to_vec());
        assert!(read(&mut cut).is_err(), "the stream ends in the header");
        let mut huge = io::Cursor::new(b"Content-Length: 999999999999\r\n\r\n".to_vec());
        assert!(read(&mut huge).is_err());
    }
}
