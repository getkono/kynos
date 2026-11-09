//! Entry points for the `fuzz/` targets into parsers that have no public path.
//!
//! Compiled only under `cfg(fuzzing)`, which `cargo fuzz` sets. Each function
//! forwards to the crate-private parser it is named after.

use std::time::SystemTime;

use crate::http::{HeaderMap, HeaderValue};

/// The weight a `q` parameter's `value` states, in thousandths.
#[must_use]
pub fn quality(value: &str) -> Option<u16> {
    crate::http::quality::parse(value)
}

/// The instant an HTTP-date names, in any of its three formats.
#[must_use]
pub fn parse_date(value: &str) -> Option<SystemTime> {
    crate::http::date::parse(value)
}

/// `time` rendered as an IMF-fixdate.
#[must_use]
pub fn format_date(time: SystemTime) -> Option<String> {
    crate::http::date::format(time)
}

/// The members of an entity-tag list, in the order `text` wrote them.
#[must_use]
pub fn etags(text: &str) -> Vec<&str> {
    crate::http::etag::split(text).collect()
}

/// Whether an `If-None-Match` `field` names `current`.
#[must_use]
pub fn etag_matches(field: &HeaderValue, current: &str) -> bool {
    crate::http::etag::matches(field, current)
}

/// Whether an `If-Match` `field` holds for a representation tagged `current`.
#[must_use]
pub fn etag_matches_strongly(field: &HeaderValue, current: Option<&str>) -> bool {
    crate::http::etag::matches_strongly(field, current)
}

/// The form-encoded pairs of `query`, percent-decoded, as `QueryParams` and
/// query API keys read them.
pub fn query_pairs(
    query: Option<&str>,
) -> impl Iterator<Item = (std::borrow::Cow<'_, [u8]>, std::borrow::Cow<'_, [u8]>)> {
    crate::__private::uri::query_pairs(query)
}

/// Whether a `Form<T>` body `text` percent-decodes to UTF-8, checked in place.
///
/// # Errors
///
/// The offset into `text` of the escape that began the first sequence that is
/// not UTF-8.
#[cfg(feature = "form")]
pub fn percent_decoded_utf8(text: &str) -> Result<(), usize> {
    crate::__private::uri::check_percent_decoded_utf8(text)
}

/// The weight an `Accept-Encoding` field `accept` gives the coding `token`,
/// its own or else `*`'s, in thousandths; `None` where neither appears.
#[cfg(any(feature = "compression", feature = "assets"))]
#[must_use]
pub fn coding_quality(accept: &str, token: &str) -> Option<u16> {
    crate::http::coding::quality(accept, token)
}

/// The coding of `available` an `Accept-Encoding` field `accept` prefers to
/// identity, or `None` to send identity.
#[cfg(any(feature = "compression", feature = "assets"))]
#[must_use]
pub fn preferred_coding<'a>(accept: &str, available: &[&'a str]) -> Option<&'a str> {
    crate::http::coding::preferred(accept, available)
}

/// The weight an `Accept-Encoding` field `accept` gives identity, in
/// thousandths.
#[cfg(any(feature = "compression", feature = "assets"))]
#[must_use]
pub fn identity_quality(accept: &str) -> u16 {
    crate::http::coding::identity_quality(accept)
}

/// What a `Cors` permitting `origins` and every origin ending in `suffix`,
/// with credentials where `credentials` says, and every request header where
/// `any_header` does or else `x-trace`, answers a request carrying `headers`.
///
/// The first map is the CORS fields of an actual response. The status and the
/// second map are the answer to an `OPTIONS` carrying `headers`, on a path
/// serving `GET` and `POST` under that `Cors` and `DELETE` under none.
#[must_use]
pub fn cors(
    origins: &[&str],
    suffix: &str,
    credentials: bool,
    any_header: bool,
    headers: &HeaderMap,
) -> (HeaderMap, crate::http::StatusCode, HeaderMap) {
    use kynos_openapi::Method;

    use crate::{
        extract::params::header::EncodeHeaders,
        http::{Request, body::Body},
        middleware::cors::{
            Cors,
            preflight::{Preflight, Scope},
        },
        router::policy::FallbackPolicy,
    };

    let suffix = suffix.to_owned();
    let mut cors = Cors::new()
        .allow_origins(origins.iter().map(|origin| (*origin).to_owned()))
        .allow_origins_matching(move |origin| origin.ends_with(&suffix));
    if credentials {
        cors = cors.allow_credentials();
    }
    cors = if any_header {
        cors.allow_any_header()
    } else {
        cors.allow_headers(["x-trace"])
    };
    let config = cors.config();

    let actual = config
        .headers_for(headers)
        .encode()
        .expect("CORS fields are field values")
        .into_iter()
        .collect();

    let preflight = Preflight::new(
        vec![Scope::new(config.clone(), vec![Method::Get, Method::Post])],
        vec![Method::Get, Method::Post, Method::Delete],
        Some(HeaderValue::from_static("GET, POST, DELETE")),
        FallbackPolicy::Problem,
    );
    let mut request = Request::new(Body::empty());
    *request.method_mut() = crate::http::Method::OPTIONS;
    *request.headers_mut() = headers.clone();
    let answer = preflight.answer(&request);

    (actual, answer.status(), answer.headers().clone())
}

