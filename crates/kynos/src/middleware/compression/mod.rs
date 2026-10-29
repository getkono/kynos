//! Response compression.
//!
//! The coding is out-of-document: content coding is transport, and OpenAPI
//! does not model it. The 406 refusal is declared.

use std::{io, marker::PhantomData, pin::Pin, task::Poll};

use async_compression::{
    Level,
    tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder},
    zstd::CParameter,
};
use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    error::problem::ProblemType,
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
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

/// A content coding this crate can produce and decode, ordered by server
/// preference to break ties between equally weighted codings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
    Zstd,
    Brotli,
    Gzip,
}

impl Coding {
    /// Every coding, most preferred first.
    const ALL: [Self; 3] = [Self::Zstd, Self::Brotli, Self::Gzip];

    /// Every coding's token, in [`ALL`](Coding::ALL)'s order.
    const TOKENS: [&'static str; 3] = [
        Self::ALL[0].token(),
        Self::ALL[1].token(),
        Self::ALL[2].token(),
    ];

    /// The token this coding is named by on the wire.
    const fn token(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }

    /// The coding `token` names, case-insensitively (RFC 9110 section 8.4.1),
    /// accepting a deprecated alias such as `x-gzip` (section 8.4.1.3).
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|coding| crate::http::coding::names(token, coding.token()))
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
    fn encode(&self) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, UnencodableHeader> {
        let Some(coding) = self.coding else {
            return Ok(Vec::new());
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

        Ok(fields)
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
/// Reachable only by excluding identity *and* leaving zstd, br and gzip all
/// unacceptable, or when a handler requires an encoding
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
    /// The request names no coding, so every one is acceptable, identity
    /// included (rule 1). Sent as it is unless the response requires a coding,
    /// which is then the server's first preference.
    Unstated,
    /// Send the representation as it is.
    Identity,
    /// Nothing is acceptable, identity included; answered 406 (section 12.4.1).
    Nothing,
}

/// The coding to apply, per RFC 9110 section 12.5.3.
fn negotiate(headers: &http::HeaderMap) -> Negotiated {
    let Some(accept) = crate::http::field_lines(headers, http::header::ACCEPT_ENCODING) else {
        // Rule 1: "If no Accept-Encoding header field is in the request, any
        // content coding is considered acceptable by the user agent."
        return Negotiated::Unstated;
    };
    let accept = &*accept;

    // A tie with identity goes to the coding, so plain `Accept-Encoding: gzip`
    // encodes; a tie between codings goes to `Coding::ALL`'s order.
    if let Some(coding) =
        crate::http::coding::preferred(accept, &Coding::TOKENS).and_then(Coding::from_token)
    {
        return Negotiated::Encode(coding);
    }

    // Rule 2: identity is excluded only by `identity;q=0`, or `*;q=0` with no
    // more specific `identity` entry; a weight that is not a qvalue excludes
    // nothing.
    if crate::http::coding::identity_quality(accept) > 0 {
        Negotiated::Identity
    } else {
        // Every coding unacceptable and identity excluded. An empty field
        // value excludes nothing, so it resolves to identity above.
        Negotiated::Nothing
    }
}

/// How much of a codec's output one read takes.
///
/// Mirrored by `DRAIN_CHUNK` in `tests/alloc_codecs.rs`; change both together.
const DRAIN_CHUNK: usize = 8 * 1024;

/// Why [`drain`] stopped before its source ended.
#[derive(Debug)]
pub(crate) enum DrainError {
    /// The source failed.
    Read(io::Error),
    /// The output would have passed the cap.
    PastCap,
}

impl From<DrainError> for io::Error {
    /// The read's own failure; `PastCap` arises only under a cap, which
    /// encoding never sets.
    fn from(error: DrainError) -> Self {
        match error {
            DrainError::Read(error) => error,
            DrainError::PastCap => io::ErrorKind::FileTooLarge.into(),
        }
    }
}

/// Reads an encoder or a decoder to its end, by hand so neither direction
/// needs anything of tokio beyond `AsyncRead`.
///
/// With a `cap`, refuses to produce more than that many bytes. Checked per
/// chunk, so a decompression bomb is refused while still small.
pub(crate) async fn drain<R: AsyncRead + Unpin>(
    mut source: R,
    cap: Option<u64>,
) -> Result<Bytes, DrainError> {
    let mut drained = BytesMut::new();
    let mut chunk = [0_u8; DRAIN_CHUNK];

    loop {
        let read = std::future::poll_fn(|context| {
            let mut buffer = ReadBuf::new(&mut chunk);
            std::task::ready!(Pin::new(&mut source).poll_read(context, &mut buffer))?;
            Poll::Ready(io::Result::Ok(buffer.filled().len()))
        })
        .await
        .map_err(DrainError::Read)?;

        if read == 0 {
            return Ok(drained.freeze());
        }

        if let Some(cap) = cap {
            let so_far = u64::try_from(drained.len()).unwrap_or(u64::MAX);
            if so_far.saturating_add(u64::try_from(read).unwrap_or(u64::MAX)) > cap {
                return Err(DrainError::PastCap);
            }
        }

        drained.extend_from_slice(&chunk[..read]);
    }
}

/// Applies `coding` to `bytes` at the level `levels` sets for it.
///
/// Each drain is boxed: its 8 KiB buffer and the encoder's state come to
/// kilobytes, and inline they would widen the interceptor's future, which is
/// held for the whole exchange, on every response it declines too.
async fn encode(coding: Coding, bytes: Bytes, levels: Levels) -> io::Result<Bytes> {
    let encoded = match coding {
        Coding::Zstd => {
            Box::pin(drain(
                ZstdEncoder::with_quality_and_params(
                    io::Cursor::new(bytes),
                    Level::Precise(levels.zstd.get()),
                    zstd_window(levels.zstd).as_slice(),
                ),
                None,
            ))
            .await
        }
        Coding::Brotli => {
            Box::pin(drain(
                BrotliEncoder::with_quality(
                    io::Cursor::new(bytes),
                    Level::Precise(as_level(levels.brotli.get())),
                ),
                None,
            ))
            .await
        }
        Coding::Gzip => {
            Box::pin(drain(
                GzipEncoder::with_quality(
                    io::Cursor::new(bytes),
                    Level::Precise(as_level(levels.gzip.get())),
                ),
                None,
            ))
            .await
        }
    };

    encoded.map_err(io::Error::from)
}

/// The window cap zstd needs beside `level`, if it needs one.
///
/// RFC 9659 section 3: an HTTP encoder "MUST NOT generate frames requiring a
/// `Window_Size` larger than 8 MB". On an input of unknown size, which both
/// paths hand it, zstd's own table gives levels 20 to 22 a 32 to 128 MB
/// window, so theirs is capped at 2^23. Levels up to 19 already stay within
/// it and keep the smaller window they choose: capping every level at 2^23
/// would raise the default's 2 MB fourfold, per concurrent encode.
fn zstd_window(level: ZstdLevel) -> Option<CParameter> {
    /// The highest level whose own window fits in 8 MB.
    const WITHIN_WINDOW: i32 = 19;
    /// 8 MB, as the base-2 logarithm zstd takes.
    const HTTP_WINDOW_LOG: u32 = 23;

    (level.get() > WITHIN_WINDOW).then(|| CParameter::window_log(HTTP_WINDOW_LOG))
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

    /// 406, for a request that refused identity, zstd, br and gzip alike, or
    /// a required encoding none of them can apply.
    type Short = NotAcceptable<T>;

    // Negotiates before building the future, so the future holds the rest of
    // the chain rather than the request.
    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<ContentEncoding>, NotAcceptable<T>>> + Send {
        let _ = (reads, context);

        let negotiated = negotiate(request.headers());

        // Refused before the chain runs: nothing it produced could be sent.
        let run = (negotiated != Negotiated::Nothing).then(|| next.run(request));

        async move {
            let Some(run) = run else {
                return Err(NotAcceptable::new());
            };

            self.settle(run.await, negotiated).await
        }
    }
}

impl<T: ProblemType> Compression<T> {
    /// Decides whether to encode what the chain produced, and encodes it.
    async fn settle(
        &self,
        mut continued: Continued,
        negotiated: Negotiated,
    ) -> Result<Continued<ContentEncoding>, NotAcceptable<T>> {
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
            // Rule 1 leaves the choice to the server, which keeps identity
            // unless the response excludes it.
            Negotiated::Unstated if !refused && policy == Encoding::Required => Coding::ALL[0],
            // `Required` excludes identity, so nothing is left. `leave_alone`
            // reaches here deliberately: its demands conflict, and 406 says so.
            _ if policy == Encoding::Required => return Err(NotAcceptable::new()),
            _ => return Ok(continued.with_headers(ContentEncoding::default())),
        };

        let mut body = continued.take_body();
        let length = body.exact_length();

        // An unknown length streams regardless of `min_size`; `Required`
        // outranks `min_size` but not an empty body.
        let worth_encoding = match length {
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

        if length.is_none() {
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
