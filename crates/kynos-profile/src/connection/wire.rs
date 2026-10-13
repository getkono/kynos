//! A catalog request as the octets a client would send, and the status read
//! back from the octets a server wrote: just enough HTTP/1.1 and HTTP/2 for a
//! connection row, written and read outside the region.
//!
//! Apart from [the rows](super) because the two change for different reasons:
//! a row is added when what is measured changes, and this when what a request
//! looks like on the wire does.

use std::pin::pin;
use std::task::{Context, Poll, Waker};

use bytes::{Bytes, BytesMut};
use http_body::Body as _;
use kynos::http::{Request, StatusCode};

/// HTTP/2's frame types and flags this module writes or reads.
const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const SETTINGS: u8 = 0x4;
const GOAWAY: u8 = 0x7;
const END_STREAM: u8 = 0x1;
const END_HEADERS: u8 = 0x4;

/// The client connection preface, RFC 9113 §3.4.
const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// The request's body octets, which every profiled request has ready.
fn body(request: Request) -> (http::request::Parts, Bytes) {
    let (parts, body) = request.into_parts();
    let mut body = pin!(body);
    let mut octets = BytesMut::new();
    let mut context = Context::from_waker(Waker::noop());
    while let Poll::Ready(Some(frame)) = body.as_mut().poll_frame(&mut context) {
        if let Ok(data) = frame.expect("a profiled request body produces").into_data() {
            octets.extend_from_slice(&data);
        }
    }
    assert!(
        body.is_end_stream(),
        "a profiled request body is ready at once"
    );
    (parts, octets.freeze())
}

/// `request` as HTTP/1.1 octets; with `close`, they close the connection
/// behind it, and without, the connection stays open for another request.
pub(crate) fn http1(request: Request, chunked: bool, close: bool) -> Vec<u8> {
    let (parts, body) = body(request);
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |target| target.as_str());
    let mut octets =
        format!("{} {target} HTTP/1.1\r\nhost: localhost\r\n", parts.method).into_bytes();
    for (name, value) in &parts.headers {
        octets.extend_from_slice(name.as_str().as_bytes());
        octets.extend_from_slice(b": ");
        octets.extend_from_slice(value.as_bytes());
        octets.extend_from_slice(b"\r\n");
    }
    if chunked {
        octets.extend_from_slice(b"transfer-encoding: chunked\r\n");
    } else if !body.is_empty() {
        octets.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    if close {
        octets.extend_from_slice(b"connection: close\r\n");
    }
    octets.extend_from_slice(b"\r\n");
    if chunked {
        if !body.is_empty() {
            octets.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            octets.extend_from_slice(&body);
            octets.extend_from_slice(b"\r\n");
        }
        octets.extend_from_slice(b"0\r\n\r\n");
    } else {
        octets.extend_from_slice(&body);
    }
    octets
}

/// The client preface and an empty `SETTINGS` frame, which is all a client's
/// half of the handshake needs.
pub(crate) fn handshake() -> Vec<u8> {
    let mut octets = PREFACE.to_vec();
    frame(&mut octets, SETTINGS, 0, 0, &[]);
    octets
}

/// `GOAWAY` naming no server-initiated stream, with `NO_ERROR`: the client is
/// done once the streams it opened are answered.
pub(crate) fn go_away() -> Vec<u8> {
    let mut octets = Vec::new();
    frame(&mut octets, GOAWAY, 0, 0, &[0; 8]);
    octets
}

/// `request` as HTTP/2 frames on `stream`: one `HEADERS` frame, and one
/// `DATA` frame where it has a body.
///
/// Every field is a literal never added to the dynamic table, so that the
/// encoding is the same on every connection and the server's decoder does the
/// same work for the first stream and the second.
pub(crate) fn http2(request: Request, stream: u32) -> Vec<u8> {
    let (parts, body) = body(request);
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |target| target.as_str());
    let mut block = Vec::new();
    literal(&mut block, b":method", parts.method.as_str().as_bytes());
    literal(&mut block, b":scheme", b"http");
    literal(&mut block, b":authority", b"localhost");
    literal(&mut block, b":path", target.as_bytes());
    for (name, value) in &parts.headers {
        literal(&mut block, name.as_str().as_bytes(), value.as_bytes());
    }
    if !body.is_empty() {
        literal(
            &mut block,
            b"content-length",
            body.len().to_string().as_bytes(),
        );
    }
    let mut octets = Vec::new();
    if body.is_empty() {
        frame(
            &mut octets,
            HEADERS,
            END_HEADERS | END_STREAM,
            stream,
            &block,
        );
    } else {
        frame(&mut octets, HEADERS, END_HEADERS, stream, &block);
        frame(&mut octets, DATA, END_STREAM, stream, &body);
    }
    octets
}

