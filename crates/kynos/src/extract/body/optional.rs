//! `Option<T>` over a request body, and when that body is absent.

use http_body_util::BodyExt;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Parts, body::Body, header},
    router::operation::OperationCx,
};

/// Whether a request without a `Content-Type` carries an octet of content.
///
/// A `Content-Length` other than zero decides it from the head, before any
/// read; an unparsable one is not a declared zero, so it counts. Otherwise the
/// body decides it: content is what remains once the framing is removed (RFC
/// 9110 section 6.4), so a chunked body of no chunks and HTTP/2 DATA frames
/// that are all empty carry none, as a body a buffering interceptor rebuilt
/// whole does not. Reads up to the first octet only, which nothing reads
/// after: the body is no representation `T` is handed.
async fn carries_content(parts: &Parts, mut body: Body) -> Result<bool, BodyRejection> {
    let declared = parts.headers.get_all(header::CONTENT_LENGTH);
    if declared
        .iter()
        .any(|value| super::limit::parse_length(value) != Some(0))
    {
        return Ok(true);
    }

    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| BodyRejection::Syntax {
            detail: error.to_string(),
        })?;
        if frame.data_ref().is_some_and(|data| !data.is_empty()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// An optional body is absent only when the request declares no
/// `Content-Type` and its content carries no octet.
///
/// A request that names a media type is answered by `T` exactly as if the
/// `Option` were not there, so an unsupported type is still 415 and an empty
/// JSON body is still 400. Emptiness alone is not absence: an empty
/// [`Text`](super::text::Text) body that names its type is the empty string.
///
/// Content without a `Content-Type` is a 415 naming no type rather than an
/// absent body, so it is never discarded unread: a recipient may only assume
/// `application/octet-stream` for it (RFC 9110 section 8.3), which is not a
/// representation `T` reads. Empty content, however it is framed, is absent.
/// A body that fails before its first octet is a 400.
impl<C, T> FromRequest<C> for Option<T>
where
    C: Sync,
    T: FromRequest<C> + RequestContent,
    T::Rejection: From<BodyRejection>,
{
    type Rejection = T::Rejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        context: &C,
    ) -> Result<Self, Self::Rejection> {
        if parts.headers.contains_key(header::CONTENT_TYPE) {
            return T::from_request(parts, body, context).await.map(Some);
        }
        if carries_content(parts, body).await? {
            Err(BodyRejection::UnsupportedMediaType { received: None }.into())
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
