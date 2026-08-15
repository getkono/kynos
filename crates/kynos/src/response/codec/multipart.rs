//! Writing `multipart/form-data` as a response.
//!
//! The declared field names, per-part media types and encodings are preserved
//! in both directions, so a `MultipartForm<T>` returned from a handler
//! describes the same parts it would accept.
//!
//! `multer` parses; nothing writes. So the body is rendered here, to RFC 7578
//! over RFC 2046: a delimiter line per part, a `Content-Disposition` naming the
//! field, the part's own `Content-Type` when it declared one, and a closing
//! delimiter. A plain `Serialize` bound would not do — a [`FilePart`]'s bytes
//! would reach the wire as an array of numbers — so the writing half has a
//! trait of its own, mirroring the reading one.

use bytes::Bytes;

use crate::{
    extract::{
        body::multipart::{FilePart, MultipartForm, Part},
        describe::RequestContent,
    },
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

/// How a declared type becomes the parts of a `multipart/form-data` body.
///
/// The counterpart of
/// [`FromMultipart`](crate::extract::body::multipart::FromMultipart), yielding
/// the same [`Part`]s it consumes. `#[derive(MultipartForm)]` writes both from
/// one declaration, which is what makes "the same parts it would accept" true
/// by construction.
///
/// ```
/// use kynos::{
///     extract::body::multipart::{FilePart, Part},
///     response::codec::multipart::IntoMultipart,
/// };
///
/// struct Avatar(FilePart);
///
/// impl IntoMultipart for Avatar {
///     fn into_parts(self) -> Vec<Part> {
///         vec![Part {
///             name: "avatar".to_owned(),
///             file_name: self.0.file_name,
///             content_type: self.0.content_type,
///             bytes: self.0.bytes,
///         }]
///     }
/// }
/// ```
pub trait IntoMultipart {
    /// Renders the value as the parts of a body, in the order they are written.
    fn into_parts(self) -> Vec<Part>;
}

/// How one declared field becomes the part that carries it.
///
/// The counterpart of
/// [`FromPart`](crate::extract::body::multipart::FromPart), implemented for the
/// same three shapes a form field takes. An `Option<T>` field writes nothing
/// when it is absent and a `Vec<T>` field writes one part per element, so an
/// implementation here only ever produces a single part.
pub trait IntoPart {
    /// Renders the field as one part carried under `name`.
    fn into_part(self, name: &str) -> Part;
}

impl IntoPart for FilePart {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: self.file_name,
            content_type: self.content_type,
            bytes: self.bytes,
        }
    }
}

/// Typeless bytes, which is the default RFC 7578 derives for a part whose
/// schema states no type — the same default the describing half relies on.
impl IntoPart for Bytes {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: None,
            content_type: Some("application/octet-stream".to_owned()),
            bytes: self,
        }
    }
}

/// The charset is stated rather than left to RFC 7578's `text/plain` default,
/// because Kynos writes and reads UTF-8 and a recipient guessing otherwise
/// would decode a different string than the one that was sent.
impl IntoPart for String {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: None,
            content_type: Some("text/plain; charset=utf-8".to_owned()),
            bytes: Bytes::from(self.into_bytes()),
        }
    }
}

/// The fixed part of every delimiter Kynos generates.
///
/// Long enough that a body containing it is a body that meant to.
const BOUNDARY_PREFIX: &str = "kynos-boundary-";

/// CRLF, which frames every line of a multipart body. RFC 2046 admits no other
/// line ending here, whatever the parts themselves contain.
const CRLF: &[u8] = b"\r\n";

impl<T: IntoMultipart> IntoResponse for MultipartForm<T> {
    fn into_response(self) -> Response {
        let parts = self.0.into_parts();
        let boundary = boundary(&parts);
        let body = render(parts, &boundary);

        let mut response = Response::new(Body::from_bytes(body));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::try_from(format!("multipart/form-data; boundary={boundary}"))
                .expect("a generated boundary is printable ASCII"),
        );
        response
    }
}

/// A delimiter no part contains.
///
/// RFC 2046 requires exactly that, and Kynos has no source of randomness to
/// make it overwhelmingly likely with — so the delimiter is chosen by looking:
/// a fixed prefix and a counter, raised until nothing encapsulates it. The
/// first candidate wins for every body that was not written to defeat it, so
/// this is one pass over the parts.
fn boundary(parts: &[Part]) -> String {
    let mut counter: u64 = 0;
    loop {
        let candidate = format!("{BOUNDARY_PREFIX}{counter:016x}");
        if !parts
            .iter()
            .any(|part| contains(&part.bytes, candidate.as_bytes()))
        {
            return candidate;
        }
        counter += 1;
    }
}

/// Whether `haystack` encapsulates `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The body: one encapsulation per part, then the closing delimiter.
///
/// No preamble and no epilogue. Both are legal and both are ignored, so writing
/// either would be bytes every recipient discards.
fn render(parts: Vec<Part>, boundary: &str) -> Bytes {
    let capacity = parts
        .iter()
        .map(|part| part.bytes.len() + 128)
        .sum::<usize>()
        + boundary.len();
    let mut body = Vec::with_capacity(capacity);

    for part in parts {
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(CRLF);

        body.extend_from_slice(b"Content-Disposition: form-data; name=\"");
        body.extend_from_slice(quoted(&part.name).as_bytes());
        body.push(b'"');
        if let Some(file_name) = &part.file_name {
            body.extend_from_slice(b"; filename=\"");
            body.extend_from_slice(quoted(file_name).as_bytes());
            body.push(b'"');
        }
        body.extend_from_slice(CRLF);

        if let Some(content_type) = &part.content_type {
            body.extend_from_slice(b"Content-Type: ");
            body.extend_from_slice(unfolded(content_type).as_bytes());
            body.extend_from_slice(CRLF);
        }

        body.extend_from_slice(CRLF);
        body.extend_from_slice(&part.bytes);
        body.extend_from_slice(CRLF);
    }

    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--");
    body.extend_from_slice(CRLF);

    Bytes::from(body)
}

/// A `Content-Disposition` parameter, as the quoted-string it travels in.
///
/// RFC 7578 carries the field name and the file name as quoted-strings and says
/// to send them as UTF-8, so the text itself is left alone and only the two
/// characters a quoted-string cannot hold unescaped are escaped. A line ending
/// inside a header value would end the header rather than appear in it, so it
/// is dropped: a name that spans two lines is a name that would inject a third
/// party's header.
fn quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\r' | '\n' => {}
            _ => quoted.push(character),
        }
    }
    quoted
}

/// A header value with its line endings removed, for the same reason.
fn unfolded(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}

impl<T: Schema> Responses for MultipartForm<T> {
    // Taken from the extracting half rather than rebuilt, which is what makes
    // "the same parts it would accept" true by construction instead of by
    // agreement between two lists of parts and encodings.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut response = kynos_openapi::Response::new("OK");
        response.content = <Self as RequestContent>::request_body(registry).content;

        kynos_openapi::Responses::new().with(200, response)
    }
}
