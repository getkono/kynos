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

use kynos::http::{Method, Request, StatusCode, body::Body};

use crate::{
    app::{
        compressed, echo_body, get, get_encoded, headers_request, layers_4, layers_8, post,
        request, service,
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
/// `stream-sse` and `stream-ndjson` are not rows yet, though the region now
/// drains a streamed body; `idle-conns` is absent, since an idle connection is
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
        request: || {
            post(
                "/echo",
                Body::from_bytes(bytes::Bytes::from_static(b"{\"id\":")),
            )
        },
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
        .copied()
        .chain(
            connection::CONNECTION
                .iter()
                .map(|connected| connected.scenario),
        )
}
