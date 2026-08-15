//! Inputs that consume the request body, each describing itself as an OpenAPI
//! Request Body Object.
//!
//! One module per codec. Adding a codec is a new file, one `pub mod` line here
//! gated on its feature, and its entries in [`alternative`] — rather than a new
//! `#[cfg]` threaded through a shared file at every impl site.
//!
//! [`OneOf`] and `Option<T>` are the two combinators over those codecs, and
//! live here because they are generic over any [`RequestContent`].

pub mod alternative;
pub mod binary;
pub mod text;

#[cfg(test)]
mod tests;

#[cfg(feature = "form")]
pub mod form;
#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "protobuf")]
pub mod protobuf;

use bytes::Bytes;
use http_body_util::{BodyExt, Collected};

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        body::alternative::Alternative,
        describe::{Describe, RequestContent},
    },
    http::{HeaderMap, Request, header},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// The media type a request declares, split from its parameters.
///
/// `None` when the header is absent or is not text a media type can be read out
/// of, which every codec here treats the way it treats an unacceptable one.
fn content_type(headers: &HeaderMap) -> Option<(&str, &str)> {
    let value = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    let (media_type, parameters) = value.split_once(';').unwrap_or((value, ""));
    Some((media_type.trim(), parameters))
}

/// Whether the parameters trailing a media type are ones a codec accepts.
///
/// A codec accepts none at all, or `charset=utf-8`: Kynos decodes every text
/// format as UTF-8, so another charset names something it would misread rather
/// than something it can decline to notice. Any other parameter is a media type
/// no [`media_types`](RequestContent::media_types) claims.
fn parameters_are_acceptable(parameters: &str) -> bool {
    parameters
        .split(';')
        .filter(|parameter| !parameter.trim().is_empty())
        .all(|parameter| {
            parameter.split_once('=').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case("charset")
                    && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
            })
        })
}

/// Whether the request offers exactly `media_type`.
///
/// The comparison is on the media type itself, never on a structured suffix: an
/// operation accepts what its description claims, and `application/vnd.x+json`
/// is not `application/json`.
fn offers(headers: &HeaderMap, media_type: &str) -> bool {
    content_type(headers).is_some_and(|(offered, parameters)| {
        offered.eq_ignore_ascii_case(media_type) && parameters_are_acceptable(parameters)
    })
}

/// Whether the request offers any of `media_types`.
fn offers_any(headers: &HeaderMap, media_types: &[&str]) -> bool {
    media_types
        .iter()
        .any(|media_type| offers(headers, media_type))
}

/// The 415 a body extractor raises, quoting what the client offered.
fn unsupported_media_type(headers: &HeaderMap) -> BodyRejection {
    BodyRejection::UnsupportedMediaType {
        received: headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    }
}

/// Enforces `media_type`, then reads the whole body into memory.
///
/// This is the first half of every codec in this module. Enforcing the content
/// type first is what keeps an operation from accepting one its description
/// never claimed, and a transport failure part-way through is a 400: what
/// arrived is not the body the client meant to send, and no codec can be asked
/// about it.
async fn read_body(request: Request, media_type: &str) -> Result<Bytes, BodyRejection> {
    if !offers(request.headers(), media_type) {
        return Err(unsupported_media_type(request.headers()));
    }

    request
        .into_body()
        .collect()
        .await
        .map(Collected::to_bytes)
        .map_err(|error| BodyRejection::Syntax {
            detail: error.to_string(),
        })
}

/// One of two request body representations, selected by `Content-Type`.
///
/// The alternatives must implement [`Alternative`], which is provided only
/// for pairs whose media types are known to be distinct. Unsupported media
/// types reject with 415; a malformed selected representation uses that
/// representation's normal rejection.
///
/// ```no_run
/// use kynos::extract::{
///     FromRequest,
///     body::{OneOf, binary::Binary, text::Text},
///     media::Pdf,
/// };
///
/// async fn upload(body: OneOf<Text, Binary<Pdf>>) {
///     match body {
///         OneOf::Left(text) => drop(text),
///         OneOf::Right(pdf) => drop(pdf),
///     }
/// }
///
/// // Naming the signature is not enough to prove the pair is offerable: the
/// // `Alternative` bound lives on the implementations rather than on the type,
/// // so an unproven pair still typechecks until something asks for one.
/// fn takes_a_body<C, T: FromRequest<C>>() {}
/// takes_a_body::<(), OneOf<Text, Binary<Pdf>>>();
/// ```
///
/// Alternatives with the same media type are intentionally not implemented:
///
/// ```compile_fail
/// use kynos::extract::body::{OneOf, text::Text};
///
/// fn body<T: kynos::extract::FromRequest<()>>() {}
/// body::<OneOf<Text, Text>>();
/// ```
///
/// Two [`Binary`](binary::Binary)s are refused for the same reason, even when
/// the markers differ. Both media types come from a marker, so nothing at the
/// implementation site can tell this pair from `Binary<Pdf>` beside itself —
/// unlike the pair above, where one side's media type is fixed by its type:
///
/// ```compile_fail
/// use kynos::extract::{body::{OneOf, binary::Binary}, media::{Pdf, Png}};
///
/// fn body<T: kynos::extract::FromRequest<()>>() {}
/// body::<OneOf<Binary<Pdf>, Binary<Png>>>();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OneOf<L, R> {
    /// The left representation was selected.
    Left(L),
    /// The right representation was selected.
    Right(R),
}

/// An optional body is absent when the request declares no `Content-Type`.
///
/// That is the whole rule, and it is decided from the head alone. A request
/// that names no media type is stating it sent no representation; one that
/// names a media type is answered by `T` exactly as if the `Option` were not
/// there, so an unsupported type is still 415 and an empty JSON body is still
/// 400. Emptiness is deliberately not the test: an empty [`Text`](text::Text)
/// body is the empty string, and `Option` must not swallow it.
impl<C, T> FromRequest<C> for Option<T>
where
    C: Sync,
    T: FromRequest<C> + RequestContent,
{
    type Rejection = T::Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        if request.headers().contains_key(header::CONTENT_TYPE) {
            T::from_request(request, context).await.map(Some)
        } else {
            Ok(None)
        }
    }
}

impl<T: RequestContent> Describe for Option<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = T::request_body(operation.registry()).optional();
        operation.set_request_body(body);
    }
}

impl<C, L, R> FromRequest<C> for OneOf<L, R>
where
    C: Sync,
    L: FromRequest<C, Rejection = BodyRejection> + Alternative<R>,
    R: FromRequest<C, Rejection = BodyRejection> + RequestContent,
{
    type Rejection = BodyRejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        // The alternative is chosen before either side reads a byte, so a
        // malformed representation still fails as that representation rather
        // than falling through to the other one.
        if offers_any(request.headers(), &L::media_types()) {
            L::from_request(request, context).await.map(Self::Left)
        } else if offers_any(request.headers(), &R::media_types()) {
            R::from_request(request, context).await.map(Self::Right)
        } else {
            Err(unsupported_media_type(request.headers()))
        }
    }
}

impl<L, R> Describe for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
{
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<L, R> RequestContent for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
{
    fn media_types() -> Vec<&'static str> {
        let mut media_types = L::media_types();
        media_types.extend(R::media_types());
        media_types
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let mut body = L::request_body(registry);
        for (media_type, content) in R::request_body(registry).content {
            assert!(
                body.content.insert(media_type.clone(), content).is_none(),
                "request body alternative repeats media type `{media_type}`"
            );
        }
        body
    }
}

impl<L, R, N> Alternative<N> for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
    N: RequestContent,
{
}
