//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use std::{io, marker::PhantomData, pin::Pin, task::Poll};

use async_compression::{
    Level,
    tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder},
};
use bytes::{Bytes, BytesMut};
use http_body::Body as _;
use http_body_util::BodyExt;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    error::problem::ProblemType,
    extract::params::header::{EncodeHeaders, HeaderParams},
    http,
    middleware::{
        Continued, Interceptor, Next,
        compression::{
            levels::{BrotliLevel, GzipLevel, ZstdLevel},
            policy::Encoding,
            streaming::{LatencyMode, Streamed},
        },
    },
};

/// A content coding this crate can produce, ordered by server preference to
/// break ties between equally weighted codings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
    Zstd,
    Brotli,
    Gzip,
}

impl Coding {
    /// Every coding, most preferred first.
    const ALL: [Self; 3] = [Self::Zstd, Self::Brotli, Self::Gzip];

    /// The token this coding is named by on the wire.
    fn token(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }
}

/// What compression sets on a response it encoded.
///
/// Both headers are defined by HTTP itself, so they are declared (to stop a
/// second interceptor writing them) but not described in the document.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentEncoding {
    /// The coding applied, or `None` when the response was left as it was.
    coding: Option<Coding>,
    /// The encoded length, where a coding was applied.
    length: Option<usize>,
}

impl HeaderParams for ContentEncoding {
    const NAMES: &'static [&'static str] = &["content-encoding", "content-length"];
    const DESCRIBED: bool = false;
    // On every response, encoded or not. A union set rather than in `NAMES`, so
    // `Compression` and `Cors` can cover one route.
    const VARIES: &'static [&'static str] = &["accept-encoding"];
}

impl EncodeHeaders for ContentEncoding {
    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let Some(coding) = self.coding else {
            return Vec::new();
        };

        let mut fields = vec![(
            http::header::CONTENT_ENCODING,
            http::HeaderValue::from_static(coding.token()),
        )];

        // A pre-encoding length is known to be incorrect (RFC 9110 sections 8.4,
        // 8.6); restated rather than removed so it stays right if the body stops
        // being buffered.
        if let Some(length) = self.length {
            if let Ok(value) = http::HeaderValue::from_str(&length.to_string()) {
                fields.push((http::header::CONTENT_LENGTH, value));
            }
        }

        fields
    }
}

/// Whether the response carries a *strong* validator, which encoding would make
/// name two representations (RFC 9110 section 8.8.1). A weak one may.
fn strongly_tagged(headers: &http::HeaderMap) -> bool {
    headers
        .get(http::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tag| !crate::http::etag::is_weak(tag.trim()))
}

/// What a request refusing every available representation is answered with.
///
/// RFC 9110 section 12.4.1: when no available representation is acceptable, the
/// origin server "can either honor the header field by sending a 406 (Not
/// Acceptable) response or disregard the header field". Kynos honours it.
///
/// Reachable only by excluding identity *and* leaving every coding this build
/// offers unacceptable, or when a handler requires an encoding
/// ([`policy::Encoding::Required`]) and none can be applied.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`Compression::problem_type`].
pub struct NotAcceptable<T = ()> {
    /// `fn() -> T`, so the refusal is `Send` and `Sync` whatever the marker is.
    problem_type: PhantomData<fn() -> T>,
}

impl<T> NotAcceptable<T> {
    /// The refusal itself, which carries nothing but its type.
    #[must_use]
    pub fn new() -> Self {
        Self {
            problem_type: PhantomData,
        }
    }
}

impl<T> Default for NotAcceptable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ProblemType> crate::response::IntoResponse for NotAcceptable<T> {
    fn into_response(self) -> http::Response {
        crate::error::problem::refusal_problem::<T>(http::StatusCode::NOT_ACCEPTABLE)
            .with_detail("no representation of this resource has an acceptable content coding")
            .into_response()
    }
}

impl<T: ProblemType> crate::response::ShortCircuit for NotAcceptable<T> {
    const STATUSES: &'static [u16] = &[406];
}

impl<T: ProblemType> crate::response::Responses for NotAcceptable<T> {
    fn responses(registry: &mut crate::schema::registry::Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            406,
            crate::error::problem::refusal_response::<T>(
                registry,
                406,
                "no representation has a content coding the request accepts",
            ),
        )
    }
}

