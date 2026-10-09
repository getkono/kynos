//! Writing `multipart/form-data` as a response.
//!
//! The declared field names, per-part media types and encodings are preserved
//! in both directions, so a `MultipartForm<T>` returned from a handler
//! describes the same parts it would accept.
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
    response::{IntoResponse, Responses, framing},
    schema::{Schema, registry::Registry},
};

use crate::response::framing::unfolded;
#[cfg(test)]
use crate::response::framing::{BOUNDARY_PREFIX, contains};

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

        let mut response = Response::new(Body::from_bytes(body));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::try_from(format!("multipart/form-data; boundary={boundary}"))
                .expect("a generated boundary is printable ASCII"),
        );
        response
    }
}

/// A delimiter no part contains, over the octets these parts carry.
fn boundary(parts: &[Part]) -> String {
    framing::boundary(parts.iter().map(|part| part.bytes.as_ref()))
}

/// The body: this subtype's header block per part, framed by RFC 2046.
fn render(parts: Vec<Part>, boundary: &str) -> Bytes {
    let encapsulations = parts
        .into_iter()
        .map(|part| (headers(&part), part.bytes))
        .collect();

    framing::render(encapsulations, boundary)
}

/// The header lines one form-data part declares, CRLF-terminated.
fn headers(part: &Part) -> Vec<u8> {
    let mut headers = Vec::with_capacity(128);

    headers.extend_from_slice(b"Content-Disposition: form-data; name=\"");
    headers.extend_from_slice(quoted(&part.name).as_bytes());
    headers.push(b'"');
    if let Some(file_name) = &part.file_name {
        headers.extend_from_slice(b"; filename=\"");
        headers.extend_from_slice(quoted(file_name).as_bytes());
        headers.push(b'"');
    }
    headers.extend_from_slice(framing::CRLF);

    if let Some(content_type) = &part.content_type {
        headers.extend_from_slice(b"Content-Type: ");
        headers.extend_from_slice(unfolded(content_type).as_bytes());
        headers.extend_from_slice(framing::CRLF);
    }

    headers
}

/// A `Content-Disposition` parameter, as the quoted-string it travels in.
///
/// UTF-8 text is kept (RFC 7578). `"` is escaped; line endings are dropped so a
/// name cannot inject a header; `\` is dropped because readers, `multer`
/// included, do not unescape `\\` and a trailing one breaks the header.
fn quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => {
                quoted.push('\\');
                quoted.push('"');
            }
            '\\' | '\r' | '\n' => {}
            _ => quoted.push(character),
        }
    }
    quoted
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
