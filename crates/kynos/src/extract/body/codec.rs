//! A body in a format the application brings.
//!
//! [`BodyCodec`] is the one point the codec set opens at: an application
//! implements it for a unit struct naming a format Kynos does not ship, and
//! [`Encoded`] carries a value in that format as a request or response body,
//! beside the built-in codecs in a [`OneOf`](super::OneOf) or a
//! [`Negotiated`](crate::response::negotiate::Negotiated) offer. The codec
//! turns bytes into a value and back; every check around that stays Kynos's.

use std::{fmt, marker::PhantomData};

use bytes::Bytes;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Parts, body::Body, media::is_media_type},
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// A format a body of `T` is decoded from and encoded into.
///
/// Implemented by a unit struct the application declares, one per format and
/// ideally one per family of `T`, and used through [`Encoded<C, T>`]. The
/// implementor writes only the two conversions. Kynos keeps everything around
/// them, so no codec can skip it: the 415 for another `Content-Type`, the body
/// limit, the refusal of a body still under a content coding, the 400/422
/// split [`DecodeError`] states, the bounds `T`'s schema declares, the 500 for
/// a value that cannot be encoded, and the description.
///
/// # Trust model
///
/// The description states `T`'s [`Schema`] under [`MEDIA_TYPE`](Self::MEDIA_TYPE),
/// and OpenAPI 3.2 reads a non-JSON form through the JSON Schema data model
/// (§Parsing and Serializing). That the schema describes the data form this
/// codec produces and accepts is the implementor's assertion: Kynos cannot
/// check it for a format it did not write. A codec whose format cannot carry
/// some value the schema admits, or can carry one the schema does not, refuses
/// it in [`encode`](Self::encode) or [`decode`](Self::decode) rather than let
/// the document say otherwise.
///
/// # Example
///
/// A decimal tally: bytes that are not ASCII digits are not the format, and
/// digits past `u64::MAX` are the format holding a value `u64` cannot.
///
/// ```
/// use std::io::Write;
///
/// use bytes::Bytes;
/// use kynos::extract::body::codec::{BodyCodec, DecodeError, Encoded};
///
/// struct Tally;
///
/// impl BodyCodec<u64> for Tally {
///     const MEDIA_TYPE: &'static str = "application/vnd.example.tally";
///
///     fn decode(bytes: &Bytes) -> Result<u64, DecodeError> {
///         if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
///             return Err(DecodeError::Syntax("a tally is ASCII digits".to_owned()));
///         }
///         // ASCII digits are UTF-8, so only the range can fail.
///         let digits = std::str::from_utf8(bytes).unwrap_or_default();
///         digits
///             .parse()
///             .map_err(|_| DecodeError::Data("the tally exceeds u64".to_owned()))
///     }
///
///     fn encode(value: &u64, out: &mut impl Write) -> std::io::Result<()> {
///         write!(out, "{value}")
///     }
/// }
///
/// async fn increment(tally: Encoded<Tally, u64>) -> Encoded<Tally, u64> {
///     Encoded::new(tally.into_inner() + 1)
/// }
///
/// let written = kynos::response::IntoResponse::into_response(Encoded::<Tally, u64>::new(7));
/// assert_eq!(written.headers()["content-type"], "application/vnd.example.tally");
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a body codec for `{T}`",
    label = "not a `BodyCodec<{T}>`",
    note = "the first parameter of `Encoded<C, T>` is a type implementing `BodyCodec<T>`, which \
            names the media type and converts `T` from and into bytes"
)]
pub trait BodyCodec<T> {
    /// The media type, as it appears in a `Content-Type` header.
    ///
    /// A `type/subtype` of RFC 9110 tokens, optionally followed by
    /// parameters, all in octets a header field value may hold. Anything else
    /// fails the build wherever an [`Encoded`] of this codec is extracted,
    /// written, described or offered.
    ///
    /// A request matches it by type and subtype in any ASCII case: it need not
    /// repeat a parameter declared here, may not contradict one, and may add
    /// `charset=utf-8`. A response carries it verbatim.
    const MEDIA_TYPE: &'static str;

