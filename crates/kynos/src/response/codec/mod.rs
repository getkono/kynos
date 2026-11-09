//! Writing a codec type as a response.
//!
//! The codec types are defined once under
//! [`extract::body`](crate::extract::body); this is their responding half.

// Private where they declare no item; `multipart` declares `IntoMultipart`.
mod binary;
mod encoded;
mod text;

#[cfg(feature = "form")]
mod form;
#[cfg(feature = "json")]
mod json;
#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "protobuf")]
mod protobuf;

#[cfg(test)]
mod tests;

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::IntoResponse,
};

/// The one 200 a body describes: `media` under `media_type`.
pub(crate) fn ok(media_type: &str, media: kynos_openapi::MediaType) -> kynos_openapi::Responses {
    kynos_openapi::Responses::new().with(
        200,
        kynos_openapi::Response::with_content("OK", media_type, media),
    )
}

/// The Media Type Object of raw bytes: the empty Schema Object, since raw
/// bytes are outside what JSON Schema describes and `contentMediaType` would
/// only repeat the content key.
pub(crate) fn unconstrained() -> kynos_openapi::MediaType {
    kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default()))
}

/// `body` under `content_type`.
pub(crate) fn with_content_type(body: Body, content_type: HeaderValue) -> Response {
    let mut response = Response::new(body);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response
}

/// The 500 a body that cannot be encoded answers, its detail saying why.
pub(crate) fn unserializable(detail: &'static str) -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
        .with_detail(detail)
        .into_response()
}

/// Serializes `value` as JSON into a body of exactly its length.
///
/// The body of a [`Json`] or `JsonAs`, a `#[derive(Reply)]` variant and a [`Problem`]
/// goes through here; streamed JSON records, server-sent events and the
/// OpenAPI description serialize on their own. Ungated, since a problem
/// document is written without the `json` feature.
///
/// [`Json`]: crate::extract::body::json::Json
/// [`Problem`]: crate::error::problem::Problem
pub(crate) fn json_bytes<T: serde::Serialize + ?Sized>(
    value: &T,
) -> serde_json::Result<bytes::Bytes> {
    let mut writer = BodyWriter::new();
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.finish())
}

/// How many octets a body is written on the stack before any heap is touched.
///
/// A body that fits is copied once into an allocation of exactly its length,
/// which `Bytes` takes without the shared header an under-filled `Vec` costs:
/// one block where `serde_json::to_vec` spent a 128-octet start, a reallocation
/// per doubling and that header. Sized past kynos-bench's `echo-post` document.
const STACK_OCTETS: usize = 1024;

/// Where a body is serialized: on the stack until it outgrows it, then on the
/// heap, starting at twice the stack so a spilled body skips the small
/// doublings.
///
/// Codec-neutral: [`json_bytes`] writes JSON into it, and an
/// [`Encoded`](crate::extract::body::codec::Encoded) response hands it to its
/// `BodyCodec`, so an application's format pays what JSON does.
///
/// No hint carried between responses: the first response costs what the
/// thousandth does, which is the property the counting harness holds.
struct BodyWriter {
    stack: [u8; STACK_OCTETS],
    /// Octets on the stack; `STACK_OCTETS` once spilled, so the stack's
    /// leftover room is never written again.
    len: usize,
    /// Empty and unallocated until the body outgrows the stack.
    spilled: Vec<u8>,
}

impl BodyWriter {
    fn new() -> Self {
        Self {
            stack: [0; STACK_OCTETS],
            len: 0,
            spilled: Vec::new(),
        }
    }

    /// What was written, as a body.
    ///
    /// Borrowed rather than consumed: taking `self` by value moves the whole
    /// stack buffer, a kibibyte copied for nothing on every response.
    fn finish(&mut self) -> bytes::Bytes {
        if self.spilled.capacity() == 0 {
            bytes::Bytes::copy_from_slice(&self.stack[..self.len])
        } else {
            bytes::Bytes::from(std::mem::take(&mut self.spilled))
        }
    }

    /// Writes `octets` where neither buffer has room for them: the first time,
    /// by moving what the stack holds to the heap and closing the stack; after
    /// that, by growing the heap.
    #[cold]
    #[inline(never)]
    fn grow(&mut self, octets: &[u8]) {
        if self.spilled.capacity() == 0 {
            self.spilled = Vec::with_capacity((self.len + octets.len()).max(2 * STACK_OCTETS));
            self.spilled.extend_from_slice(&self.stack[..self.len]);
            self.len = STACK_OCTETS;
        }
        self.spilled.extend_from_slice(octets);
    }
}

// Inlined, as `Vec<u8>`'s are: `serde_json` writes in pieces of a few octets,
// and an out-of-line call per piece costs more than the piece. The heap's room
// is tested first and in the form `Vec` tests it, so a spilled body's piece
// costs the one comparison `Vec<u8>`'s own writer makes rather than a second
// one ahead of it; an unspilled body's heap has no room, so it falls through.
impl std::io::Write for BodyWriter {
    #[inline]
    fn write(&mut self, octets: &[u8]) -> std::io::Result<usize> {
        self.write_all(octets)?;
        Ok(octets.len())
    }

    #[inline]
    fn write_all(&mut self, octets: &[u8]) -> std::io::Result<()> {
        if self.spilled.capacity() - self.spilled.len() >= octets.len() {
            self.spilled.extend_from_slice(octets);
        } else if let Some(free) = self.stack.get_mut(self.len..self.len + octets.len()) {
            free.copy_from_slice(octets);
            self.len += octets.len();
        } else {
            self.grow(octets);
        }
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
