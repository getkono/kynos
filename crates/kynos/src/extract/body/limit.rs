//! How much of a request body an extractor will hold in memory.
//!
//! The figure every buffering codec reads under, shared by all of them.

use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Collected, LengthLimitError, Limited};

use crate::{
    error::rejection::BodyRejection,
    http::{
        HeaderMap, Request,
        body::{Body, BoxError},
        header,
    },
};

/// The most bytes a body extractor holds in memory when no limit covering its
/// operation names another: 2 MiB.
///
/// Every extractor that buffers a body — each codec in
/// [`body`](crate::extract::body), and each record of a streamed one — refuses
/// past it with [`BodyRejection::TooLarge`], so every operation reading a body
/// declares a 413. A declared `Content-Length` past it is refused before a byte
/// is read.
///
/// A covering [`BodySize`](crate::middleware::limits::body_size::BodySize), or
/// the `compression` feature's `Decompression`, replaces this figure for that
/// operation, upward or downward. Mount one on a single operation to raise the
/// cap for one large upload; an attribute route reaches the same method through
/// `kynos::routes![upload.intercept(..)]`.
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

/// The limit a covering interceptor set for this request's body, as a request
/// extension. Crate-private so only an interceptor declaring the 413 moves it.
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

/// What [`collect_capped`] read of a body.
pub(crate) enum Capped {
    /// Every byte the body carried.
    Whole(Bytes),
    /// What arrived before the read failed, and that failure.
    FailedPartWay {
        /// The octets read before the failure.
        arrived: Bytes,
        /// The failure itself.
        error: BoxError,
    },
    /// A frame would have taken the running total past the limit.
    Exceeded,
}

/// Reads `body` while the running total stays within `limit`.
///
/// For an interceptor that must decide a 413 before the extractor runs. A
/// failed read comes back as it failed, for the extractor beneath to refuse;
/// swallowing it would hand on a truncated payload.
pub(crate) async fn collect_capped(mut body: Body, limit: u64) -> Capped {
    let mut collected = BytesMut::new();

    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return Capped::FailedPartWay {
                    arrived: collected.freeze(),
                    error,
                };
            }
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };

        let so_far = u64::try_from(collected.len()).unwrap_or(u64::MAX);
        let arriving = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if so_far.saturating_add(arriving) > limit {
            return Capped::Exceeded;
        }

        collected.extend_from_slice(&data);
    }

    Capped::Whole(collected.freeze())
}

/// Reads `body` whole, refusing it on the frame that passes `limit`. A
/// transport failure part-way through is a 400.
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
