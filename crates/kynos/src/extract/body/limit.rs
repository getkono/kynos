//! How much of a request body an extractor will hold in memory.
//!
//! Every codec that buffers reads under the same figure, so it lives apart
//! from any one of them: the default, the limit a covering interceptor hands
//! down in its place, and the two ways a body is refused for passing it.

use bytes::Bytes;
use http_body_util::{BodyExt, Collected, LengthLimitError, Limited};

use crate::{
    error::rejection::BodyRejection,
    http::{HeaderMap, Request, body::Body, header},
};

/// The most bytes a body extractor holds in memory when no limit covering its
/// operation names another: 2 MiB.
///
/// Every extractor that buffers a body — each codec in
/// [`body`](crate::extract::body), and each record of a streamed one — refuses
/// past it with [`BodyRejection::TooLarge`], so every operation reading a body
/// declares a 413 whether or not anything was mounted. A body that declares a
/// `Content-Length` past it is refused before a byte is read.
///
/// A [`BodySize`](crate::middleware::limits::body_size::BodySize) covering an
/// operation replaces this figure for that operation, upward as well as
/// downward, and so does the `compression` feature's `Decompression`, whose
/// limit is the route's body limit. Mount one on the operation itself to raise
/// the cap for one large upload without a group of its own. An attribute route
/// reaches the same method through `kynos::routes![upload].0`.
///
/// ```no_run
/// use kynos::{
///     extract::body::binary::Binary, http::media::OctetStream,
///     middleware::limits::body_size::BodySize, openapi,
///     response::status::NoContent, router::endpoint::builder::EndpointBuilder,
/// };
///
/// async fn upload(archive: Binary<OctetStream>) -> NoContent {
///     drop(archive.into_inner());
///     NoContent
/// }
///
/// let endpoint = EndpointBuilder::new(
///     openapi::Method::Post,
///     openapi::PathTemplate::parse("/archives").expect("valid path"),
///     upload,
/// )
/// .intercept(BodySize::new(96 * 1024 * 1024));
/// let router = kynos::Router::<()>::new().mount(endpoint);
/// # let _ = router;
/// ```
pub const DEFAULT_LIMIT: u64 = 2 * 1024 * 1024;

/// The limit a covering interceptor set for this request's body, carried to the
/// extractor beneath it as a request extension.
///
/// Crate-private, so that only an interceptor whose 413 is declared can move
/// the cap: one an application could set would raise it with nothing in the
/// description saying so.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BodyLimit(pub(crate) u64);

/// The limit `request`'s body is read under.
pub(crate) fn of(request: &Request) -> u64 {
    request
        .extensions()
        .get::<BodyLimit>()
        .map_or(DEFAULT_LIMIT, |limit| limit.0)
}

/// The length the request declared, when it declared one.
pub(crate) fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The 413 for a request whose declared length already passes `limit`, decided
/// from the head so that not a byte of the body is read.
pub(crate) fn refuse_declared(headers: &HeaderMap, limit: u64) -> Result<(), BodyRejection> {
    match declared_length(headers) {
        Some(declared) if declared > limit => Err(BodyRejection::TooLarge { limit }),
        _ => Ok(()),
    }
}

/// Reads `body` whole, refusing it on the frame that passes `limit` rather
/// than after the whole body has arrived.
///
/// A transport failure part-way through is a 400: what arrived is not the body
/// the client meant to send, and no codec can be asked about it.
pub(crate) async fn read(body: Body, limit: u64) -> Result<Bytes, BodyRejection> {
    Limited::new(body, usize::try_from(limit).unwrap_or(usize::MAX))
        .collect()
        .await
        .map(Collected::to_bytes)
        .map_err(|error| {
            if error.is::<LengthLimitError>() {
                BodyRejection::TooLarge { limit }
            } else {
                BodyRejection::Syntax {
                    detail: error.to_string(),
                }
            }
        })
}
