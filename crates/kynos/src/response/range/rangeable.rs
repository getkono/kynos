//! What a byte range can be taken from.

use bytes::Bytes;

use crate::{
    extract::{body::binary::Binary, media::MediaType},
    response::{IntoResponse, Responses},
};

/// What keeps the rangeable set closed.
///
/// Implemented by Kynos for the one body shape a byte range is defined over,
/// and unnameable downstream.
mod sealed {
    /// The private supertrait. Deliberately empty.
    pub trait Sealed {}
}

/// A response body a byte range can be taken from.
///
/// Sealed, and implemented for
/// [`Binary<M>`](crate::extract::body::binary::Binary) alone. RFC 9110 section
/// 14.1.2 defines a byte range over *the representation data's octet sequence*,
/// with every offset relative to a complete length the sender is asked to
/// state, so the set is exactly the bodies that are already octets of a known
/// length.
///
/// # Three deliberate absences
///
/// These are claims about what a byte range means, not implementations waiting
/// to be written.
///
/// * [`Text`](crate::extract::body::text::Text) holds a `String`. A byte range
///   of UTF-8 is octets — it may begin and end mid-character — so the value a
///   206 carries is not a `String` and calling it one would be a lie the type
///   system tells.
/// * `Json<T>` is worse. A byte range of a
///   document is not a document, so the `content` schema the 200 declares would
///   describe something the 206 never carries, and the description would be
///   wrong in the one direction `emitted ⊇ observable` does not cover.
/// * `BinaryStream<S, M>` has
///   neither a complete length nor random access. Section 14.4 asks a sender to
///   state the complete length, and section 14.1.2 makes every offset relative
///   to it, so a stream can answer neither half.
///
/// ```
/// use kynos::{
///     extract::{body::binary::Binary, media::OctetStream},
///     response::range::rangeable::Rangeable,
/// };
///
/// let whole = Binary::<OctetStream>::new(&b"0123456789"[..]);
///
/// assert_eq!(whole.complete_length(), 10);
/// assert_eq!(whole.slice(2, 4).octets(), &b"234"[..]);
///
/// // Clamped, not checked: a last offset past the end selects fewer bytes.
/// assert_eq!(whole.slice(8, 99).octets(), &b"89"[..]);
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be served as a byte range",
    label = "not rangeable",
    note = "`Range<T>` and `Ranged<T>` take a body that is already octets of a known length, \
            which is `Binary<M>`",
    note = "`Text` holds a `String` and `Json<T>` a document, and a byte range of either is \
            neither; a stream has no complete length to state and no way to seek within it"
)]
pub trait Rangeable: IntoResponse + Responses + sealed::Sealed + Sized {
    /// The media type the octets are carried under.
    ///
    /// The same string the whole representation states, because a part of a
    /// representation is carried as the media type the whole one is — section
    /// 15.3.7.1's worked example sends `Content-Type: image/gif` on the 206.
    fn media_type() -> &'static str;

    /// The octets this body is.
    fn octets(&self) -> &Bytes;

    /// The same body over different octets.
    #[must_use]
    fn with_octets(&self, octets: Bytes) -> Self;

    /// The `complete-length` a `Content-Range` states.
    fn complete_length(&self) -> u64 {
        u64::try_from(self.octets().len()).unwrap_or(u64::MAX)
    }

    /// The octets from `first` to `last`, inclusive.
    ///
    /// **Clamped, not checked.** `Bytes::slice` panics on an out-of-range
    /// index, and a response path that panics is worse than one that sends
    /// fewer bytes than were asked for — which section 15.3.7 permits outright:
    /// *a server might want to send only a subset of the data requested for
    /// reasons of its own*, and a 206 is self-descriptive, so the client can
    /// still tell what it received.
    ///
    /// Nothing is copied. `Bytes::slice` is refcounted and `O(1)`, so a range of
    /// a large representation allocates no octets at all.
    #[must_use]
    fn slice(&self, first: u64, last: u64) -> Self {
        self.with_octets(clamped(self.octets(), first, last))
    }
}

/// The octets from `first` to `last` inclusive, clamped to what is there.
///
/// Free rather than a method, because `router::assets` slices `Bytes` that are
/// not a [`Rangeable`] body — a file's contents are octets before they are
/// anything — and two clampings would be two chances to be off by one.
pub(crate) fn clamped(octets: &Bytes, first: u64, last: u64) -> Bytes {
    let length = octets.len();
    let start = usize::try_from(first).unwrap_or(length).min(length);
    let end = last
        .checked_add(1)
        .and_then(|end| usize::try_from(end).ok())
        .unwrap_or(length)
        .clamp(start, length);

    octets.slice(start..end)
}

impl<M> sealed::Sealed for Binary<M> {}

impl<M: MediaType> Rangeable for Binary<M> {
    fn media_type() -> &'static str {
        M::MEDIA_TYPE
    }

    fn octets(&self) -> &Bytes {
        &self.bytes
    }

    fn with_octets(&self, octets: Bytes) -> Self {
        Self::new(octets)
    }
}
