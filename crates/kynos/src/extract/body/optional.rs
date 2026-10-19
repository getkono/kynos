//! `Option<T>` over a request body, and when that body is absent.

use crate::{
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Request, header},
    router::operation::OperationCx,
};

/// An optional body is absent when the request declares no `Content-Type`.
///
/// A request that names a media type is answered by `T` exactly as if the
/// `Option` were not there, so an unsupported type is still 415 and an empty
/// JSON body is still 400. Emptiness is not the test: an empty
/// [`Text`](super::text::Text) body is the empty string.
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
