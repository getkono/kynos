//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use std::{io, pin::Pin, task::Poll};

use async_compression::{
    Level,
    tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder},
};
use bytes::{Bytes, BytesMut};
use http_body::Body as _;
use http_body_util::BodyExt;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{
        Continued, Interceptor, Next,
        compression::levels::{BrotliLevel, GzipLevel, ZstdLevel},
    },
};

/// A content coding this crate can produce.
///
/// Ordered by server preference, which is what breaks a tie between two codings
/// the client weighted equally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Coding {
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
/// `DESCRIBED` is `false`: both headers are defined by HTTP itself and handled
/// by every client without being told. Declaring the names is still what stops
/// a second interceptor touching them -- the check does not care whether a
/// consumer wanted to hear about them.
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

    // `Vary` rides on every response, encoded or not: what the cache has to know
    // is that the answer depends on `Accept-Encoding`, which is true the moment
    // this interceptor is mounted. It is declared here rather than in `NAMES`
    // because it is a set the framework unions -- naming it above would make
    // `Compression` and `Cors` covering one route a compile error, and they are
    // a pairing every browser-facing service wants.
    const VARIES: &'static [&'static str] = &["accept-encoding"];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let Some(coding) = self.coding else {
            return Vec::new();
        };

        let mut fields = vec![(
            http::header::CONTENT_ENCODING,
            http::HeaderValue::from_static(coding.token()),
        )];

        // RFC 9110 section 8.6 counts the octets actually transferred, and
        // section 8.4 defines the representation "in terms of the coded form" --
        // so a length written before encoding describes a body that no longer
        // exists. Section 8.6 is blunt about the consequence: "a sender MUST NOT
        // forward a message with a Content-Length header field value that is
        // known to be incorrect."
        //
        // Restated rather than removed. Removing it would leave hyper to derive
        // one from the body's size hint, which is right today and depends on the
        // body being buffered; stating it is right whatever the body becomes.
        if let Some(length) = self.length {
            if let Ok(value) = http::HeaderValue::from_str(&length.to_string()) {
                fields.push((http::header::CONTENT_LENGTH, value));
            }
        }

        fields
    }
}

/// Whether the response carries a *strong* validator.
///
/// RFC 9110 section 8.8.1 says outright that "if the origin server sends the
/// same validator for a representation with a gzip content coding applied as it
/// does for a representation with no content coding, then that validator is
/// weak". So encoding a strongly tagged response makes the tag a lie: one
/// strong tag naming two representations.
///
/// The encoder cannot correct it from here. The only sanctioned way to write a
/// response header is the `Adds` group, and declaring `etag` there would make
/// `Compression` and
/// [`Cache::deriving_etags`](crate::middleware::cache::Cache::deriving_etags)
/// a compile error on a stack that is otherwise right.
///
/// A *weak* validator may be shared across representations -- that is what weak
/// means -- so it is left alone and the response still compresses.
fn strongly_tagged(headers: &http::HeaderMap) -> bool {
    headers
        .get(http::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tag| !crate::http::etag::is_weak(tag.trim()))
}

/// The deprecated spellings a recipient must treat as `token`.
///
/// RFC 9110 sections 8.4.1.1 and 8.4.1.3: "A recipient SHOULD consider
/// `x-compress` to be equivalent to `compress`" and the same for `x-gzip`.
/// Only `gzip` has one among the codings Kynos ships.
fn aliases(token: &str) -> &'static [&'static str] {
    match token {
        "gzip" => &["x-gzip"],
        _ => &[],
    }
}

/// What a request refusing every available representation is answered with.
///
/// RFC 9110 section 12.4.1: when no available representation is acceptable, the
/// origin server "can either honor the header field by sending a 406 (Not
/// Acceptable) response or disregard the header field". Kynos honours it, since
/// disregarding a `q=0` means sending octets the client said it cannot decode.
///
/// Reachable only through `Accept-Encoding`: it takes refusing identity *and*
/// every coding this build offers, which no ordinary client does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAcceptable;

impl crate::response::IntoResponse for NotAcceptable {
    fn into_response(self) -> http::Response {
        crate::error::problem::Problem::new(http::StatusCode::NOT_ACCEPTABLE)
            .with_detail("no representation of this resource has an acceptable content coding")
            .into_response()
    }
}

impl crate::response::ShortCircuit for NotAcceptable {
    const STATUSES: &'static [u16] = &[406];
}