    /// Reads a whole request body, which Kynos has already held to
    /// [`MEDIA_TYPE`](Self::MEDIA_TYPE), the body limit and the absence of a
    /// content coding.
    ///
    /// Handed the `Bytes` the framework read, so a body arriving as one frame
    /// reaches the codec without a copy.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Syntax`] for bytes that are not this format, and
    /// [`DecodeError::Data`] for a well-formed body that does not fit `T`.
    fn decode(bytes: &Bytes) -> Result<T, DecodeError>;

    /// Writes `value` as a response body into `out`.
    ///
    /// `out` is the buffer Kynos responds from, so the codec allocates none of
    /// its own. Nothing is sent until this returns, so an error still changes
    /// the response to a 500.
    ///
    /// # Errors
    ///
    /// Any error, from `out` or from the codec, which Kynos answers with the
    /// 500 problem document a `Json<T>` that cannot be serialized answers.
    fn encode(value: &T, out: &mut impl std::io::Write) -> std::io::Result<()>;
}

/// Why a [`BodyCodec`] could not decode a body: its whole vocabulary for
/// failure.
///
/// The detail reaches the client in the problem document, as a `Json<T>`
/// decoder's does, so it must not echo input the codec should not repeat.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// The bytes are not the format: a 400.
    #[error("{0}")]
    Syntax(String),
    /// The format, holding a value that does not fit `T`: a 422, keyed at the
    /// root pointer, since the codec names no location.
    #[error("{0}")]
    Data(String),
}

/// The 400 or 422 a [`DecodeError`] is, as `Json<T>`'s line between the two
/// draws it.
// By value to fit `map_err`.
#[allow(clippy::needless_pass_by_value)]
fn rejection(error: DecodeError) -> BodyRejection {
    match error {
        DecodeError::Syntax(detail) => BodyRejection::Syntax { detail },
        DecodeError::Data(detail) => BodyRejection::schema_at(String::new(), detail),
    }
}

/// A request or response body of `T`, in the format the codec `C` names.
///
/// What [`Json<T>`](super::json::Json) is for `application/json`, this is for
/// any format a [`BodyCodec<T>`] implements: a request is read, decoded and
/// checked against `T`'s bounds, a response encoded under `C::MEDIA_TYPE`, and
/// both are described as `T`'s schema keyed by `C::MEDIA_TYPE`. Extracting one
/// requires `T: Schema`, since the bounds a derived field declares are checked
/// once the codec has decoded it.
///
/// A request with a missing or different `Content-Type`, a body past the
/// limit, or one still under a content coding is refused before the codec
/// sees a byte. A [`DecodeError::Syntax`] rejects with 400, a
/// [`DecodeError::Data`] with 422 at the root pointer, and a broken bound with
/// 422 at the pointer of each member that broke one.
///
/// A codec's media type must be one: a `type/subtype` of tokens, in octets a
/// header may hold.
///
/// ```compile_fail
/// use kynos::extract::{body::codec::{BodyCodec, DecodeError, Encoded}, describe::RequestContent};
///
/// struct Tally;
/// impl BodyCodec<u64> for Tally {
///     const MEDIA_TYPE: &'static str = "tally";
///     fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> { Ok(0) }
///     fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> { Ok(()) }
/// }
///
/// Encoded::<Tally, u64>::media_types();
/// ```
///
/// Beside another body in a [`OneOf`](super::OneOf), the two must be offered
/// different requests, so a codec is refused beside itself, beside another
/// codec of its type and subtype, and beside any built-in body or `Binary`
/// naming its type and subtype:
///
/// ```compile_fail
/// use kynos::extract::{body::{OneOf, codec::{BodyCodec, DecodeError, Encoded}}, describe::RequestContent};
///
/// struct Tally;
/// impl BodyCodec<u64> for Tally {
///     const MEDIA_TYPE: &'static str = "application/vnd.example.tally";
///     fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> { Ok(0) }
///     fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> { Ok(()) }
/// }
///
/// OneOf::<Encoded<Tally, u64>, Encoded<Tally, u64>>::media_types();
/// ```
///
/// The same holds for a response offer, which `Accept` tells apart by type and
/// subtype alone, so a codec naming `text/plain` is refused beside `Text`:
///
/// ```compile_fail
/// use kynos::{
///     extract::body::{codec::{BodyCodec, DecodeError, Encoded}, text::Text},
///     response::negotiate::representation::Representations,
/// };
///
/// struct Plain;
/// impl BodyCodec<u64> for Plain {
///     const MEDIA_TYPE: &'static str = "text/plain";
///     fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> { Ok(0) }
///     fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> { Ok(()) }
/// }
///
/// <(Text, Encoded<Plain, u64>)>::media_types();
/// ```
///
/// Distinct types are alternatives, in a request and in a response offer:
///
/// ```
/// use kynos::{
///     extract::{
///         body::{OneOf, codec::{BodyCodec, DecodeError, Encoded}, text::Text},
///         describe::RequestContent,
///     },
///     response::negotiate::representation::Representations,
/// };
///
/// struct Tally;
/// impl BodyCodec<u64> for Tally {
///     const MEDIA_TYPE: &'static str = "application/vnd.example.tally";
///     fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> { Ok(0) }
///     fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> { Ok(()) }
/// }
///
/// let offered = OneOf::<Encoded<Tally, u64>, Text>::media_types();
/// assert_eq!(offered, ["application/vnd.example.tally", "text/plain"]);
///
/// let offered = <(Text, Encoded<Tally, u64>)>::media_types();
/// assert_eq!(offered.len(), 2);
/// ```
pub struct Encoded<C, T> {
    /// The decoded value.
    pub value: T,
    // A function pointer, so the codec, never held, bounds no auto trait.
    codec: PhantomData<fn() -> C>,
}

