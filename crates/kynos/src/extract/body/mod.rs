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

#[cfg(feature = "form")]
pub mod form;
#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "multipart")]
pub mod multipart;
#[cfg(feature = "protobuf")]
pub mod protobuf;

use crate::{
    error::Rejection,
    extract::{
        FromRequest,
        body::alternative::Alternative,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::Registry,
};

/// One of two request body representations, selected by `Content-Type`.
///
/// The alternatives must implement [`Alternative`], which is provided only
/// for pairs whose media types are known to be distinct. Unsupported media
/// types reject with 415; a malformed selected representation uses that
/// representation's normal rejection.
///
/// ```no_run
/// use kynos::extract::{
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OneOf<L, R> {
    /// The left representation was selected.
    Left(L),
    /// The right representation was selected.
    Right(R),
}

impl<C, T> FromRequest<C> for Option<T>
where
    C: Sync,
    T: FromRequest<C> + RequestContent,
{
    type Rejection = T::Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
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
    L: FromRequest<C, Rejection = Rejection> + Alternative<R>,
    R: FromRequest<C, Rejection = Rejection> + RequestContent,
{
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
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