// Written out because `#[derive]` would bound each on the marker type.

impl<T> Clone for NotAcceptable<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NotAcceptable<T> {}

impl<T> std::fmt::Debug for NotAcceptable<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { problem_type: _ } = self;

        formatter.debug_struct("NotAcceptable").finish()
    }
}

impl<T> PartialEq for NotAcceptable<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self { problem_type: _ } = self;
        let Self { problem_type: _ } = other;

        true
    }
}

impl<T> Eq for NotAcceptable<T> {}

/// What negotiation decided, per RFC 9110 section 12.5.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Negotiated {
    /// Encode with this coding.
    Encode(Coding),
    /// Send the representation as it is.
    Identity,
    /// Nothing is acceptable, identity included; answered 406 (section 12.4.1).
    Nothing,
}

/// The coding to apply, per RFC 9110 section 12.5.3.
fn negotiate(headers: &http::HeaderMap) -> Negotiated {
    let Some(accept) = headers
        .get(http::header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    else {
        // Rule 1: "If no Accept-Encoding header field is in the request, any
        // content coding is considered acceptable by the user agent."
        return Negotiated::Identity;
    };

    let mut best: Option<(Coding, u16)> = None;
    for coding in Coding::ALL {
        let Some(weight) = crate::http::coding::quality(accept, coding.token()) else {
            continue;
        };

        if weight == 0 {
            continue;
        }

        if best.is_none_or(|(_, best)| weight > best) {
            best = Some((coding, weight));
        }
    }

    // Rule 2: identity is excluded only by `identity;q=0`, or `*;q=0` with no
    // more specific `identity` entry; a weight that is not a qvalue excludes
    // nothing.
    let identity = crate::http::coding::identity_quality(accept);

    let Some((coding, weight)) = best else {
        return if identity > 0 {
            Negotiated::Identity
        } else {
            // Every coding unacceptable and identity excluded. An empty field
            // value excludes nothing, so it resolves to identity above.
            Negotiated::Nothing
        };
    };

    // A tie goes to the coding, so plain `Accept-Encoding: gzip` encodes.
    if identity <= weight {
        Negotiated::Encode(coding)
    } else {
        Negotiated::Identity
    }
}

/// How much of an encoder's output one read takes.
///
/// Mirrored by `DRAIN_CHUNK` in `tests/alloc_codecs.rs`; change both together.
const DRAIN_CHUNK: usize = 8 * 1024;

/// Reads an encoder to its end, by hand so compression needs nothing of tokio
/// beyond `AsyncRead`.
async fn drain<R: AsyncRead + Unpin>(mut source: R) -> io::Result<Bytes> {
    let mut encoded = BytesMut::new();
    let mut chunk = [0_u8; DRAIN_CHUNK];

    loop {
        let read = std::future::poll_fn(|context| {
            let mut buffer = ReadBuf::new(&mut chunk);
            std::task::ready!(Pin::new(&mut source).poll_read(context, &mut buffer))?;
            Poll::Ready(io::Result::Ok(buffer.filled().len()))
        })
        .await?;

        if read == 0 {
            return Ok(encoded.freeze());
        }

        encoded.extend_from_slice(&chunk[..read]);
    }
}

/// Applies `coding` to `bytes` at the level `levels` sets for it.
async fn encode(coding: Coding, bytes: Bytes, levels: Levels) -> io::Result<Bytes> {
    match coding {
        Coding::Zstd => {
            drain(ZstdEncoder::with_quality(
                io::Cursor::new(bytes),
                Level::Precise(levels.zstd.get()),
            ))
            .await
        }
        // Boxed because brotli's encoder state is measured in kilobytes, and an
        // interceptor's future is held for the whole exchange.
        Coding::Brotli => {
            Box::pin(drain(BrotliEncoder::with_quality(
                io::Cursor::new(bytes),
                Level::Precise(as_level(levels.brotli.get())),
            )))
            .await
        }
        Coding::Gzip => {
            drain(GzipEncoder::with_quality(
                io::Cursor::new(bytes),
                Level::Precise(as_level(levels.gzip.get())),
            ))
            .await
        }
    }
}

/// The level as `async-compression` spells one. The fallback is unreachable:
/// both levels reaching here are bounded at 11.
fn as_level(level: u32) -> i32 {
    i32::try_from(level).unwrap_or(i32::MAX)
}

/// What each algorithm is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Levels {
    /// What gzip is asked for.
    pub(crate) gzip: GzipLevel,
    /// What brotli is asked for.
    pub(crate) brotli: BrotliLevel,
    /// What zstd is asked for.
    pub(crate) zstd: ZstdLevel,
}