/// Whether a `TrustedHosts` allowing `allowed`, and loopback where `loopback`
/// says, admits a request carrying `headers` whose target states `authority`;
/// `None` where `authority` is not one.
#[must_use]
pub fn trusted_host(
    allowed: &[&str],
    loopback: bool,
    headers: &HeaderMap,
    authority: Option<&str>,
) -> Option<bool> {
    use ::http::uri::Authority;

    use crate::middleware::trusted_hosts::TrustedHosts;

    let authority = match authority {
        Some(authority) => Some(Authority::try_from(authority).ok()?),
        None => None,
    };
    let mut hosts = allowed.iter().fold(TrustedHosts::new(), |hosts, host| {
        hosts.allow((*host).to_owned())
    });
    if loopback {
        hosts = hosts.allow_loopback();
    }
    Some(hosts.admits(headers, authority.as_ref()))
}

/// Whether a request carrying `headers` forbids `Cache` from answering it
/// from the store, and from storing its response: its `Cache-Control`
/// `no-cache` and `no-store`.
#[cfg(feature = "cache")]
#[must_use]
pub fn request_cache_control(headers: &HeaderMap) -> (bool, bool) {
    let requested = crate::middleware::cache::freshness::Requested::of(headers);
    (requested.forbids_reuse(), requested.forbids_storing())
}

/// The delimiter a multipart request carrying `headers` declares, or the
/// status refusing it: 415 for a media type `MultipartForm` does not accept,
/// 400 for no `boundary`.
#[cfg(feature = "multipart")]
pub fn multipart_boundary(headers: &HeaderMap) -> Result<String, u16> {
    crate::extract::body::multipart::boundary(headers).map_err(|refused| refused.status().as_u16())
}

/// What a `QueryString<Unchecked<serde_json::Value>, Json>` reads from a
/// request whose target has the query `query`, or none at all; `None` where
/// `/?query` is not a request target.
#[cfg(all(feature = "json", feature = "openapi32"))]
#[must_use]
pub fn querystring(
    query: Option<&str>,
) -> Option<Result<serde_json::Value, crate::error::rejection::QueryRejection>> {
    use crate::{
        extract::{FromRequestParts, params::querystring::QueryString},
        http::{Request, Uri, body::Body, media::Json},
        schema::unchecked::Unchecked,
    };

    let target = match query {
        Some(query) => format!("/?{query}"),
        None => "/".to_owned(),
    };
    let mut request = Request::new(Body::empty());
    *request.uri_mut() = Uri::try_from(target).ok()?;
    let (mut parts, _) = request.into_parts();
    let read = ready(
        <QueryString<Unchecked<serde_json::Value>, Json> as FromRequestParts<()>>::from_request_parts(
            &mut parts,
            &(),
        ),
    );
    Some(read.map(|read| read.into_inner().0))
}

/// `bytes` read as one JSON document by the reader every JSON input shares,
/// which reads a struct only from an object.
///
/// # Errors
///
/// Whether the document was well-formed JSON that did not fit `T`, and the
/// JSON Pointer of the position that failed.
#[cfg(any(feature = "json", feature = "openapi32"))]
pub fn strict_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, (bool, String)> {
    crate::extract::strict_json::from_slice(bytes).map_err(|failure| {
        (
            failure.is_schema_failure(),
            failure.pointer(crate::schema::constraints::Pointer::root()),
        )
    })
}

/// Whether a request carrying `headers` offers a body codec's `media_type`.
#[must_use]
pub fn offers(headers: &HeaderMap, media_type: &str) -> bool {
    crate::extract::body::offers(headers, media_type)
}

/// The `range-spec`s a `Range` field `value` asks for, as written, or why it
/// is ignored.
///
/// An `int-range` is `(Some(first), last)` and a `suffix-range` is
/// `(None, Some(length))`.
///
/// # Errors
///
/// The reason the reader ignores the field for.
pub fn range_specs(
    value: &str,
) -> Result<Vec<(Option<u64>, Option<u64>)>, crate::response::range::spec::Ignored> {
    use crate::response::range::spec::Spec;

    let specs = crate::response::range::spec::parse(value)?;
    Ok(specs
        .into_iter()
        .map(|spec| match spec {
            Spec::Offsets { first, last } => (Some(first), last),
            Spec::Suffix { length } => (None, Some(length)),
        })
        .collect())
}

