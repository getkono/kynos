//! The fixture the instruction-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy) profiles:
//! one service per shape of request, and one request per scenario.
//!
//! The scenarios are [`kynos-bench`](https://github.com/getkono/kynos-bench)'s
//! catalog, taken through `Service::call` in process rather than over a
//! socket. The shapes match so that a figure read here and one read there
//! describe the same request, and a gap between them is the transport's: this
//! harness counts what Kynos does between a parsed request and a drained
//! response, and the wire plane's numbers are everything else. How a request
//! is driven, and what the region is, is [`driver`]'s. The exception is
//! [`connection`]'s rows, which send a catalog request through the server's
//! connection path over an in-memory duplex, so that the server's share of
//! that gap is counted too.
//!
//! A scenario may name a [`base`](Scenario::base): the scenario it differs
//! from in one variable, whose difference the report gives as that variable's
//! marginal cost.
//!
//! The ablation tables — `ROUTING` through `REALISTIC` — are that, over the
//! whole per-request surface: each row adds, moves or swaps one element of
//! a request's path against a catalog row, so its difference is what that
//! element costs. Every one is served by the catalog's route table or that
//! table with one thing changed; `src/app/mod.rs` says why.
//! Where the element is an interceptor, the request carries what that
//! interceptor acts on — an `Origin` for CORS, an `Accept-Encoding` for
//! compression — since a row that never reaches its interceptor's work would
//! count the interceptor declining rather than the interceptor.
//!
//! `CALIBRATION` is five more, and they are the shapes and stacks
//! [`tests/alloc.rs`](../../kynos/tests/alloc.rs) already counts with
//! `alloc_counter`. They are here so that the two instruments are read over
//! the same request: DHAT's block count for each is held to the count that
//! target records, which is what says this harness measures the region the
//! other one does rather than a neighbouring one.
//!
//! A library rather than code inside the benchmark, because the benchmark only
//! runs under Valgrind. Everything here is also exercised by an ordinary test,
//! which is what keeps a fixture that answers the wrong status from reaching a
//! profile nobody reads until the trend moves.

use kynos::{
    http::{Method, Request, StatusCode, body::Body},
    openapi::model::body::mime_names,
};

use crate::{
    app::{
        codecs, echo_body, extract, get, get_encoded, headers_request, interceptors, post,
        realistic, request, respond, routing, scope,
        scope::{layers_4, layers_8},
        service,
        sweep::compressed,
        with,
    },
    driver::{Driver, Served},
};

mod app;
pub mod connection;
pub mod driver;

#[cfg(test)]
mod tests;

/// One request to profile: the service it goes to, how it is built, and the
/// status it must be answered with for the count to be of what its name says.
///
/// The status is part of the row for
/// [`alloc_codecs.rs`](../../kynos/tests/alloc_codecs.rs)'s reason: a request
/// refused before it reaches what is being measured — a
/// 415 where a decode was meant, a 404 where a match was — is a count of the
/// refusal, and it is cheaper than the thing it stands in for.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// The name a benchmark and a report refer to it by.
    pub name: &'static str,
    /// Builds the service the request is sent to, of whatever context.
    pub service: fn() -> Box<dyn Served>,
    /// Builds the request, always outside the measured region.
    pub request: fn() -> Request,
    /// The status the response must carry.
    pub expected: StatusCode,
    /// How the request is driven.
    pub driver: Driver,
    /// The scenario this one differs from in exactly one variable, so that
    /// the difference between their counts is that variable's marginal cost.
    ///
    /// `scripts/profile_report.py` reports the difference beside the counts.
    /// `None` where no other scenario differs in one variable alone.
    pub base: Option<&'static str>,
}

