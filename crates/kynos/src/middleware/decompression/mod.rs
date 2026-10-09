//! Request-body decompression.
//!
//! The other direction from [`compression`](super::compression). A client's
//! request coding is announced in `Content-Encoding` (RFC 9110 section 8.4),
//! not negotiated: the server either decodes it or refuses it.
//!
//! Content coding is out-of-document, but the refusals are declared.

use std::{fmt, io, marker::PhantomData};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZstdDecoder};
use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use kynos_openapi::model::schema::types::SchemaType;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
    extract::body::limit::BodyLimit,
    http::{self, body::Body},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// A content coding this crate can decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Coding {
    Zstd,
    Brotli,
    Gzip,
}

impl Coding {
    /// The coding `token` names, case-insensitively (RFC 9110 section 8.4.1),
    /// accepting the `x-gzip` alias (section 8.4.1.3).
    fn from_token(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("zstd") {
            Some(Self::Zstd)
        } else if token.eq_ignore_ascii_case("br") {
            Some(Self::Brotli)
        } else if token.eq_ignore_ascii_case("gzip") || token.eq_ignore_ascii_case("x-gzip") {
            Some(Self::Gzip)
        } else {
            None
        }
    }
}

/// What this server decodes, most preferred first: the `Accept-Encoding` a 415
/// for an unsupported coding ought to carry (RFC 9110 section 15.5.16).
const ACCEPTED: &str = "zstd, br, gzip";

/// The longest chain of codings that will be decoded; each costs a full pass.
const MAX_CODINGS: usize = 4;

/// The markers a decoding refusal and its interceptor carry, in one phantom.
type Markers<U, M, L> = PhantomData<fn() -> (U, M, L)>;

/// Why [`Decompression`] would not hand a body on.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The body named a content coding this server cannot decode. Produces 415.
    UnsupportedCoding,
    /// The body did not decode as the coding it claimed. Produces 400.
    Malformed,
    /// The decoded body passed the configured bound. Produces 413.
    TooLarge {
        /// The bound it passed, in bytes.
        limit: u64,
    },
}

/// What [`Decompression`] answers with when it will not hand a body on.
///
/// # Three markers, not one
///
/// A 400, a 413 and a 415 are three problems, and a [`ProblemType`] names one,
/// so each refusal takes its own marker, set by its own builder on
/// [`Decompression`].
pub struct Undecodable<U = (), M = (), L = ()> {
    /// Why the body was refused, and with it which status this answers.
    pub reason: Reason,
    /// The three markers; `fn() -> _` keeps a refusal `Send` and `Sync`.
    problem_type: Markers<U, M, L>,
}

impl<U, M, L> Undecodable<U, M, L> {
    /// A refusal for a coding this server cannot decode.
    #[must_use]
    pub fn unsupported_coding() -> Self {
        Self::of(Reason::UnsupportedCoding)
    }

    /// A refusal for a body that did not decode as the coding it declared.
    #[must_use]
    pub fn malformed() -> Self {
        Self::of(Reason::Malformed)
    }

    /// A refusal for a body that decoded past `limit`.
    #[must_use]
    pub fn too_large(limit: u64) -> Self {
        Self::of(Reason::TooLarge { limit })
    }

    /// The one constructor the three above go through.
    fn of(reason: Reason) -> Self {
        Self {
            reason,
            problem_type: PhantomData,
        }
    }
}

impl<U, M, L> IntoResponse for Undecodable<U, M, L>
where
    U: ProblemType,
    M: ProblemType,
    L: ProblemType,
{
    fn into_response(self) -> http::Response {
        match self.reason {
            Reason::UnsupportedCoding => {
                let mut response = refusal_problem::<U>(http::StatusCode::UNSUPPORTED_MEDIA_TYPE)
                    .with_detail(format!(
                        "the request body's content coding is not one this server decodes; \
                         it accepts {ACCEPTED}"
                    ))
                    .into_response();

                // Only on this 415: on a media-type 415 it would misread as a
                // coding complaint (section 15.5.16).
                response.headers_mut().insert(
                    http::header::ACCEPT_ENCODING,
                    http::HeaderValue::from_static(ACCEPTED),
                );

                response
            }
            Reason::Malformed => refusal_problem::<M>(http::StatusCode::BAD_REQUEST)
                .with_detail("the request body did not decode as the coding it declared")
                .into_response(),
            Reason::TooLarge { limit } => refusal_problem::<L>(http::StatusCode::PAYLOAD_TOO_LARGE)
                .with_detail(format!(
                    "the request body exceeds {limit} bytes once decoded"
                ))
                .into_response(),
        }
    }
}

