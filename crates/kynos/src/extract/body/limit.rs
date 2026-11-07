//! How much of a request body an extractor will hold in memory.
//!
//! The figure every buffering codec reads under, shared by all of them.

use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;

use crate::{
    error::rejection::BodyRejection,
    http::{
        HeaderMap, HeaderValue, Parts,
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

/// The limit the body of the request `parts` heads is read under.
pub(crate) fn of(parts: &Parts) -> u64 {
    parts
        .extensions
        .get::<BodyLimit>()
        .map_or(DEFAULT_LIMIT, |limit| limit.0)
}

/// The length the request declared, when it declared one.
///
/// The first `Content-Length` line, read past surrounding whitespace as an
/// unsigned decimal; `None` when there is none or it is not one, which leaves
/// the limit to the read. A buffering interceptor refuses a declared length
/// past its limit from this before reading a byte.
///
/// ```
/// use kynos::{
///     extract::body::limit::declared_length,
///     http::{HeaderMap, HeaderValue, header},
/// };
///
/// let mut headers = HeaderMap::new();
/// assert_eq!(declared_length(&headers), None);
///
/// headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("1024"));
/// assert_eq!(declared_length(&headers), Some(1024));
/// ```
#[must_use]
pub fn declared_length(headers: &HeaderMap) -> Option<u64> {
    parse_length(headers.get(header::CONTENT_LENGTH)?)
}

/// One `Content-Length` line as a count of octets, surrounding whitespace
/// aside; `None` when it is not one.
pub(crate) fn parse_length(value: &HeaderValue) -> Option<u64> {
    value.to_str().ok()?.trim().parse().ok()
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
///
/// [`into_body`](Self::into_body) is the body to hand on to the extractor
/// beneath, whichever way the read ended short of the limit.
#[derive(Debug)]
pub enum Capped {
    /// Every byte the body carried.
    Whole(Bytes),
    /// What arrived before the read failed, and that failure.
    FailedPartWay {
        /// The octets read before the failure.
        arrived: Bytes,
        /// The failure itself.
        error: BoxError,
    },
    /// A frame would have taken the running total past `limit`.
    Exceeded {
        /// The limit the body was read under.
        limit: u64,
    },
}

/// Reads `body` while the running total stays within `limit`.
///
/// For an interceptor that must decide a 413 before the extractor runs. A
/// failed read comes back as it failed, for the extractor beneath to refuse;
/// swallowing it would hand on a truncated payload.
///
/// A body arriving as one data frame comes back as that frame's `Bytes`,
/// neither copied nor queued; only a second non-empty frame starts a buffer
/// joining them. Frames that are not data, and empty ones, are skipped.
///
/// A body whose [`declared_length`] already passes `limit` need not be read at
/// all to be refused. The limit is the interceptor's own: the one an extractor
/// beneath reads under is moved only by an interceptor Kynos ships that
/// declares the 413.
///
/// ```
/// use bytes::Bytes;
/// use kynos::{
///     extract::body::limit::{Capped, collect_capped},
///     http::body::Body,
/// };
///
/// /// What a buffering interceptor hands on, or `None` for its 413.
/// async fn buffered(body: Body, limit: u64) -> Option<Body> {
///     let capped = collect_capped(body, limit).await;
///     if let Capped::Whole(bytes) = &capped {
///         // Inspect the whole payload: verify a signature, log it.
///         let _ = bytes.len();
///     }
///     capped.into_body()
/// }
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let payload = || Body::from_bytes(Bytes::from_static(b"payload"));
/// assert!(buffered(payload(), 64).await.is_some());
/// assert!(buffered(payload(), 4).await.is_none());
/// # }
/// ```
pub async fn collect_capped(mut body: Body, limit: u64) -> Capped {
    let mut first = Bytes::new();
    let mut joined: Option<BytesMut> = None;
    let mut so_far = 0_u64;
    let arrived = |first: Bytes, joined: Option<BytesMut>| joined.map_or(first, BytesMut::freeze);

    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return Capped::FailedPartWay {
                    arrived: arrived(first, joined),
                    error,
                };
            }
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if data.is_empty() {
            continue;
        }

        so_far = so_far.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        if so_far > limit {
            return Capped::Exceeded { limit };
        }

        match &mut joined {
            Some(buffer) => buffer.extend_from_slice(&data),
            None if first.is_empty() => first = data,
            None => {
                let first = std::mem::take(&mut first);
                let mut buffer = BytesMut::with_capacity(first.len() + data.len());
                buffer.extend_from_slice(&first);
                buffer.extend_from_slice(&data);
                joined = Some(buffer);
            }
        }
    }

    Capped::Whole(arrived(first, joined))
}

impl Capped {
    /// The body to hand on: the bytes as a complete body when the read was
    /// whole, or those that arrived followed by the failure when it failed
    /// part-way, through [`Body::failed_after`], so the extractor beneath
    /// refuses a truncated payload rather than parsing it. `None` past the
    /// limit, which the interceptor answers itself.
    #[must_use]
    pub fn into_body(self) -> Option<Body> {
        match self {
            Self::Whole(bytes) => Some(Body::from_bytes(bytes)),
            Self::FailedPartWay { arrived, error } => Some(Body::failed_after(arrived, error)),
            Self::Exceeded { .. } => None,
        }
    }

    /// A body read whole, or the rejection an extractor raises: the 413 past
    /// the limit, and for a transport failure part-way the 408 of a stall or
    /// else a 400.
    ///
    /// Applied to [`collect_capped`]'s result rather than wrapping it in an
    /// `async fn`, whose future would keep a second slot for the body under
    /// every buffering extractor.
    pub(crate) fn into_read(self) -> Result<Bytes, BodyRejection> {
        match self {
            Self::Whole(bytes) => Ok(bytes),
            Self::FailedPartWay { error, .. } => Err(BodyRejection::read_failed(&*error)),
            Self::Exceeded { limit } => Err(BodyRejection::TooLarge { limit }),
        }
    }
}