/// Whether the `const` check refusing a `OneOf` pair finds that a `Binary`
/// `marker` and a codec's parameterless `body` type share a request.
#[must_use]
pub fn shares_requests(marker: &str, body: &str) -> bool {
    crate::extract::body::alternative::shares_requests(marker, body)
}

/// What `Decompression::new(limit)`, with `max_ratio` where one is given,
/// hands on for a body that arrived whole as `arrived` under the
/// `Content-Encoding` fields `headers` carries.
///
/// `arrived` is no longer than `limit`: a longer body is refused while it is
/// still being read, before any decoding.
///
/// # Errors
///
/// Why the layer refuses the body.
#[cfg(feature = "compression")]
pub fn decompress(
    headers: &HeaderMap,
    arrived: &[u8],
    limit: u64,
    max_ratio: Option<u64>,
) -> Result<Vec<u8>, crate::middleware::decompression::Reason> {
    use crate::middleware::decompression::{Decompression, Reason, declared};

    let codings = declared(headers).ok_or(Reason::UnsupportedCoding)?;
    let layer = Decompression::new(limit);
    let layer = match max_ratio {
        Some(ratio) => layer.max_ratio(ratio),
        None => layer,
    };
    let arrived = bytes::Bytes::copy_from_slice(arrived);
    ready(layer.undo(&codings, arrived)).map(|decoded| decoded.to_vec())
}

/// What a `JsonLines` body, or with `sequence` a `JsonSeq` one, reads as
/// when it arrives split into `chunks` under a body limit of `limit`: every
/// record in order, up to and including the failure that ends the stream.
#[cfg(all(feature = "json", feature = "openapi32"))]
#[must_use]
pub fn json_records(
    sequence: bool,
    chunks: &[&[u8]],
    limit: u64,
) -> Vec<Result<serde_json::Value, crate::error::rejection::BodyRejection>> {
    use crate::{
        extract::{
            FromRequest,
            body::{
                json_lines::{
                    JsonLines, JsonSeq, LINES_MEDIA_TYPE, SEQUENCE_MEDIA_TYPE, records::Records,
                },
                limit::BodyLimit,
            },
        },
        http::{Request, body::Body, header::CONTENT_TYPE},
        schema::unchecked::Unchecked,
    };

    let chunks = chunks
        .iter()
        .map(|chunk| bytes::Bytes::copy_from_slice(chunk));
    let mut request = Request::new(Body::from_chunks(chunks.collect()));
    let media_type = if sequence {
        SEQUENCE_MEDIA_TYPE
    } else {
        LINES_MEDIA_TYPE
    };
    request
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(media_type));
    request.extensions_mut().insert(BodyLimit(limit));

    type Items = Records<Unchecked<serde_json::Value>>;
    let (mut parts, body) = request.into_parts();
    let mut records = if sequence {
        ready(<JsonSeq<Items> as FromRequest<()>>::from_request(
            &mut parts,
            body,
            &(),
        ))
        .map(|decoded| decoded.items)
    } else {
        ready(<JsonLines<Items> as FromRequest<()>>::from_request(
            &mut parts,
            body,
            &(),
        ))
        .map(|decoded| decoded.items)
    }
    .expect("a body of the codec's own media type");

    let mut read = Vec::new();
    while let Some(record) = ready(records.next()) {
        read.push(record.map(|Unchecked(value)| value));
    }
    read
}

/// Runs `future`, which reads only memory, to its end.
///
/// A codec past `YIELD_ABOVE` octets returns `Pending` after waking itself,
/// so that is polled again; `Pending` with no wake would never end, and
/// panics.
#[cfg(any(feature = "compression", all(feature = "json", feature = "openapi32")))]
fn ready<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct Woken(AtomicBool);

    impl std::task::Wake for Woken {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    let woken = Arc::new(Woken(AtomicBool::new(false)));
    let waker = std::task::Waker::from(Arc::clone(&woken));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => assert!(
                woken.0.swap(false, Ordering::Relaxed),
                "a read of memory waited"
            ),
        }
    }
}

/// The `(name, filename)` a multipart part's `Content-Disposition` `field`
/// declares, or why it declares no field.
#[cfg(feature = "multipart")]
pub fn disposition(field: &str) -> Result<(String, Option<String>), String> {
    crate::extract::body::multipart::disposition::parse(field)
        .map(|disposition| (disposition.name, disposition.file_name))
}

/// `value` as the `MultipartForm` response writer quotes a `Content-Disposition`
/// parameter, without its surrounding quotes.
#[cfg(feature = "multipart")]
#[must_use]
pub fn quoted(value: &str) -> String {
    crate::response::codec::multipart::quoted(value)
}
