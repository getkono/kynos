//! What a byte range can be taken from.

use bytes::Bytes;

use crate::{
    extract::body::binary::Binary,
    http::media::MediaType,
    response::{IntoResponse, Responses},
};

/// What keeps the rangeable set closed.
mod sealed {
    /// The private supertrait.
    pub trait Sealed {}
}

/// A response body a byte range can be taken from.
///
/// Sealed, and implemented for
/// [`Binary<M>`](crate::extract::body::binary::Binary) alone: RFC 9110 section
/// 14.1.2 defines a byte range over octets of a known length.
///
/// Deliberately not implemented for:
///
/// * [`Text`](crate::extract::body::text::Text): a byte range of UTF-8 may
///   split a character, so it is not a `String`.
/// * `Json<T>`: a byte range of a document is not a document, so the declared
///   schema would not describe the 206.
/// * `BinaryStream<S, M>`: it has no complete length (section 14.4) and no
///   random access. Read ranges from a
///   [`ByteSource`](super::source::ByteSource) instead.
///
/// ```
/// use kynos::{
///     extract::body::binary::Binary,
///     http::media::OctetStream,
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
    /// The media type the octets are carried under, for a part as for the
    /// whole (section 15.3.7.1).
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
    /// Clamped rather than panicking: an offset past the end selects fewer
    /// bytes, which section 15.3.7 permits. Nothing is copied.
    #[must_use]
    fn slice(&self, first: u64, last: u64) -> Self {
        self.with_octets(clamped(self.octets(), first, last))
    }
}

/// The octets from `first` to `last` inclusive, clamped to what is there.
///
/// Free, so callers slicing plain `Bytes` share the one clamping.
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
