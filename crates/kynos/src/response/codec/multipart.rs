//! Writing `multipart/form-data` as a response.
//!
//! The declared field names, per-part media types and encodings are preserved
//! in both directions, so a `MultipartForm<T>` returned from a handler
//! describes the same parts it would accept.
//!
//! `multer` parses; nothing writes. So the body is rendered here, to RFC 7578
//! over RFC 2046: a `Content-Disposition` naming the field and the part's own
//! `Content-Type` when it declared one. The delimiters around them belong to
//! RFC 2046 rather than to this subtype and come from `response::framing`,
//! which the two multipart subtypes share. A plain `Serialize` bound
//! would not do — a [`FilePart`]'s bytes would reach the wire as an array of
//! numbers — so the writing half has a trait of its own, mirroring the reading
//! one.

use bytes::Bytes;

use crate::{
    extract::{
        body::multipart::{FilePart, MultipartForm, Part},
        describe::RequestContent,
    },
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses, framing},
    schema::{Schema, registry::Registry},
};

// The framing is RFC 2046's rather than this subtype's, so the delimiter search
// and the unfolding moved out with it and the byteranges writer under
// `response::range` uses the same ones.
use crate::response::framing::unfolded;
// The search's own vocabulary, which only this file's tests still name.
#[cfg(test)]
use crate::response::framing::{BOUNDARY_PREFIX, contains};

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
///
/// RFC 7578's whole contribution to the framing: which field this part is, the
/// file name if it had one, and the media type if it declared one.
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
/// RFC 7578 carries the field name and the file name as quoted-strings and says
/// to send them as UTF-8, so the text itself is left alone and only what a
/// quoted-string cannot hold is touched.
///
/// Three characters cannot be held. A `"` is escaped, because that is the one
/// escape every reader of this format performs. A line ending would end the
/// header rather than appear in it, so it is dropped: a name that spans two
/// lines is a name that would inject a third party's header.
///
/// A `\` is dropped for the same reason as a line ending — it cannot be
/// represented, only misread. Escaping it as `\\` produces a name readers
/// return with both backslashes, including
/// [`multer`](https://docs.rs/multer), the reader Kynos itself uses on the
/// extracting half; and a name *ending* in one makes the whole header
/// unparseable, since the scan for the closing quote treats the escape as
/// covering it and runs off the end. Percent-encoding it, which is what the
/// HTML form algorithm does for `"`, only moves the problem: nothing on the
/// reading side percent-decodes. Dropping is the one option under which every
/// name Kynos writes is a name Kynos reads back.
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
    // Taken from the extracting half rather than rebuilt, which is what makes
    // "the same parts it would accept" true by construction instead of by
    // agreement between two lists of parts and encodings.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut response = kynos_openapi::Response::new("OK");
        response.content = <Self as RequestContent>::request_body(registry).content;

        kynos_openapi::Responses::new().with(200, response)
    }
}

#[cfg(test)]
mod tests;