/// kynos-bench's catalog, as far as it can be driven without a socket.
///
/// `stream-sse` and `stream-ndjson` are [`STREAM`]'s `sse-16` and `ndjson-16`;
/// `idle-conns` is absent, since an idle connection is
/// not a request. `ops-*` measures document generation, which
/// [`kynos-openapi/tests/alloc.rs`](../../kynos-openapi/tests/alloc.rs) counts.
/// `not-found`, `method-not-allowed` and `rejection` are not in the catalog;
/// they are the three error paths a request reaches without a handler.
pub(crate) const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "plaintext",
        service,
        request: || get("/plaintext"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "json-small",
        service,
        request: || get("/json/small"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "json-large",
        service,
        request: || get("/json/large"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "echo-post",
        service,
        request: || post("/echo", echo_body()),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "path-params",
        service,
        request: || get("/items/7/11/13?after=5&per_page=20"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "headers",
        service,
        request: headers_request,
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "layers-0",
        service,
        request: || get("/json/small"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "layers-4",
        service: layers_4,
        request: || get("/json/small"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: Some("layers-0"),
    },
    Scenario {
        name: "layers-8",
        service: layers_8,
        request: || get("/json/small"),
        expected: StatusCode::OK,
        driver: Driver::Poll,
        base: Some("layers-4"),
    },
    Scenario {
        name: "not-found",
        service,
        request: || get("/nope"),
        expected: StatusCode::NOT_FOUND,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "method-not-allowed",
        service,
        // POST, which `/echo` implements: a method no operation implements is
        // a 501, not the 405 this scenario measures.
        request: || request(Method::POST, "/plaintext", None, Body::empty()),
        expected: StatusCode::METHOD_NOT_ALLOWED,
        driver: Driver::Poll,
        // The same target and route, asked with a method it does not have.
        base: Some("plaintext"),
    },
    Scenario {
        name: "rejection",
        service,
        request: codecs::rejection_request,
        expected: StatusCode::BAD_REQUEST,
        driver: Driver::Poll,
        // The same operation, sent a body that does not parse.
        base: Some("echo-post"),
    },
];

/// The shapes [`tests/alloc.rs`](../../kynos/tests/alloc.rs) records, over the
/// same handlers and stacks, so that DHAT and `alloc_counter` read the same
/// request.
///
/// `scripts/profile_report.py` pairs each with its row by request target or
/// stack depth, and reads the counts from those tables rather than repeating
/// them here.
pub(crate) const CALIBRATION: &[Scenario] = &[
    Scenario {
        name: "calibration-static",
        service,
        request: || get("/ping"),
        expected: StatusCode::NO_CONTENT,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "calibration-capture",
        service,
        request: || get("/users/7"),
        expected: StatusCode::NO_CONTENT,
        driver: Driver::Poll,
        base: None,
    },
    Scenario {
        name: "calibration-miss",
        service,
        request: || get("/nope"),
        expected: StatusCode::NOT_FOUND,
        driver: Driver::Poll,
        base: None,
    },
    // Then that file's `STACKS` rows past depth 0: the static match behind
    // four and eight transparent layers. These are the rows that caught DHAT
    // dropping allocations whose backtrace no longer reached the benchmark
    // frame, which no shallow shape could have.
    Scenario {
        name: "calibration-stacked-4",
        service: layers_4,
        request: || get("/ping"),
        expected: StatusCode::NO_CONTENT,
        driver: Driver::Poll,
        base: Some("calibration-static"),
    },
    Scenario {
        name: "calibration-stacked-8",
        service: layers_8,
        request: || get("/ping"),
        expected: StatusCode::NO_CONTENT,
        driver: Driver::Poll,
        base: Some("calibration-stacked-4"),
    },
];

/// One sweep row: `$name` asks `/json/$size` for `$coding`, and a coded row
/// names the identity row at its size as its base.
macro_rules! sweep {
    ($name:literal, $size:literal, $coding:literal) => {
        sweep!($name, $size, $coding, None)
    };
    ($name:literal, $size:literal, $coding:literal, $base:literal) => {
        sweep!($name, $size, $coding, Some($base))
    };
    ($name:literal, $size:literal, $coding:literal, $base:expr) => {
        Scenario {
            name: $name,
            service: compressed,
            request: || get_encoded(concat!("/json/", $size), $coding),
            expected: StatusCode::OK,
            driver: Driver::Poll,
            base: $base,
        }
    };
}

/// The compression sweep: one JSON document at five sizes, each encoded by
/// each coding `Compression` ships, and the two ends passed through.
///
/// Not kynos-bench's: what encoding costs against what it saves is a question
/// only a framework with this interceptor asks, which is `performance.md`'s
/// boundary. The rows are what `Compression::new()`'s default `min_size` is
/// read from — the instructions an encode costs here, and the octets it saves,
/// which this crate's tests hold to the rule — and `middleware.md` records the
/// reading.
///
/// The `identity` rows are the interceptor declining, one per size, so the
/// difference to a coded row at the same size is the encode alone.
pub(crate) const COMPRESSION: &[Scenario] = &[
    sweep!("compressed-identity-small", "small", "identity"),
    sweep!("compressed-identity-1k", "1k", "identity"),
    sweep!("compressed-identity-2k", "2k", "identity"),
    sweep!("compressed-identity-4k", "4k", "identity"),
    sweep!("compressed-identity-large", "large", "identity"),
    sweep!(
        "compressed-gzip-small",
        "small",
        "gzip",
        "compressed-identity-small"
    ),
    sweep!("compressed-gzip-1k", "1k", "gzip", "compressed-identity-1k"),
    sweep!("compressed-gzip-2k", "2k", "gzip", "compressed-identity-2k"),
    sweep!("compressed-gzip-4k", "4k", "gzip", "compressed-identity-4k"),
    sweep!(
        "compressed-gzip-large",
        "large",
        "gzip",
        "compressed-identity-large"
    ),
    sweep!(
        "compressed-br-small",
        "small",
        "br",
        "compressed-identity-small"
    ),
    sweep!("compressed-br-1k", "1k", "br", "compressed-identity-1k"),
    sweep!("compressed-br-2k", "2k", "br", "compressed-identity-2k"),
    sweep!("compressed-br-4k", "4k", "br", "compressed-identity-4k"),
    sweep!(
        "compressed-br-large",
        "large",
        "br",
        "compressed-identity-large"
    ),
    sweep!(
        "compressed-zstd-small",
        "small",
        "zstd",
        "compressed-identity-small"
    ),
    sweep!("compressed-zstd-1k", "1k", "zstd", "compressed-identity-1k"),
    sweep!("compressed-zstd-2k", "2k", "zstd", "compressed-identity-2k"),
    sweep!("compressed-zstd-4k", "4k", "zstd", "compressed-identity-4k"),
    sweep!(
        "compressed-zstd-large",
        "large",
        "zstd",
        "compressed-identity-large"
    ),
];

/// What driving a request with [`Driver::Runtime`] costs over polling it once:
/// a catalog request under the runtime, based on itself under `Poll`.
///
/// The row a `Runtime` scenario's own base subtracts first, since a scenario
/// that needs the runtime has no `Poll` counterpart to differ from in one
/// variable.
pub(crate) const RUNTIME: &[Scenario] = &[Scenario {
    name: "json-small-runtime",
    service,
    request: || get("/json/small"),
    expected: StatusCode::OK,
    driver: Driver::Runtime,
    base: Some("json-small"),
}];

/// One ablation row: `$name`, served by `$service` and sent `$request`,
/// answered `$expected` under `$driver`, and differing from `$base`.
macro_rules! row {
    ($name:literal, $service:expr, $request:expr, $expected:ident, $driver:ident, $base:expr) => {
        Scenario {
            name: $name,
            service: $service,
            request: $request,
            expected: StatusCode::$expected,
            driver: Driver::$driver,
            base: $base,
        }
    };
}

/// Routing: a match past the captures held inline, and each answer the router
/// gives where no operation does.
///
/// `redirect-slash` is served by the table under `TrailingSlashPolicy::Redirect`,
/// which a matched request never reads: its base answers `/plaintext` under
/// the default as it would under that policy.
pub(crate) const ROUTING: &[Scenario] = &[
    row!(
        "captures-4",
        service,
        || get("/items/7/11/13/17?after=5&per_page=20"),
        OK,
        Poll,
        Some("path-params")
    ),
    // `PATCH`, which no operation implements: decided before the path is.
    row!(
        "not-implemented",
        service,
        || request(Method::PATCH, "/plaintext", None, Body::empty()),
        NOT_IMPLEMENTED,
        Poll,
        Some("method-not-allowed")
    ),
    row!(
        "redirect-slash",
        routing::redirecting,
        || get("/plaintext/"),
        PERMANENT_REDIRECT,
        Poll,
        Some("plaintext")
    ),
    row!(
        "head-get",
        service,
        || request(Method::HEAD, "/plaintext", None, Body::empty()),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "not-found-empty",
        routing::bare_not_found,
        || get("/nope"),
        NOT_FOUND,
        Poll,
        Some("not-found")
    ),
    // The root router's interceptors run around its fallback too.
    row!(
        "not-found-layered-4",
        layers_4,
        || get("/nope"),
        NOT_FOUND,
        Poll,
        Some("not-found")
    ),
];

/// Scope: `layers-4`'s layers at the two narrower scopes, panic recovery at
/// the widest and narrowest, and an observer.
pub(crate) const SCOPE: &[Scenario] = &[
    row!(
        "group-layers-4",
        scope::group_layers_4,
        || get("/json/small"),
        OK,
        Poll,
        Some("layers-4")
    ),
    row!(
        "endpoint-layers-4",
        scope::endpoint_layers_4,
        || get("/json/small"),
        OK,
        Poll,
        Some("layers-4")
    ),
    row!(
        "catch-panics-router",
        scope::catching_router,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "catch-panics-endpoint",
        scope::catching_endpoint,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "observer-noop",
        scope::observed,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    // `Trace` with no subscriber installed: every event is built as far as
    // `tracing` builds one nobody is interested in.
    row!(
        "trace-nosub",
        scope::traced,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "trace-sub",
        scope::traced_subscribed,
        || get("/json/small"),
        OK,
        Poll,
        Some("trace-nosub")
    ),
];

/// Extraction: one more extractor than the base reads, or the base's read
/// another way.
///
/// `pattern ← bounds` is not a row: `pattern` holds a string, and `bounds`,
/// being `path-params` with bounds, holds integers, so no `pattern` row
/// differs from it in the pattern alone.
pub(crate) const EXTRACT: &[Scenario] = &[
    row!(
        "inject",
        service,
        || get("/inject"),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "auth-bearer",
        service,
        || with(get("/auth/bearer"), &[("authorization", extract::TOKEN)]),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "auth-basic",
        service,
        || with(get("/auth/basic"), &[("authorization", extract::BASIC)]),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "auth-apikey",
        service,
        || with(get("/auth/api-key"), &[("x-api-key", extract::API_KEY)]),
        OK,
        Poll,
        Some("plaintext")
    ),
    // A well-formed token the authenticator refuses.
    row!(
        "auth-401",
        service,
        || with(
            get("/auth/bearer"),
            &[("authorization", extract::WRONG_TOKEN)]
        ),
        UNAUTHORIZED,
        Poll,
        Some("auth-bearer")
    ),
    row!(
        "cookies",
        service,
        || with(get("/cookies"), &[("cookie", "session=opaque")]),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "bounds",
        service,
        || get("/bounded/7/11/13?after=5&per_page=20"),
        OK,
        Poll,
        Some("path-params")
    ),
    // Eight pairs no field declares, after the two `Page` reads (#237).
    row!(
        "query-unused-8",
        service,
        || get(concat!(
            "/items/7/11/13?after=5&per_page=20",
            "&u0=0&u1=1&u2=2&u3=3&u4=4&u5=5&u6=6&u7=7"
        )),
        OK,
        Poll,
        Some("path-params")
    ),
    // `{"after":5,"per_page":20}`, percent-encoded.
    row!(
        "querystring",
        service,
        || get("/querystring/7/11/13?%7B%22after%22:5,%22per_page%22:20%7D"),
        OK,
        Poll,
        Some("path-params")
    ),
    row!(
        "one-of",
        service,
        || post("/one-of", echo_body()),
        OK,
        Poll,
        Some("echo-post")
    ),
    // `POST`, since a text body is what is read: `plaintext`'s octets in, and
    // back out as its answer.
    row!(
        "text-body",
        service,
        || request(
            Method::POST,
            "/text",
            Some(mime_names::TEXT_PLAIN),
            Body::from_bytes(bytes::Bytes::from_static(b"Hello, World!")),
        ),
        OK,
        Poll,
        Some("plaintext")
    ),
];

/// Codecs: `echo-post`'s document through every other body codec, and each
/// way a body is refused before it is decoded.
pub(crate) const CODECS: &[Scenario] = &[
    row!(
        "form-echo",
        service,
        codecs::form_request,
        OK,
        Poll,
        Some("echo-post")
    ),
    row!(
        "multipart-echo",
        service,
        codecs::multipart_request,
        OK,
        Poll,
        Some("echo-post")
    ),
    row!(
        "protobuf-echo",
        service,
        codecs::protobuf_request,
        OK,
        Poll,
        Some("echo-post")
    ),
    row!(
        "reject-415",
        service,
        codecs::reject_415_request,
        UNSUPPORTED_MEDIA_TYPE,
        Poll,
        Some("rejection")
    ),
    row!(
        "reject-413",
        service,
        codecs::reject_413_request,
        PAYLOAD_TOO_LARGE,
        Poll,
        Some("rejection")
    ),
    row!(
        "reject-422",
        service,
        codecs::reject_422_request,
        UNPROCESSABLE_ENTITY,
        Poll,
        Some("rejection")
    ),
];

/// Responses: `plaintext`'s octets as a representation answering ranges and
/// preconditions, and `json-small`'s negotiated or created.
pub(crate) const RESPOND: &[Scenario] = &[
    row!(
        "binary-full",
        service,
        || get("/binary"),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "ranged",
        service,
        || with(get("/binary"), &[("range", "bytes=0-4")]),
        PARTIAL_CONTENT,
        Poll,
        Some("binary-full")
    ),
    row!(
        "conditional-304",
        service,
        || with(get("/binary"), &[("if-none-match", respond::BINARY_TAG)]),
        NOT_MODIFIED,
        Poll,
        Some("binary-full")
    ),
    row!(
        "negotiated",
        service,
        || with(get("/json/negotiated"), &[("accept", "application/json")]),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "created",
        service,
        || get("/json/created"),
        CREATED,
        Poll,
        Some("json-small")
    ),
];

/// `json-small`'s request, from [`interceptors::ORIGIN`].
fn from_origin() -> Request {
    with(get("/json/small"), &[("origin", interceptors::ORIGIN)])
}

/// Interceptors: the table with one mounted at router scope, sent
/// `json-small`'s request, or the base's where another is named.
///
/// `timeout` and `body-timeout` arm a timer and so need the runtime; their
/// base is `json-small` under it.
pub(crate) const INTERCEPTORS: &[Scenario] = &[
    row!(
        "cors-simple",
        interceptors::cors,
        from_origin,
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "cors-preflight",
        interceptors::cors,
        || with(
            request(Method::OPTIONS, "/json/small", None, Body::empty()),
            &[
                ("origin", interceptors::ORIGIN),
                ("access-control-request-method", "GET"),
            ]
        ),
        NO_CONTENT,
        Poll,
        Some("json-small")
    ),
    row!(
        "csrf",
        interceptors::csrf,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "request-id-counter",
        interceptors::request_id_counter,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "request-id-random",
        interceptors::request_id_random,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "security-headers",
        interceptors::security_headers,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "body-size",
        interceptors::body_size,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "concurrency",
        interceptors::concurrency,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "timeout",
        interceptors::timeout,
        || get("/json/small"),
        OK,
        Runtime,
        Some("json-small-runtime")
    ),
    row!(
        "body-timeout",
        interceptors::body_timeout,
        || get("/json/small"),
        OK,
        Runtime,
        Some("json-small-runtime")
    ),
    row!(
        "decompress-gzip",
        interceptors::decompression,
        interceptors::gzipped_echo,
        OK,
        Poll,
        Some("echo-post")
    ),
    row!(
        "rate-limit",
        interceptors::rate_limit,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "cache-miss",
        interceptors::cache_miss,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "cache-hit",
        interceptors::cache_hit,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "set-cookies",
        interceptors::set_cookies,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
    // Offered gzip, and declining it: `json-small` is under the default
    // `min_size`, which is what this row measures applying.
    row!(
        "compression-default",
        interceptors::compression,
        || get_encoded("/json/small", "gzip"),
        OK,
        Poll,
        Some("json-small")
    ),
    row!(
        "unchecked-layer",
        interceptors::unchecked_layer,
        || get("/json/small"),
        OK,
        Poll,
        Some("json-small")
    ),
];

/// Streams: sixteen of `json-small`'s documents, or of `plaintext`'s octets,
/// from an iterator, the body drained in the region.
///
/// No row but the compressed one names a base: none differs from a catalog
/// row in its framing alone, and `sse-keepalive` needs the runtime `sse-16`
/// is not driven under.
pub(crate) const STREAM: &[Scenario] = &[
    row!("sse-16", service, || get("/stream/sse"), OK, Poll, None),
    row!(
        "sse-keepalive",
        service,
        || get("/stream/sse/keep-alive"),
        OK,
        Runtime,
        None
    ),
    row!(
        "ndjson-16",
        service,
        || get("/stream/ndjson"),
        OK,
        Poll,
        None
    ),
    row!(
        "json-seq-16",
        service,
        || get("/stream/json-seq"),
        OK,
        Poll,
        None
    ),
    row!(
        "binary-stream-16",
        service,
        || get("/stream/binary"),
        OK,
        Poll,
        None
    ),
    row!(
        "compressed-stream-gzip",
        interceptors::compression,
        || get_encoded("/stream/ndjson", "gzip"),
        OK,
        Poll,
        Some("ndjson-16")
    ),
];

/// Static: `plaintext`'s octets as an embedded asset, and the description.
pub(crate) const STATIC: &[Scenario] = &[
    row!(
        "assets-hit",
        service,
        || get("/static/plaintext.txt"),
        OK,
        Poll,
        Some("plaintext")
    ),
    row!(
        "docs-json",
        service,
        || get("/openapi.json"),
        OK,
        Poll,
        Some("plaintext")
    ),
];

/// The composite: `json-small` behind the stack a production API typically
/// mounts, with the request that stack acts on. Its `Timeout` needs the
/// runtime, so its base is `json-small` under it.
pub(crate) const REALISTIC: &[Scenario] = &[row!(
    "stack-typical",
    realistic::stack,
    || with(
        get("/json/small"),
        &[
            ("origin", interceptors::ORIGIN),
            ("authorization", extract::TOKEN),
            ("accept-encoding", "gzip, br, zstd"),
        ]
    ),
    OK,
    Runtime,
    Some("json-small-runtime")
)];

/// Looks a scenario up by name, from any table.
///
/// # Panics
///
/// When no scenario has that name, which is a benchmark naming one that was
/// removed.
#[must_use]
pub fn scenario(name: &str) -> Scenario {
    catalog()
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("no scenario is named {name:?}"))
}

/// Every scenario, from every table, in declaration order.
pub fn catalog() -> impl Iterator<Item = Scenario> {
    SCENARIOS
        .iter()
        .chain(CALIBRATION)
        .chain(COMPRESSION)
        .chain(RUNTIME)
        .chain(ROUTING)
        .chain(SCOPE)
        .chain(EXTRACT)
        .chain(CODECS)
        .chain(RESPOND)
        .chain(INTERCEPTORS)
        .chain(STREAM)
        .chain(STATIC)
        .chain(REALISTIC)
        .copied()
        .chain(
            connection::CONNECTION
                .iter()
                .map(|connected| connected.scenario),
        )
}