/// One HPACK literal field without indexing and with a literal name, RFC 7541
/// §6.2.2, its strings not Huffman-coded.
fn literal(block: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    block.push(0);
    for string in [name, value] {
        // Every length here fits the 7-bit prefix whole.
        block.push(
            u8::try_from(string.len())
                .ok()
                .filter(|&length| length < 0x7f)
                .expect("a short field"),
        );
        block.extend_from_slice(string);
    }
}

/// Appends one HTTP/2 frame, RFC 9113 §4.1.
fn frame(octets: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let length = u32::try_from(payload.len()).expect("a frame's payload fits its length");
    octets.extend_from_slice(&length.to_be_bytes()[1..]);
    octets.extend_from_slice(&[kind, flags]);
    octets.extend_from_slice(&stream.to_be_bytes());
    octets.extend_from_slice(payload);
}

/// One HTTP/2 frame read back.
pub(crate) struct Frame<'a> {
    kind: u8,
    flags: u8,
    /// The stream it belongs to; `0` for the connection.
    pub(crate) stream: u32,
    payload: &'a [u8],
}

impl Frame<'_> {
    /// Whether it is the last frame its stream sends.
    pub(crate) fn ends_stream(&self) -> bool {
        matches!(self.kind, HEADERS | DATA) && self.flags & END_STREAM != 0
    }
}

/// Every whole frame in what a server wrote.
pub(crate) fn frames(mut octets: &[u8]) -> impl Iterator<Item = Frame<'_>> {
    std::iter::from_fn(move || {
        let header = octets.get(..9)?;
        let length =
            usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
        let payload = octets.get(9..9 + length)?;
        let frame = Frame {
            kind: header[3],
            flags: header[4],
            stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff,
            payload,
        };
        octets = &octets[9 + length..];
        Some(frame)
    })
}

/// The status line's code in an HTTP/1.1 response.
pub(crate) fn http1_status(octets: &[u8]) -> StatusCode {
    octets
        .strip_prefix(b"HTTP/1.1 ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| StatusCode::from_bytes(code).ok())
        .unwrap_or_else(|| {
            panic!(
                "no HTTP/1.1 status line in {:?}",
                String::from_utf8_lossy(octets)
            )
        })
}

/// Whether `octets` hold a whole HTTP/1.1 response framed by
/// `content-length`, which is how a server frames every body a profiled row
/// answers with.
///
/// # Panics
///
/// When a whole head names no `content-length`.
pub(crate) fn http1_answered(octets: &[u8]) -> bool {
    let Some(end) = octets.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = std::str::from_utf8(&octets[..end]).expect("an ASCII response head");
    let length: usize = head
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().expect("a decimal content-length"))
        })
        .unwrap_or_else(|| panic!("no content-length in {head:?}"));
    octets.len() >= end + 4 + length
}

/// `:status` in the `HEADERS` frame on `stream`, which a server encodes as the
/// first field of the block.
///
/// Only an indexed `:status` from HPACK's static table, RFC 7541 Appendix A,
/// is read: every status a profiled row expects has an entry there.
pub(crate) fn http2_status(octets: &[u8], stream: u32) -> StatusCode {
    let headers = frames(octets)
        .find(|frame| frame.kind == HEADERS && frame.stream == stream)
        .unwrap_or_else(|| panic!("no HEADERS frame on stream {stream}"));
    let code: u16 = match headers.payload.first() {
        Some(0x88) => 200,
        Some(0x89) => 204,
        Some(0x8a) => 206,
        Some(0x8b) => 304,
        Some(0x8c) => 400,
        Some(0x8d) => 404,
        Some(0x8e) => 500,
        other => panic!("`:status` is not an indexed static field: {other:?}"),
    };
    StatusCode::from_u16(code).expect("a valid status")
}