impl crate::response::Responses for NotAcceptable {
    fn responses(registry: &mut crate::schema::registry::Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            406,
            kynos_openapi::Response::new(
                "no representation has a content coding the request accepts",
            ),
        )
    }
}

/// The quality `accept` assigns `token`, honouring `*`.
///
/// `None` when neither the token nor a wildcard appears, which is what
/// distinguishes "not mentioned" from "mentioned and refused" -- the difference
/// between the two is the whole of `q=0`.
fn quality(accept: &str, token: &str) -> Option<f32> {
    let mut wildcard = None;

    for entry in accept.split(',') {
        let mut parts = entry.split(';');
        let name = parts.next().unwrap_or_default().trim();

        // A malformed weight is a refusal rather than a default: a client that
        // wrote something unparsable did not ask for this coding.
        let weight = parts
            .find_map(|parameter| {
                let parameter = parameter.trim();
                parameter
                    .strip_prefix("q=")
                    .or_else(|| parameter.strip_prefix("Q="))
            })
            .map_or(1.0, |weight| {
                weight
                    .trim()
                    .parse()
                    // RFC 9110 section 12.4.2 bounds a qvalue at 1. A larger
                    // one is not a qvalue, and reading it literally lets
                    // `gzip;q=1.5` outrank a legitimate `q=1.0` -- a preference
                    // inversion a client cannot have meant. Clamped rather than
                    // refused: the client did ask for the coding.
                    .map_or(0.0, |weight: f32| weight.clamp(0.0, 1.0))
            });

        if name.eq_ignore_ascii_case(token)
            || aliases(token)
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
        {
            return Some(weight);
        }

        if name == "*" {
            wildcard = Some(weight);
        }
    }

    wildcard
}

/// What negotiation decided, per RFC 9110 section 12.5.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Negotiated {
    /// Encode with this coding.
    Encode(Coding),
    /// Send the representation as it is. Rule 2: identity "is acceptable by
    /// default unless specifically excluded".
    Identity,
    /// Nothing is acceptable, identity included. Section 12.4.1 leaves the
    /// server to honour the field with a 406 or disregard it; Kynos honours it.
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

    let mut best: Option<(Coding, f32)> = None;
    for coding in Coding::ALL {
        let Some(weight) = quality(accept, coding.token()) else {
            continue;
        };

        if weight <= 0.0 {
            continue;
        }

        if best.is_none_or(|(_, best)| weight > best) {
            best = Some((coding, weight));
        }
    }

    // Rule 2: identity "is acceptable by default unless specifically excluded
    // by the Accept-Encoding header field stating either `identity;q=0` or
    // `*;q=0` without a more specific entry for `identity`". `quality` falls
    // back to the wildcard, so both spellings land here as `Some(0.0)`.
    let identity = quality(accept, "identity").unwrap_or(1.0);

    let Some((coding, weight)) = best else {
        return if identity > 0.0 {
            Negotiated::Identity
        } else {
            // Every coding this build offers was refused *and* identity was
            // refused. An empty field value reaches here too: it "implies that
            // the user agent does not want any content coding in response",
            // which excludes nothing, so it resolves to identity above.
            Negotiated::Nothing
        };
    };

    // Identity only wins when the client asked for it *more* strongly than for
    // anything encoded. A tie goes to the coding, which is what makes plain
    // `Accept-Encoding: gzip` mean what everybody writes it to mean.
    if identity <= weight {
        Negotiated::Encode(coding)
    } else {
        Negotiated::Identity
    }
}

/// Reads an encoder to its end.
///
/// Driven by hand rather than through `AsyncReadExt`, so that compression needs
/// nothing of tokio beyond the `AsyncRead` trait `async-compression` is written
/// against.
async fn drain<R: AsyncRead + Unpin>(mut source: R) -> io::Result<Bytes> {
    let mut encoded = BytesMut::new();
    let mut chunk = [0_u8; 8 * 1024];

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

/// The level as `async-compression` spells one.
///
/// Infallible in practice and written to be infallible in fact: both levels
/// that reach here are bounded at 11 by their own constructors, so the fallback
/// is unreachable rather than a silent clamp.
fn as_level(level: u32) -> i32 {
    i32::try_from(level).unwrap_or(i32::MAX)
}

/// What each algorithm is asked for.
///
/// One value rather than three arguments, so a level cannot be passed to the
/// wrong encoder: the types already make that impossible, and this keeps the
/// call sites from having to say so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Levels {
    /// What gzip is asked for.
    pub gzip: GzipLevel,
    /// What brotli is asked for.
    pub brotli: BrotliLevel,
    /// What zstd is asked for.
    pub zstd: ZstdLevel,
}