impl<U, M, L> ShortCircuit for Undecodable<U, M, L>
where
    U: ProblemType,
    M: ProblemType,
    L: ProblemType,
{
    const STATUSES: &'static [u16] = &[400, 413, 415];
}

impl<U, M, L> Responses for Undecodable<U, M, L>
where
    U: ProblemType,
    M: ProblemType,
    L: ProblemType,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new()
            .with(
                400,
                refusal_response::<M>(
                    registry,
                    400,
                    "the request body did not decode as the coding it declared",
                ),
            )
            .with(
                413,
                refusal_response::<L>(
                    registry,
                    413,
                    "the request body exceeds the configured limit",
                ),
            )
            .with(
                415,
                refusal_response::<U>(
                    registry,
                    415,
                    "the request body's content coding is not one this server decodes",
                )
                .with_header("Accept-Encoding", accepted_encoding_header()),
            )
    }
}

// Written out: `#[derive]` would bound each on all three markers.

impl<U, M, L> Clone for Undecodable<U, M, L> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U, M, L> Copy for Undecodable<U, M, L> {}

impl<U, M, L> fmt::Debug for Undecodable<U, M, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            reason,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("Undecodable")
            .field("reason", reason)
            .finish()
    }
}

impl<U, M, L> PartialEq for Undecodable<U, M, L> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            reason,
            problem_type: _,
        } = self;

        *reason == other.reason
    }
}

impl<U, M, L> Eq for Undecodable<U, M, L> {}

/// Describes the `Accept-Encoding` that rides on the 415.
fn accepted_encoding_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description("The content codings this server decodes")
}

/// Reads `source` to its end, refusing to produce more than `limit` bytes.
///
/// Checked per chunk, so a bomb is refused while still small.
async fn drain_capped<R: AsyncRead + Unpin>(mut source: R, limit: u64) -> Result<Bytes, Reason> {
    let mut decoded = BytesMut::new();
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let read = std::future::poll_fn(|context| {
            let mut buffer = ReadBuf::new(&mut chunk);
            std::task::ready!(std::pin::Pin::new(&mut source).poll_read(context, &mut buffer))?;
            std::task::Poll::Ready(io::Result::Ok(buffer.filled().len()))
        })
        .await
        .map_err(|_| Reason::Malformed)?;

        if read == 0 {
            return Ok(decoded.freeze());
        }

        let so_far = u64::try_from(decoded.len()).unwrap_or(u64::MAX);
        if so_far.saturating_add(u64::try_from(read).unwrap_or(u64::MAX)) > limit {
            return Err(Reason::TooLarge { limit });
        }

        decoded.extend_from_slice(&chunk[..read]);
    }
}

/// Removes `coding` from `bytes`, producing no more than `limit` bytes.
async fn decode(coding: Coding, bytes: Bytes, limit: u64) -> Result<Bytes, Reason> {
    match coding {
        Coding::Zstd => drain_capped(ZstdDecoder::new(io::Cursor::new(bytes)), limit).await,
        // Boxed: brotli's state is kilobytes, held for the whole exchange.
        Coding::Brotli => {
            Box::pin(drain_capped(
                BrotliDecoder::new(io::Cursor::new(bytes)),
                limit,
            ))
            .await
        }
        Coding::Gzip => drain_capped(GzipDecoder::new(io::Cursor::new(bytes)), limit).await,
    }
}

/// The codings `headers` declares, in the order they were applied.
///
/// `None` for an undecodable token or more than [`MAX_CODINGS`]. `identity` is
/// skipped rather than refused (RFC 9110 section 8.4).
fn declared(headers: &http::HeaderMap) -> Option<Vec<Coding>> {
    let mut codings = Vec::new();

    for value in headers.get_all(http::header::CONTENT_ENCODING) {
        let text = value.to_str().ok()?;

        for token in text.split(',') {
            let token = token.trim();
            if token.is_empty() || token.eq_ignore_ascii_case("identity") {
                continue;
            }

            codings.push(Coding::from_token(token)?);

            if codings.len() > MAX_CODINGS {
                return None;
            }
        }
    }

    Some(codings)
}

