//! Which pairs of body representations may be offered as alternatives.
//!
//! The trait and the constant checks its implementations evaluate live here;
//! the implementations, one per codec pair, in private submodules: `encoded`
//! for an application's codec beside every body, `pairs` for the rest.

use crate::extract::describe::RequestContent;

mod encoded;
mod pairs;

/// Proves that two request content types can be alternatives.
///
/// Kynos implements this for its non-overlapping body wrappers. It is not a
/// blanket trait: writing `OneOf<Json<A>, Json<B>>` therefore fails to compile
/// instead of making dispatch order observable.
#[diagnostic::on_unimplemented(
    message = "`{Self}` and `{Rhs}` cannot be alternatives",
    label = "overlapping alternatives",
    note = "two alternatives must be distinguishable by content type, so `OneOf` cannot hold \
            two bodies that share one — dispatch order would decide which won, and no \
            description can express that"
)]
pub trait Alternative<Rhs>: RequestContent
where
    Rhs: RequestContent,
{
    /// Fails to evaluate when the two sides are offered the same requests.
    ///
    /// A pair of distinct body types keeps the default. A
    /// [`Binary<M>`](super::binary::Binary) beside another body is the one
    /// pair its types cannot prove distinct, since a marker may name that
    /// body's own media type, so its implementations compare the two; so is a
    /// `JsonAs` beside a `Json`, another `JsonAs` or a `Binary`, and a
    /// [`TextAs`](super::text_as::TextAs) beside a `Text`, another `TextAs`, a
    /// `Binary` or a `JsonAs`, whose `+json` marker may be a `text/*` type,
    /// and an [`Encoded`](super::codec::Encoded) beside any body, since a
    /// codec may name any media type. `OneOf` evaluates this
    /// wherever it is used, so such a pair fails the build that instantiates
    /// it rather than the description of its route.
    ///
    /// A marker parameter does not keep the pair: a request need not repeat
    /// it, so a bare `text/plain` would reach both [`Text`](super::text::Text)
    /// and a marker spelled `text/plain; charset=iso-8859-1`, in either order,
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, text::Text}, describe::RequestContent},
    ///     http::media::MediaType,
    /// };
    ///
    /// struct Latin1;
    /// impl MediaType for Latin1 {
    ///     const MEDIA_TYPE: &'static str = "text/plain; charset=iso-8859-1";
    /// }
    ///
    /// OneOf::<Text, Binary<Latin1>>::media_types();
    /// ```
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, text::Text}, describe::RequestContent},
    ///     http::media::MediaType,
    /// };
    ///
    /// struct Latin1;
    /// impl MediaType for Latin1 {
    ///     const MEDIA_TYPE: &'static str = "text/plain; charset=iso-8859-1";
    /// }
    ///
    /// OneOf::<Binary<Latin1>, Text>::media_types();
    /// ```
    ///
    /// The refusal holds on each path that reaches the pair on its own:
    /// listing its media types,
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, json::Json}, describe::RequestContent},
    ///     http::media,
    /// };
    ///
    /// OneOf::<Json<u64>, Binary<media::Json>>::media_types();
    /// ```
    ///
    /// describing the route, the pair alone or one side nested in a `OneOf`,
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, json::Json}, describe::RequestContent},
    ///     http::media,
    ///     schema::registry::Registry,
    /// };
    ///
    /// OneOf::<Json<u64>, Binary<media::Json>>::request_body(&mut Registry::new());
    /// ```
    ///
    /// ```compile_fail
    /// use kynos::{
    ///     extract::{body::{OneOf, binary::Binary, json::Json, text::Text}, describe::RequestContent},
    ///     http::media,
    ///     schema::registry::Registry,
    /// };
    ///
    /// OneOf::<OneOf<Text, Json<u64>>, Binary<media::Json>>::request_body(&mut Registry::new());
    /// ```
    ///
    /// and extracting the body:
    ///
    /// ```compile_fail
    /// use std::{
    ///     future::Future,
    ///     pin::pin,
    ///     task::{Context, Waker},
    /// };
    ///
    /// use kynos::{
    ///     extract::{FromRequest, body::{OneOf, binary::Binary, json::Json}},
    ///     http::{Request, media},
    /// };
    ///
    /// let (mut parts, body) = Request::new(kynos::http::body::Body::empty()).into_parts();
    /// let extract = <OneOf<Json<u64>, Binary<media::Json>> as FromRequest<()>>::from_request(
    ///     &mut parts,
    ///     body,
    ///     &(),
    /// );
    /// let _ = pin!(extract).poll(&mut Context::from_waker(Waker::noop()));
    /// ```
    const DISTINCT: () = ();
}