/// Compresses responses when the client accepts it.
///
/// ```no_run
/// # #[cfg(feature = "compression")]
/// # {
/// use kynos::middleware::compression::{Compression, levels::GzipLevel};
///
/// // Everything at its default level, except gzip, which this service serves
/// // enough of to care about the CPU.
/// let compression = Compression::new()
///     .min_size(1_024)
///     .gzip_level(GzipLevel::FASTEST);
/// # }
/// ```
///
/// # Levels, and the scope they are set at
///
/// Each algorithm keeps its own type — [`GzipLevel`], [`BrotliLevel`],
/// [`ZstdLevel`] — rather than sharing one `Fastest`/`Best` scale. The three
/// number their levels differently and put the knee of the curve in a different
/// place, so a shared scale would hide the one fact an operator is choosing
/// between. Each refuses a number its own format does not define, and none
/// converts into another.
///
/// The defaults are gzip 6, brotli **4** and zstd 3. Brotli's is the one worth
/// knowing: its reference encoder defaults to 11, which is meant for content
/// compressed once and served a million times, and applied per request costs
/// roughly a fifth of a second of CPU on a 200 KB document.
/// [`BrotliLevel::DEFAULT`] records why 4 is right for an API.
///
/// Levels are set per mount, and a mount is a scope: a `Compression` on the
/// router covers everything, one on a [`Group`](crate::router::group::Group)
/// covers that group, one on an endpoint covers that endpoint. What is *not*
/// available is a global one plus a per-endpoint override — both would add
/// `Content-Encoding` to the same operation, which
/// [`header_names_disjoint`](crate::middleware::stack) refuses at the mount
/// site. Mount the one that varies and leave the rest uncovered.
///
/// Only a response whose length is already known is encoded. A body that cannot
/// state its length is a stream, and buffering one to compress it would defeat
/// the reason it is a stream.
///
/// # A response that ranges is never encoded
///
/// Anything carrying `Accept-Ranges` is left as it is, as are a 206, a 416 and
/// anything carrying `Content-Range`. RFC 9110 section 14.1.2 calculates a byte
/// range over the *encoded* octets while Kynos calculates one over the identity
/// octets, and section 8.8.1 will not let one strong validator name both forms
/// — so a client resuming a download it began encoded would splice identity
/// bytes onto an encoded prefix and corrupt the file with no error anywhere.
///
/// **This costs real bandwidth**, on exactly the content most worth
/// compressing: an [`AssetSet`](crate::router::assets::AssetSet) advertises
/// ranges on every file it serves, so a stylesheet or a bundle under this
/// interceptor ships uncompressed. Two ways out, both outside the encoder:
///
/// * mount `Compression` on a [`Group`](crate::router::group::Group) that does
///   not cover the asset set, so the API is encoded and the files are ranged;
/// * let a reverse proxy or CDN encode them, which is sound only because it
///   owns the validator it sends as well as the coding.
///
/// Encoding here and re-deriving the validator afterwards is not the third
/// option it looks like: the range and its `ETag` are settled by the handler or
/// the asset server before this interceptor is handed the response.
///
/// # A strong validator survives encoding, and should not
///
/// The guard above reads `Content-Encoding`, `Accept-Ranges`, `Content-Range`
/// and the 206/416 statuses. It does not read `ETag`. So a strongly tagged 200
/// that advertises no ranges *is* encoded, and keeps the validator minted over
/// its identity octets — one strong validator naming two representations,
/// against RFC 9110 section 8.8.1. A client can then validate the encoded body
/// with `If-None-Match`, be answered 304 — which replays `ETag` and `Vary` and
/// not `Content-Encoding` — and reuse those octets as the identity form.
///
/// A [`Cache`](crate::middleware::cache::Cache) mounted inside this is the
/// arrangement that makes it easy to hit, but no cache is needed: a handler
/// setting its own `ETag` reaches it identically. Filed as
/// [#29](https://github.com/getkono/kynos/issues/29), which carries both
/// reproductions and what each candidate fix costs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compression {
    /// The smallest response worth encoding, in bytes.
    min_size: u64,
    /// What each algorithm is asked for.
    levels: Levels,
}