/// Compresses responses when the client accepts it.
///
/// ```no_run
/// # #[cfg(feature = "compression")]
/// # {
/// use kynos::middleware::compression::{Compression, levels::GzipLevel};
///
/// // Everything at its default level, except gzip, which this service serves
/// // enough of to care about the CPU; and bodies from 1 KiB rather than the
/// // default 2 KiB, because this service's clients pay by the octet.
/// let compression = Compression::new()
///     .min_size(1_024)
///     .gzip_level(GzipLevel::FASTEST);
/// # }
/// ```
///
/// # Levels, and the scope they are set at
///
/// Each algorithm keeps its own level type — [`GzipLevel`], [`BrotliLevel`],
/// [`ZstdLevel`] — since the three number their levels differently. Each
/// refuses a number its own format does not define, and none converts into
/// another.
///
/// The defaults are gzip 6, brotli **4** and zstd 3. Brotli's departs from its
/// reference encoder's 11, which is meant for content compressed once and
/// served many times; see [`BrotliLevel::DEFAULT`].
///
/// Levels are set per mount, and a mount is a scope: a `Compression` on the
/// router covers everything, one on a [`Group`](crate::router::group::Group)
/// covers that group, one on an endpoint covers that endpoint. A global one
/// plus a per-endpoint override is refused by
/// [`CompatibleWith`](crate::middleware::stack::CompatibleWith), since both
/// would add `Content-Encoding` to the same operation. Mount the one that varies
/// and leave the rest uncovered.
///
/// # A body still being produced is encoded as it arrives
///
/// A response whose length is already known is collected and encoded in one
/// pass. One whose length is not — an event stream, a log tail, an export — is
/// encoded frame by frame. `min_size` does not apply there.
///
/// [`LatencyMode`] sets how eagerly encoded bytes are handed on. Its default,
/// [`Interactive`](LatencyMode::Interactive), flushes after every frame, so an
/// idle event stream still delivers each event promptly. Choose [`Throughput`](LatencyMode::Throughput) for a body that is
/// a stream only because it is large.
///
/// No `Content-Length` rides on a streamed encode: the encoded length is not
/// known until after the head has gone, and RFC 9110 section 8.6 forbids
/// forwarding one known to be incorrect.
///
/// # What a handler may say about its own response
///
/// [`policy::Encoding`], attached with
/// [`WithEncoding`](policy::WithEncoding), overrules negotiation in both
/// directions for one response:
/// [`Disabled`](policy::Encoding::Disabled) for a body that reflects a secret
/// back beside attacker-chosen input, and
/// [`Required`](policy::Encoding::Required) for one too large to be worth
/// sending as it is — which makes identity unacceptable, so a client that will
/// take only identity is answered 406 rather than handed the whole
/// representation.
///
/// # A response that ranges is never encoded
///
/// Anything carrying `Accept-Ranges` is left as it is, as are a 206, a 416 and
/// anything carrying `Content-Range`. RFC 9110 section 14.1.2 calculates a byte
/// range over the *encoded* octets while Kynos calculates one over the identity
/// octets, and section 8.8.1 will not let one strong validator name both forms.
///
/// **This costs real bandwidth**: an
/// [`AssetSet`](crate::router::assets::AssetSet) advertises ranges on every
/// file it serves, so a stylesheet or a bundle under this interceptor ships
/// uncompressed. Two ways out:
///
/// * mount `Compression` on a [`Group`](crate::router::group::Group) that does
///   not cover the asset set, so the API is encoded and the files are ranged;
/// * let a reverse proxy or CDN encode them, which is sound only because it
///   owns the validator it sends as well as the coding.
///
/// # A strong validator stops the encoder too
///
/// A response carrying a *strong* `ETag` is left as it is, even a 200 that
/// advertises no ranges: encoding it would make one strong validator name two
/// representations, against RFC 9110 section 8.8.1. A *weak* validator does not
/// stop the encoder, and is the right validator for a representation that
/// exists in several codings. This applies however the tag arrives, from a
/// [`Cache`](crate::middleware::cache::Cache) mounted inside this or from the
/// handler.
///
/// # Naming what the 406 is
///
/// [`problem_type`](Compression::problem_type) puts an application's own URI on
/// the refusal, telling it apart from a 406 a handler answers for `Accept`.
pub struct Compression<T = ()> {
    /// The smallest response worth encoding, in bytes.
    min_size: u64,
    /// What each algorithm is asked for.
    levels: Levels,
    /// How eagerly a streamed body's encoded bytes are handed on.
    latency: LatencyMode,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

/// The smallest body encoded by default: 2 KiB, the smallest at which every
/// coding saves an Ethernet segment on a body that shrinks by 71% or more.
/// [`middleware.md`](../../../../docs/middleware.md) records the measurement.
pub(crate) const DEFAULT_MIN_SIZE: u64 = 2_048;

impl Compression<()> {
    /// Enables every compiled-in algorithm, at each one's default level, for
    /// bodies of at least 2 KiB — see [`min_size`](Self::min_size).
    #[must_use]
    pub fn new() -> Self {
        Self {
            min_size: DEFAULT_MIN_SIZE,
            levels: Levels::default(),
            latency: LatencyMode::default(),
            problem_type: PhantomData,
        }
    }

