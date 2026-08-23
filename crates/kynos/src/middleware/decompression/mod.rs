//! Request-body decompression.
//!
//! The other direction from [`compression`](super::compression), and a separate
//! negotiation: RFC 9110 section 12.5.3's `Accept-Encoding` says what a *client*
//! will take back, and says nothing about what it may send. What a client sends
//! is announced in `Content-Encoding` (section 8.4) and is not negotiated at
//! all -- it arrives, and the server either understands it or refuses it.
//!
//! Out-of-document, like its counterpart: content coding is transport, and
//! OpenAPI models neither direction. The refusals are declared, because a
//! status a route can answer with is part of its contract whatever produced it.

use std::io;

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZstdDecoder};
use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use kynos_openapi::model::schema::types::SchemaType;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    error::problem::Problem,
    http,
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
    /// The coding `token` names.
    ///
    /// Case-insensitive, because RFC 9110 section 8.4.1 says content codings
    /// are. `x-gzip` is the deprecated spelling section 8.4.1.3 keeps as an
    /// alias for `gzip`, and clients still send it.
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

/// What this server would have accepted, most preferred first.
///
/// The value RFC 9110 section 15.5.16 says *ought to* ride on a 415 caused by
/// an unsupported content coding, so the client learns what to send instead of
/// guessing.
const ACCEPTED: &str = "zstd, br, gzip";

/// The longest chain of codings that will be decoded.
///
/// `Content-Encoding` is a list, and each entry costs a decode pass over a body
/// already bounded by the configured limit -- so a hundred-entry list is a
/// hundred times the work for one request. No real client sends more than one.
const MAX_CODINGS: usize = 4;

/// What [`Decompression`] answers with when it will not hand a body on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undecodable {
    /// The body named a content coding this server cannot decode.
    UnsupportedCoding,
    /// The body did not decode as the coding it claimed.
    Malformed,
    /// The decoded body passed the configured bound.
    TooLarge {
        /// The bound it passed, in bytes.
        limit: u64,
    },
}

impl IntoResponse for Undecodable {
    fn into_response(self) -> http::Response {
        match self {
            Self::UnsupportedCoding => {
                let mut response = Problem::new(http::StatusCode::UNSUPPORTED_MEDIA_TYPE)
                    .with_detail(format!(
                        "the request body's content coding is not one this server decodes; \
                         it accepts {ACCEPTED}"
                    ))
                    .into_response();

                // Only on this 415, never on someone else's. A 415 raised by an
                // unsupported *media type* that carried `Accept-Encoding` would
                // read as a complaint about the coding, and section 15.5.16
                // keeps the two answers apart for exactly that reason.
                response.headers_mut().insert(
                    http::header::ACCEPT_ENCODING,
                    http::HeaderValue::from_static(ACCEPTED),
                );

                response
            }
            Self::Malformed => Problem::new(http::StatusCode::BAD_REQUEST)
                .with_detail("the request body did not decode as the coding it declared")
                .into_response(),
            Self::TooLarge { limit } => Problem::new(http::StatusCode::PAYLOAD_TOO_LARGE)
                .with_detail(format!(
                    "the request body exceeds {limit} bytes once decoded"
                ))
                .into_response(),
        }
    }
}

impl ShortCircuit for Undecodable {
    const STATUSES: &'static [u16] = &[400, 413, 415];
}

impl Responses for Undecodable {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;

        kynos_openapi::Responses::new()
            .with(
                400,
                kynos_openapi::Response::new(
                    "the request body did not decode as the coding it declared",
                ),
            )
            .with(
                413,
                kynos_openapi::Response::new("the request body exceeds the configured limit"),
            )
            .with(
                415,
                kynos_openapi::Response::new(
                    "the request body's content coding is not one this server decodes",
                )
                .with_header("Accept-Encoding", accepted_encoding_header()),
            )
    }
}

/// Describes the `Accept-Encoding` that rides on the 415.
fn accepted_encoding_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description("The content codings this server decodes")
}