/// Refuses, in the constant evaluation calling it, a marker offered a request
/// `body` is offered too.
pub(crate) const fn distinct(marker: &str, body: &str) {
    assert!(
        !shares_requests(marker, body),
        "a `Binary` marker is offered requests the body beside it in `OneOf` is offered too"
    );
}

/// Refuses, in the constant evaluation calling it, a `JsonAs` marker offered a
/// request `body` is offered too: another `JsonAs`'s marker, `Json`'s own
/// media type, or a `TextAs`'s marker.
#[cfg(feature = "json")]
pub(crate) const fn distinct_json(marker: &str, body: &str) {
    assert!(
        !shares_requests(marker, body),
        "a `JsonAs` marker is offered requests the body beside it in `OneOf` is offered too"
    );
}

/// Refuses, in the constant evaluation calling it, a `TextAs` marker offered a
/// request `body` is offered too: another `TextAs`'s marker, `Text`'s own
/// media type, or a `JsonAs`'s marker.
pub(crate) const fn distinct_text(marker: &str, body: &str) {
    assert!(
        !shares_requests(marker, body),
        "a `TextAs` marker is offered requests the body beside it in `OneOf` is offered too"
    );
}

/// Refuses, in the constant evaluation calling it, a [`BodyCodec`]'s media
/// type offered a request `body` is offered too: another codec's, a built-in
/// body's own, or a `JsonAs` or `TextAs` marker.
///
/// [`BodyCodec`]: super::codec::BodyCodec
pub(crate) const fn distinct_codec(codec: &str, body: &str) {
    assert!(
        !shares_requests(codec, body),
        "a `BodyCodec`'s media type is offered requests the body beside it in `OneOf` is offered \
         too"
    );
}

/// Whether [`offers`](super::offers) hands some request to both a marker and
/// `body`, a codec's own parameterless media type.
///
/// True when the two share a type and subtype, ignoring ASCII case and
/// surrounding whitespace, whatever parameters the marker declares: a request
/// need not repeat them, so `body` itself reaches both. A structured suffix is
/// a different subtype.
pub(crate) const fn shares_requests(marker: &str, body: &str) -> bool {
    eq_ignore_ascii_case(
        split_essence(marker.as_bytes()).0,
        split_essence(body.as_bytes()).0,
    )
}

/// A media type's trimmed type and subtype, and the parameters after them.
pub(crate) const fn split_essence(media_type: &[u8]) -> (&[u8], &[u8]) {
    let (essence, parameters) = split_at_byte(media_type, b';');
    (essence.trim_ascii(), parameters)
}

/// The bytes before the first `byte`, and those after it.
pub(crate) const fn split_at_byte(bytes: &[u8], byte: u8) -> (&[u8], &[u8]) {
    let mut end = 0;
    while end < bytes.len() && bytes[end] != byte {
        end += 1;
    }
    let (before, after) = bytes.split_at(end);
    match after {
        [_, rest @ ..] => (before, rest),
        [] => (before, after),
    }
}

/// Byte-wise ASCII case-insensitive equality, usable in a constant.
pub(crate) const fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0;
    while at < left.len() {
        if !left[at].eq_ignore_ascii_case(&right[at]) {
            return false;
        }
        at += 1;
    }
    true
}