    /// Names the RFC 9457 problem type this interceptor's 406 carries.
    ///
    /// Available only on a `Compression` that has not named one. See
    /// [`BodySize::problem_type`](crate::middleware::limits::body_size::BodySize::problem_type)
    /// for the rule.
    ///
    /// ```
    /// # #[cfg(feature = "compression")]
    /// # {
    /// use kynos::{error::problem::ProblemType, middleware::compression::Compression};
    ///
    /// struct NoAcceptableCoding;
    ///
    /// impl ProblemType for NoAcceptableCoding {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/coding");
    /// }
    ///
    /// let compression = Compression::new().problem_type::<NoAcceptableCoding>();
    /// # let _ = compression;
    /// # }
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> Compression<T> {
        Compression {
            min_size: self.min_size,
            levels: self.levels,
            latency: self.latency,
            problem_type: PhantomData,
        }
    }
}

impl<T> Compression<T> {
    /// Skips responses whose known length is smaller than `bytes`.
    ///
    /// 2048 by default: below about one Ethernet segment of saving, encoding
    /// sends the same packets and only spends CPU. Lower it for a body that
    /// compresses unusually well or a link that charges by the octet; `0`
    /// encodes every non-empty body. A body with no known length is encoded as
    /// it streams whatever this says.
    #[must_use]
    pub fn min_size(mut self, bytes: u64) -> Self {
        self.min_size = bytes;
        self
    }

    /// Sets what gzip is asked for.
    #[must_use]
    pub fn gzip_level(mut self, level: GzipLevel) -> Self {
        self.levels.gzip = level;
        self
    }

    /// Sets what brotli is asked for.
    #[must_use]
    pub fn brotli_level(mut self, level: BrotliLevel) -> Self {
        self.levels.brotli = level;
        self
    }

    /// Sets what zstd is asked for.
    #[must_use]
    pub fn zstd_level(mut self, level: ZstdLevel) -> Self {
        self.levels.zstd = level;
        self
    }

    /// Sets how eagerly a streamed body's encoded bytes are handed on.
    ///
    /// Affects only a body whose length is not known in advance. One already
    /// collected is encoded in a single pass, and there is nothing to trade.
    #[must_use]
    pub fn latency_mode(mut self, latency: LatencyMode) -> Self {
        self.latency = latency;
        self
    }
}

// Written out because `#[derive]` would bound each on the marker type.

impl<T> Clone for Compression<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Compression<T> {}

impl<T> std::fmt::Debug for Compression<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            min_size,
            levels,
            latency,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("Compression")
            .field("min_size", min_size)
            .field("levels", levels)
            .field("latency", latency)
            .finish()
    }
}