/// Reads `source` to its end, refusing to produce more than `limit` bytes.
///
/// The cap is checked on the chunk that passes it rather than after the whole
/// body has arrived, which is what makes it a defence: a bomb is refused while
/// it is still a few kilobytes of memory.
async fn drain_capped<R: AsyncRead + Unpin>(
    mut source: R,
    limit: u64,
) -> Result<Bytes, Undecodable> {
    let mut decoded = BytesMut::new();
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let read = std::future::poll_fn(|context| {
            let mut buffer = ReadBuf::new(&mut chunk);
            std::task::ready!(std::pin::Pin::new(&mut source).poll_read(context, &mut buffer))?;
            std::task::Poll::Ready(io::Result::Ok(buffer.filled().len()))
        })
        .await
        .map_err(|_| Undecodable::Malformed)?;

        if read == 0 {
            return Ok(decoded.freeze());
        }

        let so_far = u64::try_from(decoded.len()).unwrap_or(u64::MAX);
        if so_far.saturating_add(u64::try_from(read).unwrap_or(u64::MAX)) > limit {
            return Err(Undecodable::TooLarge { limit });
        }

        decoded.extend_from_slice(&chunk[..read]);
    }
}

/// Removes `coding` from `bytes`, producing no more than `limit` bytes.
async fn decode(coding: Coding, bytes: Bytes, limit: u64) -> Result<Bytes, Undecodable> {
    match coding {
        Coding::Zstd => drain_capped(ZstdDecoder::new(io::Cursor::new(bytes)), limit).await,
        // Boxed for the same reason the encoder is: brotli's state is measured
        // in kilobytes, and an interceptor's future is held for the whole
        // exchange.
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
/// `None` when a token names something this server cannot decode, or when the
/// list is longer than [`MAX_CODINGS`]. `identity` is dropped rather than
/// refused: RFC 9110 section 8.4 says it SHOULD NOT appear, and a sender that
/// includes it anyway means the body was not encoded.
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
/// # Why the limit is required, and why this replaces `BodySize`
///
/// A cap measured before decoding is not a cap. Two kilobytes of zeroes are a
/// gigabyte of gzip output, so
/// [`BodySize`](crate::middleware::limits::BodySize) guarding a route that
/// accepts codings guards nothing -- it measures the one number the attacker
/// controls freely.
///
/// So the limit here is the route's body limit, applied to whatever the handler
/// will actually see: the decoded octets when a coding was applied, and the
/// bytes as they arrived when none was. Mounting `BodySize` beside this is a
/// compile error, since both answer 413 and a consumer could not tell which
/// replied — and it would be redundant as well as ambiguous.
///
/// # Why `max_ratio` is off unless you set it
///
/// It is the cheaper of the two checks and catches the same attack earlier: a
/// body expanding past a plausible multiple of its own size is refused while it
/// is still kilobytes. But no single number is right for the three codings.
/// gzip cannot exceed about 1032:1; zstd's long-range matching goes orders of
/// magnitude beyond that, and brotli's static dictionary makes small inputs
/// expand further still. A default tight enough to be worth having under gzip
/// refuses payloads zstd produces legitimately.
///
/// It is also the check that refuses *real* traffic when it is wrong, and the
/// traffic it refuses is the most compressible — a sparse matrix, a padded
/// document, a log batch of near-identical lines. `"kynos "` repeated four
/// thousand times gzips past 200:1, and it is not an attack.
///
/// So the absolute limit is required and this is not. The absolute limit is
/// already a complete defence: it bounds the memory a request can cost. This is
/// how you buy the refusal earlier, once you know what your own payloads look
/// like. Around 20 suits JSON APIs; measure before choosing.
///
/// # What it costs a streaming read
///
/// The same as `BodySize`, and for the same reason: 413 and 415 are declared,
/// and a declared status has to be answerable before the handler runs. So the
/// body is decoded here in full and handed on as bytes. A coded body is not a
/// streaming upload in any case — it cannot be, since the coding has to be
/// undone before anything can read a record out of it.
///
/// # What is stripped, and why
///
/// RFC 9110 section 8.4 says the representation *is* the coded form, and that
/// "all other metadata about the representation is about the coded form".
/// Decoding therefore invalidates that metadata rather than preserving it:
/// `Content-Encoding` is removed, `Content-Length` is restated as the decoded
/// length, and `Content-Digest`, `Digest` and `Content-MD5` are removed rather
/// than left to be checked against octets they were never computed over.
#[derive(Clone, Copy, Debug)]
pub struct Decompression {
    /// The largest body, decoded, that will be handed on.
    limit: u64,
    /// The largest decoded-to-encoded ratio that will be handed on, when one
    /// was set.
    max_ratio: Option<u64>,
}

impl Decompression {
    /// Decodes request bodies, capping the decoded body at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            limit: bytes,
            max_ratio: None,
        }
    }

    /// Refuses a body that decodes to more than `times` its arrived size.
    ///
    /// Off unless set, and deliberately: see the type's documentation for why
    /// one default cannot be right for three codings.
    #[must_use]
    pub fn max_ratio(mut self, times: u64) -> Self {
        self.max_ratio = Some(times);
        self
    }

    /// The cap to decode under, given `encoded` bytes arrived.
    ///
    /// The tighter of the two bounds, so one pass enforces both.
    fn bound(self, encoded: u64) -> u64 {
        match self.max_ratio {
            Some(ratio) => self.limit.min(encoded.saturating_mul(ratio)),
            None => self.limit,
        }
    }
}