/// Decodes a request body the client announced a content coding for.
///
/// A body arriving under `Content-Encoding: gzip` reaches the handler's
/// extractor as the bytes it decoded to, so a handler never knows a coding was
/// involved. A body naming a coding this server does not decode is refused with
/// 415 carrying `Accept-Encoding`, per RFC 9110 sections 8.4 and 15.5.16.
///
/// ```no_run
/// # #[cfg(feature = "compression")]
/// # {
/// use kynos::middleware::decompression::Decompression;
///
/// // Sixteen megabytes decoded, and never more than sixty-four times what
/// // arrived.
/// let decompression = Decompression::new(16 * 1024 * 1024).max_ratio(64);
/// # }
/// ```
///
/// # The limit replaces `BodySize`
///
/// A cap measured before decoding is not a cap: two kilobytes of zeroes are a
/// gigabyte of gzip output. The limit here applies to what the handler will
/// see — the decoded octets, or the bytes as they arrived when no coding was
/// applied. Mounting
/// [`BodySize`](crate::middleware::limits::body_size::BodySize) beside this is
/// a compile error, since both answer 413.
///
/// # `max_ratio` is off unless you set it
///
/// It refuses a bomb earlier, but no single default suits all three codings:
/// gzip tops out near 1032:1 while zstd and brotli go far beyond, and highly
/// compressible legitimate payloads (`"kynos "` repeated four thousand times
/// gzips past 200:1) are what a wrong ratio refuses. The absolute limit alone
/// already bounds memory. Around 20 suits JSON APIs; measure before choosing.
///
/// # What it costs a streaming read
///
/// 413 and 415 are declared and must be answerable before the handler runs, so
/// the body is decoded here in full and handed on as bytes.
///
/// # What is stripped
///
/// Per RFC 9110 section 8.4, representation metadata describes the coded form,
/// so decoding removes `Content-Encoding`, restates `Content-Length` as the
/// decoded length, and removes `Content-Digest`, `Digest` and `Content-MD5`.
///
/// # Naming what each refusal is
///
/// Three refusals, three builders:
/// [`unsupported_coding_problem_type`](Decompression::unsupported_coding_problem_type),
/// [`malformed_problem_type`](Decompression::malformed_problem_type) and
/// [`too_large_problem_type`](Decompression::too_large_problem_type); see
/// [`Undecodable`].
pub struct Decompression<U = (), M = (), L = ()> {
    /// The largest body, decoded, that will be handed on.
    limit: u64,
    /// The largest decoded-to-encoded ratio handed on, when set.
    max_ratio: Option<u64>,
    /// Names each refusal's problem type without holding one.
    problem_type: Markers<U, M, L>,
}

impl Decompression<(), (), ()> {
    /// Decodes request bodies, capping the decoded body at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            limit: bytes,
            max_ratio: None,
            problem_type: PhantomData,
        }
    }
}

impl<M, L> Decompression<(), M, L> {
    /// Names the RFC 9457 problem type the 415 carries.
    ///
    /// Available only where this refusal has not been named, so a chain names
    /// each of the three at most once and in any order.
    ///
    /// ```
    /// # #[cfg(feature = "compression")]
    /// # {
    /// use kynos::{error::problem::ProblemType, middleware::decompression::Decompression};
    ///
    /// struct UnknownCoding;
    /// # impl ProblemType for UnknownCoding {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/coding");
    /// # }
    /// struct Bomb;
    ///
    /// impl ProblemType for Bomb {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/bomb");
    /// }
    ///
    /// let decompression = Decompression::new(1 << 20)
    ///     .unsupported_coding_problem_type::<UnknownCoding>()
    ///     .too_large_problem_type::<Bomb>();
    /// # let _ = decompression;
    /// # }
    /// ```
    ///
    /// Naming one of them twice does not compile:
    ///
    /// ```compile_fail
    /// use kynos::{error::problem::ProblemType, middleware::decompression::Decompression};
    ///
    /// struct UnknownCoding;
    /// # impl ProblemType for UnknownCoding {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/coding");
    /// # }
    /// struct AlsoUnknown;
    /// # impl ProblemType for AlsoUnknown {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/also");
    /// # }
    ///
    /// let decompression = Decompression::new(1 << 20)
    ///     .unsupported_coding_problem_type::<UnknownCoding>()
    ///     .unsupported_coding_problem_type::<AlsoUnknown>();
    /// # let _ = decompression;
    /// ```
    #[must_use]
    pub fn unsupported_coding_problem_type<U: ProblemType>(self) -> Decompression<U, M, L> {
        self.renamed()
    }
}

impl<U, L> Decompression<U, (), L> {
    /// Names the RFC 9457 problem type the 400 carries.
    ///
    /// Available only where this refusal has not been named. See
    /// [`unsupported_coding_problem_type`](Decompression::unsupported_coding_problem_type).
    #[must_use]
    pub fn malformed_problem_type<M: ProblemType>(self) -> Decompression<U, M, L> {
        self.renamed()
    }
}

