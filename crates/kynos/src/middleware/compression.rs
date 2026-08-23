//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use std::{io, pin::Pin, task::Poll};

use async_compression::tokio::bufread::{BrotliEncoder, GzipEncoder, ZstdEncoder};
use bytes::{Bytes, BytesMut};
use http_body::Body as _;
use http_body_util::BodyExt;
use tokio::io::{AsyncRead, ReadBuf};

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
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
}

impl HeaderParams for ContentEncoding {
    const NAMES: &'static [&'static str] = &["content-encoding"];
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

        vec![(
            http::header::CONTENT_ENCODING,
            http::HeaderValue::from_static(coding.token()),
        )]
    }
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

/// Applies `coding` to `bytes`.
async fn encode(coding: Coding, bytes: Bytes) -> io::Result<Bytes> {
    match coding {
        Coding::Zstd => drain(ZstdEncoder::new(io::Cursor::new(bytes))).await,
        // Boxed because brotli's encoder state is measured in kilobytes, and an
        // interceptor's future is held for the whole exchange.
        Coding::Brotli => Box::pin(drain(BrotliEncoder::new(io::Cursor::new(bytes)))).await,
        Coding::Gzip => drain(GzipEncoder::new(io::Cursor::new(bytes))).await,
    }
}

/// Compresses responses when the client accepts it.
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
}

impl Compression {
    /// Enables every compiled-in algorithm.
    #[must_use]
    pub fn new() -> Self {
        Self { min_size: 0 }
    }

    /// Skips responses smaller than `bytes`.
    #[must_use]
    pub fn min_size(mut self, bytes: u64) -> Self {
        self.min_size = bytes;
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
        let coding = match body.collect().await {
            Ok(collected) => {
                let bytes = collected.to_bytes();
                if let Ok(encoded) = encode(coding, bytes.clone()).await {
                    continued.set_body(crate::http::body::Body::from_bytes(encoded));
                    Some(coding)
                } else {
                    continued.set_body(crate::http::body::Body::from_bytes(bytes));
                    None
                }
            }
            Err(_) => None,
        };

        Ok(continued.with_headers(ContentEncoding { coding }))
    }
}

#[cfg(test)]
mod tests {
    use super::{Coding, Negotiated, negotiate, quality};
    use crate::http::{HeaderMap, HeaderValue, header};

    /// A request accepting `value`, or accepting nothing at all.
    fn accepting(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(
                header::ACCEPT_ENCODING,
                HeaderValue::from_str(value).expect("a printable field"),
            );
        }
        headers
    }

    /// Every rule RFC 9110 section 12.5.3 states, and what each resolves to.
    ///
    /// One table, because the rules interact: identity's default acceptability
    /// is what decides two of the rows, and the wildcard's reach decides two
    /// more.
    #[test]
    fn every_negotiation_rule_the_specification_states_is_applied() {
        let cases: &[(&str, Option<&str>, Negotiated)] = &[
            // Rule 1: absent means everything is acceptable.
            ("no field at all", None, Negotiated::Identity),
            // An empty value "implies that the user agent does not want any
            // content coding in response" -- it excludes nothing, so identity.
            ("an empty field value", Some(""), Negotiated::Identity),
            (
                "a plain coding",
                Some("gzip"),
                Negotiated::Encode(Coding::Gzip),
            ),
            (
                "the deprecated spelling of one",
                Some("x-gzip"),
                Negotiated::Encode(Coding::Gzip),
            ),
            (
                "a coding in another case",
                Some("GZIP"),
                Negotiated::Encode(Coding::Gzip),
            ),
            // Server preference breaks a tie: zstd is preferred over gzip.
            (
                "two codings weighted equally",
                Some("gzip, zstd"),
                Negotiated::Encode(Coding::Zstd),
            ),
            // The client's weighting overrides the server's preference.
            (
                "a client preferring the server's second choice",
                Some("gzip;q=1.0, zstd;q=0.5"),
                Negotiated::Encode(Coding::Gzip),
            ),
            // Rule 2, explicit form.
            (
                "identity refused by name",
                Some("gzip, identity;q=0"),
                Negotiated::Encode(Coding::Gzip),
            ),
            // Rule 2, wildcard form -- the one an implementation misses.
            (
                "identity refused through the wildcard",
                Some("gzip, *;q=0"),
                Negotiated::Encode(Coding::Gzip),
            ),
            // A more specific identity entry beats the wildcard.
            (
                "a wildcard refusal with identity readmitted",
                Some("*;q=0, identity"),
                Negotiated::Identity,
            ),
            (
                "every coding refused",
                Some("gzip;q=0, br;q=0, zstd;q=0"),
                Negotiated::Identity,
            ),
            // Nothing left at all: this is the 406.
            ("everything refused", Some("*;q=0"), Negotiated::Nothing),
            (
                "every coding and identity refused by name",
                Some("gzip;q=0, br;q=0, zstd;q=0, identity;q=0"),
                Negotiated::Nothing,
            ),
            // A client preferring identity gets it.
            (
                "identity preferred over a coding",
                Some("gzip;q=0.5, identity;q=1.0"),
                Negotiated::Identity,
            ),
        ];

        for (description, accept, expected) in cases {
            assert_eq!(negotiate(&accepting(*accept)), *expected, "{description}");
        }
    }

    /// A weight above 1 is not a qvalue and must not outrank one.
    ///
    /// RFC 9110 section 12.4.2 bounds it at 1. Read literally, `q=1.5` beats a
    /// legitimate `q=1.0` -- a preference inversion no client can have meant.
    #[test]
    fn a_weight_outside_the_range_cannot_outrank_one_inside_it() {
        assert_eq!(quality("gzip;q=1.5", "gzip"), Some(1.0));
        assert_eq!(quality("gzip;q=-1", "gzip"), Some(0.0));

        // The inversion itself: with clamping, the server's preference decides.
        assert_eq!(
            negotiate(&accepting(Some("gzip;q=1.5, zstd;q=1.0"))),
            Negotiated::Encode(Coding::Zstd)
        );
    }

    /// An unparsable weight is a refusal, which the module already argued for:
    /// a client that wrote something meaningless did not ask for this coding.
    #[test]
    fn an_unparsable_weight_is_a_refusal() {
        assert_eq!(quality("gzip;q=abc", "gzip"), Some(0.0));
    }
}