impl Compression {
    /// Enables every compiled-in algorithm, at each one's default level.
    #[must_use]
    pub fn new() -> Self {
        Self {
            min_size: 0,
            levels: Levels::default(),
        }
    }

    /// Skips responses smaller than `bytes`.
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

    /// Sets all three at once.
    #[must_use]
    pub fn levels(mut self, levels: Levels) -> Self {
        self.levels = levels;
        self
    }
}

impl<C: Sync + 'static> Interceptor<C> for Compression {
    type Reads = ();
    type Adds = ContentEncoding;

    /// 406, and only for a request that refused every representation this
    /// build can produce. Compression otherwise re-encodes a response rather
    /// than replacing it.
    type Short = NotAcceptable;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<ContentEncoding>, NotAcceptable> {
        let _ = (reads, context);

        // Negotiated before the chain runs, because the request is handed on and
        // the answer depends only on what it arrived with.
        let negotiated = negotiate(request.headers());

        // Nothing the client will accept, identity included. Section 12.4.1
        // gives two lawful answers -- honour the field with a 406, or disregard
        // it -- and Kynos honours it, because disregarding a `q=0` means
        // sending octets the client said in as many words it cannot decode.
        //
        // Answered before the chain runs. The representation the handler would
        // have produced is one no acceptable coding exists for, so producing it
        // is work whose result cannot be sent.
        if negotiated == Negotiated::Nothing {
            return Err(NotAcceptable);
        }

        let mut continued = next.run(request).await;

        // A response that is already encoded stays as it is: `Content-Encoding`
        // is one header, and something beneath has already spoken for it.
        //
        // So does anything a byte range is calculated against. RFC 9110 section
        // 14.1.2 calculates a range with respect to the *encoded* sequence of
        // bytes when a coding is applied, and Kynos calculates one against the
        // identity octets -- so the two cannot both be true of one resource.
        //
        // A range already taken is the visible half. Encoding a 206 after its
        // `Content-Range` was written makes the field describe octets the body
        // no longer carries, and section 14.4 tells the recipient of an invalid
        // `Content-Range` not to recombine -- which is the corruption a client
        // that does recombine gets. The status is checked *and* the field, so a
        // partial response reaching this from a `layer_unchecked` beneath is
        // caught too.
        //
        // A range still to come is the quiet half, and `Accept-Ranges` is what
        // announces it. Encoding that 200 leaves the sender's validator naming
        // the identity octets while the body is encoded, and section 8.8.1
        // requires a strong validator to change *whenever a change occurs to
        // the representation data that would be observable in the content of a
        // 200 response* -- a server whose representations differ only in
        // metadata "needs to incorporate additional information in the
        // validator to distinguish those representations". One tag over both
        // forms does not, so section 13.1.5's `If-Range` passes where it exists
        // to refuse, section 15.3.7.3 licenses the client to combine, and the
        // 206 it is handed is sliced from the identity file.
        //
        // The alternative is to encode and re-derive the validator over what
        // was sent, and that is not a capability this has: the range and its
        // validator are decided by the handler or the asset server, before this
        // interceptor is ever handed the response. Refusing to encode a
        // range-advertising resource is what makes the two representations
        // never both exist.
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

        let Negotiated::Encode(coding) = negotiated else {
            return Ok(continued.with_headers(ContentEncoding::default()));
        };
        if leave_alone {
            return Ok(continued.with_headers(ContentEncoding::default()));
        }

        let body = continued.take_body();

        // A length this body cannot state is one it is producing as it goes;
        // zero bytes compress to a frame header and nothing else.
        let worth_encoding = body
            .size_hint()
            .exact()
            .is_some_and(|length| length > 0 && length >= self.min_size);

        if !worth_encoding {
            continued.set_body(body);
            return Ok(continued.with_headers(ContentEncoding::default()));
        }

        // Failing to read or to encode leaves the response as the handler
        // produced it, since neither is something this may answer with.
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
            Err(_) => None,
        };

        Ok(continued.with_headers(ContentEncoding {
            coding: encoded.map(|(coding, _)| coding),
            length: encoded.map(|(_, length)| length),
        }))
    }
}

pub mod levels;

#[cfg(test)]
mod tests;