impl<C, T> Encoded<C, T> {
    /// Wraps a value with its compile-time codec.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value,
            codec: PhantomData,
        }
    }

    /// Takes the value out.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

// By hand, so each is bounded on `T` alone: a codec is a marker, and a derive
// would ask it to be `Clone` or `Debug` too.
impl<C, T: Clone> Clone for Encoded<C, T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<C, T: fmt::Debug> fmt::Debug for Encoded<C, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Encoded")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<C, T: Default> Default for Encoded<C, T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<C, T: PartialEq> PartialEq for Encoded<C, T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<C, T: Eq> Eq for Encoded<C, T> {}

impl<C: BodyCodec<T>, T> Encoded<C, T> {
    /// Refuses, in the constant evaluation reading it, a codec whose
    /// `MEDIA_TYPE` is not a media type a header can carry.
    pub(crate) const WELL_FORMED: () = assert!(
        is_media_type(C::MEDIA_TYPE),
        "a `BodyCodec`'s `MEDIA_TYPE` is not a `type/subtype` of tokens in octets a header \
         may hold, and a request names its body by one"
    );
}

/// `T: Schema` because the bounds a derived field declares are checked once
/// the codec has decoded `T`, as for `Json<T>`.
// The context is `C`, as on every other body, so rustc lists the implementors
// of `FromRequest<C>` in one form; the codec is `K` here alone.
impl<C, K, T> FromRequest<C> for Encoded<K, T>
where
    C: Sync,
    K: BodyCodec<T>,
    T: Schema + Send,
{
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let () = Self::WELL_FORMED;

        let bytes = super::read_body(parts, body, K::MEDIA_TYPE).await?;
        let value = K::decode(&bytes).map_err(rejection)?;
        super::checked(value, Pointer::root()).map(Self::new)
    }
}

impl<C: BodyCodec<T>, T: Schema> Describe for Encoded<C, T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
    }
}

impl<C: BodyCodec<T>, T: Schema> RequestContent for Encoded<C, T> {
    fn media_types() -> Vec<&'static str> {
        let () = Self::WELL_FORMED;
        vec![C::MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let () = Self::WELL_FORMED;
        kynos_openapi::RequestBody::new(
            C::MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