impl<C: Sync + 'static> Interceptor<C> for Decompression {
    type Reads = ();
    type Adds = ();
    type Short = Undecodable;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Undecodable> {
        let _ = (reads, context);

        let Some(codings) = declared(request.headers()) else {
            return Err(Undecodable::UnsupportedCoding);
        };

        let (mut parts, body) = request.into_parts();

        // Read once, whether or not a coding was applied: the limit is the
        // route's body limit, and a request that skipped the coding is not
        // thereby exempt from it.
        let arrived = collect_capped(body, self.limit).await?;

        let mut bytes = arrived;
        // Applied in the order listed, so undone in the reverse of it.
        for coding in codings.iter().rev().copied() {
            let bound = self.bound(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            bytes = decode(coding, bytes, bound).await?;
        }

        // Only what the decode invalidated. A request that carried no coding
        // carried no coded form either, so its metadata still describes its
        // body exactly -- and stripping a digest from it would destroy a fact
        // the handler may be relying on.
        if !codings.is_empty() {
            parts.headers.remove(http::header::CONTENT_ENCODING);

            // Metadata about a coded form that no longer exists. Removed rather
            // than recomputed: this is not the party that computed it, and a
            // digest rewritten in transit proves nothing about what the client
            // sent.
            for stale in ["content-digest", "digest", "content-md5"] {
                parts.headers.remove(stale);
            }

            if let Ok(length) = http::HeaderValue::from_str(&bytes.len().to_string()) {
                parts.headers.insert(http::header::CONTENT_LENGTH, length);
            }
        }

        let request = http::Request::from_parts(parts, crate::http::body::Body::from_bytes(bytes));

        Ok(next.run(request).await)
    }
}

/// Reads `body` while the running total stays within `limit`.
async fn collect_capped(
    mut body: crate::http::body::Body,
    limit: u64,
) -> Result<Bytes, Undecodable> {
    let mut collected = BytesMut::new();

    while let Some(frame) = body.frame().await {
        // A read that fails is not a size violation and must not be reported as
        // one: what arrived is handed on, and the extractor beneath rejects a
        // truncated payload with the status it already describes.
        let Ok(frame) = frame else { break };
        let Ok(data) = frame.into_data() else {
            continue;
        };

        let so_far = u64::try_from(collected.len()).unwrap_or(u64::MAX);
        let arriving = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if so_far.saturating_add(arriving) > limit {
            return Err(Undecodable::TooLarge { limit });
        }

        collected.extend_from_slice(&data);
    }

    Ok(collected.freeze())
}

#[cfg(test)]
mod tests;