impl<U, M> Decompression<U, M, ()> {
    /// Names the RFC 9457 problem type the 413 carries.
    ///
    /// Available only where this refusal has not been named. See
    /// [`unsupported_coding_problem_type`](Decompression::unsupported_coding_problem_type).
    #[must_use]
    pub fn too_large_problem_type<L: ProblemType>(self) -> Decompression<U, M, L> {
        self.renamed()
    }
}

impl<U, M, L> Decompression<U, M, L> {
    /// The same configuration under a different set of markers; the
    /// destructuring makes a newly added field a compile error here.
    fn renamed<U2, M2, L2>(self) -> Decompression<U2, M2, L2> {
        let Self {
            limit,
            max_ratio,
            problem_type: _,
        } = self;

        Decompression {
            limit,
            max_ratio,
            problem_type: PhantomData,
        }
    }

    /// Refuses a body that decodes to more than `times` its arrived size.
    ///
    /// Off unless set; see the type's documentation for choosing a value.
    #[must_use]
    pub fn max_ratio(mut self, times: u64) -> Self {
        self.max_ratio = Some(times);
        self
    }

    /// The tighter of the two caps, given `encoded` bytes arrived.
    fn bound(self, encoded: u64) -> u64 {
        match self.max_ratio {
            Some(ratio) => self.limit.min(encoded.saturating_mul(ratio)),
            None => self.limit,
        }
    }
}

// Written out: `#[derive]` would bound each on all three markers.

impl<U, M, L> Clone for Decompression<U, M, L> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U, M, L> Copy for Decompression<U, M, L> {}

impl<U, M, L> fmt::Debug for Decompression<U, M, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            limit,
            max_ratio,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("Decompression")
            .field("limit", limit)
            .field("max_ratio", max_ratio)
            .finish()
    }
}

impl<C, U, M, L> Interceptor<C> for Decompression<U, M, L>
where
    C: Sync + 'static,
    U: ProblemType,
    M: ProblemType,
    L: ProblemType,
{
    type Reads = ();
    type Adds = ();
    type Short = Undecodable<U, M, L>;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Undecodable<U, M, L>> {
        let _ = (reads, context);

        let Some(codings) = declared(request.headers()) else {
            return Err(Undecodable::unsupported_coding());
        };

        let (mut parts, body) = request.into_parts();

        // The route's body limit, replacing the extractor's default.
        parts.extensions.insert(BodyLimit(self.limit));

        // Read even when uncoded: the limit applies either way.
        let arrived = match collect_capped(body, self.limit)
            .await
            .map_err(Undecodable::of)?
        {
            Collected::Whole(bytes) => bytes,
            // Handed on as it failed, so the extractor refuses it as usual
            // rather than blaming the coding for a transport failure.
            Collected::FailedPartWay(body) => {
                return Ok(next.run(http::Request::from_parts(parts, body)).await);
            }
        };

        let mut bytes = arrived;
        // Applied in the order listed, so undone in the reverse of it.
        for coding in codings.iter().rev().copied() {
            let bound = self.bound(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            bytes = decode(coding, bytes, bound)
                .await
                .map_err(Undecodable::of)?;
        }

        // An uncoded request's metadata still describes its body exactly.
        if !codings.is_empty() {
            parts.headers.remove(http::header::CONTENT_ENCODING);

            // Removed, not recomputed: a digest rewritten in transit proves
            // nothing about what the client sent.
            for stale in ["content-digest", "digest", "content-md5"] {
                parts.headers.remove(stale);
            }

            if let Ok(length) = http::HeaderValue::from_str(&bytes.len().to_string()) {
                parts.headers.insert(http::header::CONTENT_LENGTH, length);
            }
        }

        let request = http::Request::from_parts(parts, Body::from_bytes(bytes));

        Ok(next.run(request).await)
    }
}

/// What reading a request body within the limit produced.
enum Collected {
    /// Every byte the body carried.
    Whole(Bytes),
    /// What arrived before the read failed, then the same failure.
    FailedPartWay(Body),
}

/// Reads `body` while the running total stays within `limit`.
///
/// A failed read comes back as it failed, for the extractor beneath to refuse;
/// swallowing it would hand on a truncated payload.
async fn collect_capped(mut body: Body, limit: u64) -> Result<Collected, Reason> {
    let mut collected = BytesMut::new();

    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return Ok(Collected::FailedPartWay(Body::failed_after(
                    collected.freeze(),
                    error,
                )));
            }
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };

        let so_far = u64::try_from(collected.len()).unwrap_or(u64::MAX);
        let arriving = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if so_far.saturating_add(arriving) > limit {
            return Err(Reason::TooLarge { limit });
        }

        collected.extend_from_slice(&data);
    }

    Ok(Collected::Whole(collected.freeze()))
}

#[cfg(test)]
mod tests;
