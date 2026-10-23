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
    Ok(writer.into_bytes())
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
    len: usize,
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

    fn into_bytes(self) -> bytes::Bytes {
        if self.spilled.is_empty() {
            bytes::Bytes::copy_from_slice(&self.stack[..self.len])
        } else {
            bytes::Bytes::from(self.spilled)
        }
    }

    /// Moves what the stack holds to the heap, with room for `more` after it.
    #[cold]
    #[inline(never)]
    fn spill(&mut self, more: usize) {
        self.spilled = Vec::with_capacity((self.len + more).max(2 * STACK_OCTETS));
        self.spilled.extend_from_slice(&self.stack[..self.len]);
    }
}

// Inlined, as `Vec<u8>`'s are: `serde_json` writes in pieces of a few octets,
// and an out-of-line call per piece costs more than the piece.
impl std::io::Write for BodyWriter {
    #[inline]
    fn write(&mut self, octets: &[u8]) -> std::io::Result<usize> {
        self.write_all(octets)?;
        Ok(octets.len())
    }

    #[inline]
    fn write_all(&mut self, octets: &[u8]) -> std::io::Result<()> {
        if self.spilled.is_empty() {
            if let Some(free) = self.stack.get_mut(self.len..self.len + octets.len()) {
                free.copy_from_slice(octets);
                self.len += octets.len();
                return Ok(());
            }
            self.spill(octets.len());
        }
        self.spilled.extend_from_slice(octets);
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