impl<T> Default for Compression<T> {
    fn default() -> Self {
        Self {
            min_size: DEFAULT_MIN_SIZE,
            levels: Levels::default(),
            latency: LatencyMode::default(),
            problem_type: PhantomData,
        }
    }
}

impl<C, T> Interceptor<C> for Compression<T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    type Reads = ();
    type Adds = ContentEncoding;

    /// 406, for a request that refused every representation this build can
    /// produce.
    type Short = NotAcceptable<T>;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<ContentEncoding>, NotAcceptable<T>> {
        let _ = (reads, context);

        let negotiated = negotiate(request.headers());

        // Refused before the chain runs: nothing it produced could be sent.
        if negotiated == Negotiated::Nothing {
            return Err(NotAcceptable::new());
        }

        let mut continued = next.run(request).await;

        // Leave alone a response already encoded, or one a byte range is or may be
        // calculated against: ranges and strong validators name the identity octets
        // (RFC 9110 sections 14.1.2, 8.8.1), so re-encoding would make them lie.
        let leave_alone = continued
            .headers()
            .contains_key(http::header::CONTENT_ENCODING)
            || strongly_tagged(continued.headers())
            || continued
                .headers()
                .contains_key(http::header::ACCEPT_RANGES)
            || matches!(
                continued.status(),
                http::StatusCode::PARTIAL_CONTENT | http::StatusCode::RANGE_NOT_SATISFIABLE
            )
            || continued
                .headers()
                .contains_key(http::header::CONTENT_RANGE);

        // The handler's own policy outranks negotiation in both directions.
        let policy = Encoding::of_extensions(continued.extensions());

        let refused = policy == Encoding::Disabled || leave_alone;
        let coding = match negotiated {
            Negotiated::Encode(coding) if !refused => coding,
            // `Required` excludes identity, so nothing is left. `leave_alone`
            // reaches here deliberately: its demands conflict, and 406 says so.
            _ if policy == Encoding::Required => return Err(NotAcceptable::new()),
            _ => return Ok(continued.with_headers(ContentEncoding::default())),
        };

        let body = continued.take_body();

        // An unknown length streams regardless of `min_size`; `Required`
        // outranks `min_size` but not an empty body.
        let worth_encoding = match body.size_hint().exact() {
            Some(length) => length > 0 && (length >= self.min_size || policy == Encoding::Required),
            None => true,
        };

        if !worth_encoding {
            continued.set_body(body);

            if policy == Encoding::Required {
                return Err(NotAcceptable::new());
            }

            return Ok(continued.with_headers(ContentEncoding::default()));
        }

        if body.size_hint().exact().is_none() {
            continued.set_body(crate::http::body::Body::from_body(Streamed::new(
                body,
                coding,
                self.levels,
                self.latency,
            )));

            // A stated length counts identity octets (RFC 9110 section 8.6),
            // and the encoded one is unknown until after the head has gone.
            continued.remove_declared::<ContentEncoding>(&http::header::CONTENT_LENGTH);

            return Ok(continued.with_headers(ContentEncoding {
                coding: Some(coding),
                length: None,
            }));
        }

        // Failing to encode sends identity; failing to read hands on a failing
        // body, since an empty one would read as complete.
        let encoded = match body.collect().await {
            Ok(collected) => {
                let bytes = collected.to_bytes();
                if let Ok(encoded) = encode(coding, bytes.clone(), self.levels).await {
                    let length = encoded.len();
                    continued.set_body(crate::http::body::Body::from_bytes(encoded));
                    Some((coding, length))
                } else {
                    continued.set_body(crate::http::body::Body::from_bytes(bytes));
                    None
                }
            }
            Err(error) => {
                continued.set_body(crate::http::body::Body::failed(error));
                None
            }
        };

        Ok(continued.with_headers(ContentEncoding {
            coding: encoded.map(|(coding, _)| coding),
            length: encoded.map(|(_, length)| length),
        }))
    }
}

pub mod levels;
pub mod policy;
pub mod streaming;

#[cfg(test)]
mod tests;
