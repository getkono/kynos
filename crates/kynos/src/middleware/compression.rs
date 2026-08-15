//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use std::{convert::Infallible, io, pin::Pin, task::Poll};

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
    const NAMES: &'static [&'static str] = &["content-encoding", "vary"];
    const DESCRIBED: bool = false;

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        // `Vary` rides on every response, encoded or not: what the cache has to
        // know is that the answer depends on `Accept-Encoding`, which is true
        // the moment this interceptor is mounted.
        let mut headers = vec![(
            http::header::VARY,
            http::HeaderValue::from_static("accept-encoding"),
        )];

        if let Some(coding) = self.coding {
            headers.push((
                http::header::CONTENT_ENCODING,
                http::HeaderValue::from_static(coding.token()),
            ));
        }

        headers
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
            .map_or(1.0, |weight| weight.trim().parse().unwrap_or(0.0));

        if name.eq_ignore_ascii_case(token) {
            return Some(weight);
        }

        if name == "*" {
            wildcard = Some(weight);
        }
    }

    wildcard
}

/// The coding to apply, per RFC 9110 section 12.5.3.
///
/// `None` leaves the representation as it is, which is always acceptable: an
/// absent field means every coding is acceptable and identity is one of them,
/// and a client that refuses identity outright is answered with it anyway,
/// because 406 is a status this interceptor does not declare and therefore may
/// not send.
fn negotiate(headers: &http::HeaderMap) -> Option<Coding> {
    let accept = headers
        .get(http::header::ACCEPT_ENCODING)?
        .to_str()
        .ok()?
        .trim();

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

    let (coding, weight) = best?;

    // Identity is acceptable unless it was refused, so it only wins when the
    // client asked for it *more* strongly than for anything encoded. A tie goes
    // to the coding, which is what makes plain `Accept-Encoding: gzip` mean
    // what everybody writes it to mean.
    let identity = quality(accept, "identity").unwrap_or(1.0);
    (identity <= weight).then_some(coding)
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

    /// Always continues: compression re-encodes a response, never replaces it.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<ContentEncoding>, Infallible> {
        let _ = (reads, context);

        // Negotiated before the chain runs, because the request is handed on and
        // the answer depends only on what it arrived with.
        let negotiated = negotiate(request.headers());
        let mut continued = next.run(request).await;

        // A response that is already encoded stays as it is: `Content-Encoding`
        // is one header, and something beneath has already spoken for it.
        let encoded_already = continued
            .headers()
            .contains_key(http::header::CONTENT_ENCODING);

        let Some(coding) = negotiated.filter(|_| !encoded_already) else {
            return Ok(continued.with_headers(ContentEncoding::default()));
        };

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
