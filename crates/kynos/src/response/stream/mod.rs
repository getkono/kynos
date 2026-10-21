//! Responses delivered as a sequence of chunks or items rather than one value.
//!
//! [`binary`] is in every build: a streamed binary body is described as the
//! whole representation, exactly as a buffered one, which OpenAPI 3.1 already
//! can. `sse` and JSON Lines require `openapi32`, since only 3.2's
//! `itemSchema` describes each item of a sequential media type.
//!
//! Each of these commits its status before the stream ends, so a failure
//! partway through terminates the body; it cannot replace the status already
//! sent with a problem response.

pub mod binary;
#[cfg(feature = "openapi32")]
pub mod sse;

// Private: it only implements for the types `extract::body::json_lines` declares.
#[cfg(all(feature = "json", feature = "openapi32"))]
mod json;

/// The least a fresh block holds: many small records, so a stream of them
/// allocates once per block rather than once per record.
#[cfg(feature = "openapi32")]
const BLOCK: usize = 4 * 1024;

/// The memory a serializing stream writes its records into, each split off as
/// its own frame.
///
/// A frame shares its block, so the next record is written into the room the
/// last one left, and a block is replaced only when that room is shorter than
/// the last record — by one at least as long, so a stream of large records
/// allocates once per record and never grows one from empty. A block is freed
/// once every frame split from it has been written out and dropped.
#[cfg(feature = "openapi32")]
pub(crate) struct RecordBuffer {
    block: bytes::BytesMut,
    last: usize,
}

#[cfg(feature = "openapi32")]
impl RecordBuffer {
    pub(crate) fn new() -> Self {
        Self {
            block: bytes::BytesMut::new(),
            last: 0,
        }
    }

    /// Writes one record with `write` and splits it off as a frame.
    ///
    /// A failed write leaves nothing behind for the next record to inherit.
    pub(crate) fn record<E>(
        &mut self,
        write: impl FnOnce(&mut bytes::BytesMut) -> Result<(), E>,
    ) -> Result<bytes::Bytes, E> {
        if self.block.capacity() <= self.last {
            self.block.reserve(self.last.max(BLOCK));
        }

        match write(&mut self.block) {
            Ok(()) => {
                let record = self.block.split();
                self.last = record.len();
                Ok(record.freeze())
            }
            Err(error) => {
                self.block.clear();
                Err(error)
            }
        }
    }
}
