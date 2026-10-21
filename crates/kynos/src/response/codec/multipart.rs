//! Writing `multipart/form-data` as a response.
//!
//! A `MultipartForm<T>` returned from a handler is described by the same
//! schema of `T` it is read under, so it describes the same parts it would
//! accept.
//!
//! The body is rendered to RFC 7578 over RFC 2046. Writing uses
//! [`IntoMultipart`] rather than `Serialize`, under which a [`FilePart`]'s
//! bytes would reach the wire as an array of numbers.

use bytes::Bytes;
use kynos_openapi::model::body::mime_names;

use crate::{
    extract::{
        body::multipart::{FilePart, MultipartForm, Part},
        describe::RequestContent,
    },
    http::{HeaderValue, Response, body::Body, header},
    response::{
        IntoResponse, Responses,
        framing::{self, Boundary, Encapsulation, Sink},
    },
    schema::{Schema, registry::Registry},
};

#[cfg(test)]
use crate::response::framing::{BOUNDARY_PREFIX, contains, unfolded};

/// How a declared type becomes the parts of a `multipart/form-data` body.
///
/// The counterpart of
/// [`FromMultipart`](crate::extract::body::multipart::FromMultipart), yielding
/// the same [`Part`]s it consumes. `#[derive(MultipartForm)]` writes both from
/// one declaration.
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

/// Typeless bytes, RFC 7578's default for a part whose schema states no type.
impl IntoPart for Bytes {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: None,
            content_type: Some(mime_names::APPLICATION_OCTET_STREAM.to_owned()),
            bytes: self,
        }
    }
}

/// UTF-8 text, with the charset stated rather than left to RFC 7578's
/// `text/plain` default.
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

impl<T: IntoMultipart> IntoResponse for MultipartForm<T> {
    fn into_response(self) -> Response {
        let parts = self.0.into_parts();
        let boundary = boundary(&parts);
        let body = render(parts, &boundary);

        let mut content_type = String::with_capacity(CONTENT_TYPE_PREFIX.len() + boundary.len());
        content_type.push_str(CONTENT_TYPE_PREFIX);
        content_type.push_str(&boundary);

        let mut response = Response::new(Body::from_bytes(body));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::try_from(content_type).expect("a generated boundary is printable ASCII"),
        );
        response
    }
}

/// The response's `Content-Type`, up to the delimiter it names.
const CONTENT_TYPE_PREFIX: &str = "multipart/form-data; boundary=";

/// A delimiter no part contains, over the octets these parts carry.
fn boundary(parts: &[Part]) -> Boundary {
    framing::boundary(parts.iter().map(|part| part.bytes.as_ref()))
}

/// The body: this subtype's header block per part, framed by RFC 2046.
fn render(parts: Vec<Part>, boundary: &str) -> Bytes {
    framing::render(parts, boundary)
}

/// The header lines a form-data part declares, written straight into the body.
impl Encapsulation for Part {
    fn headers(&self, sink: &mut impl Sink) {
        sink.put(b"Content-Disposition: form-data; name=\"");
        put_quoted(&self.name, sink);
        sink.put(b"\"");
        if let Some(file_name) = &self.file_name {
            sink.put(b"; filename=\"");
            put_quoted(file_name, sink);
            sink.put(b"\"");
        }
        sink.put(framing::CRLF);

        if let Some(content_type) = &self.content_type {
            sink.put(b"Content-Type: ");
            framing::put_unfolded(content_type, sink);
            sink.put(framing::CRLF);
        }
    }

    fn content(&self) -> &[u8] {
        &self.bytes
    }
}

/// A `Content-Disposition` parameter, as the quoted-string it travels in.
///
/// UTF-8 text is kept (RFC 7578). `"` is escaped; line endings are dropped so a
/// name cannot inject a header; `\` is dropped rather than escaped as `\\`.
/// Kynos's own reader unescapes a `quoted-pair`, but `multer` and readers
/// following the HTML form encoding do not, and to them a value ending in
/// `\\` escapes its closing quote and breaks the header: lossy everywhere
/// beats unreadable to some.
fn put_quoted(value: &str, sink: &mut impl Sink) {
    // Scanned as octets: each is ASCII, which no UTF-8 sequence contains.
    let mut rest = value.as_bytes();
    while let Some(at) = rest
        .iter()
        .position(|octet| matches!(octet, b'"' | b'\\' | b'\r' | b'\n'))
    {
        sink.put(&rest[..at]);
        if rest[at] == b'"' {
            sink.put(b"\\\"");
        }
        rest = &rest[at + 1..];
    }
    sink.put(rest);
}

/// [`put_quoted`]'s output, for the tests and the fuzz target that read it.
#[cfg(any(test, fuzzing))]
pub(crate) fn quoted(value: &str) -> String {
    let mut quoted = Vec::with_capacity(value.len());
    put_quoted(value, &mut quoted);
    String::from_utf8(quoted).expect("dropping or escaping ASCII keeps UTF-8")
}

impl<T: Schema> Responses for MultipartForm<T> {
    // Taken from the extracting half so both directions describe the same parts.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut response = kynos_openapi::Response::new("OK");
        response.content = <Self as RequestContent>::request_body(registry).content;

        kynos_openapi::Responses::new().with(200, response)
    }
}

#[cfg(test)]
mod tests;
