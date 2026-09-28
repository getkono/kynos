//! The fixture the instruction-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy) profiles:
//! one service per shape of request, and one request per scenario.
//!
//! The scenarios are [`kynos-bench`](https://github.com/getkono/kynos-bench)'s
//! catalog, taken through `Service::call` in process rather than over a
//! socket. The shapes match so that a figure read here and one read there
//! describe the same request, and a gap between them is the transport's: this
//! harness counts what Kynos does between a parsed request and a built
//! response, and the wire plane's numbers are everything else.
//!
//! [`CALIBRATION`] is five more, and they are the shapes and stacks
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

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use kynos::{
    http::{Method, Request, Response, StatusCode, body::Body},
    router::service::Service,
};

use crate::app::{echo_body, get, headers_request, layers_4, layers_8, post, request, service};

pub mod app;

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
    /// Builds the service the request is sent to.
    pub service: fn() -> Service<()>,
    /// Builds the request, always outside the measured region.
    pub request: fn() -> Request,
    /// The status the response must carry.
    pub expected: StatusCode,
}

/// kynos-bench's catalog, as far as it can be driven without a socket.
///
/// `stream-sse`, `stream-ndjson` and `idle-conns` are absent: a stream's cost
/// is spread over polls a single `call` does not make, and an idle connection
/// is not a request. `ops-*` measures document generation, which
/// [`kynos-openapi/tests/alloc.rs`](../../kynos-openapi/tests/alloc.rs) counts.
/// `not-found`, `method-not-allowed` and `rejection` are not in the catalog;
/// they are the three error paths a request reaches without a handler.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "plaintext",
        service,
        request: || get("/plaintext"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "json-small",
        service,
        request: || get("/json/small"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "json-large",
        service,
        request: || get("/json/large"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "echo-post",
        service,
        request: || post("/echo", echo_body()),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "path-params",
        service,
        request: || get("/items/7/11/13?after=5&per_page=20"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "headers",
        service,
        request: headers_request,
        expected: StatusCode::OK,
    },
    Scenario {
        name: "layers-0",
        service,
        request: || get("/json/small"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "layers-4",
        service: layers_4,
        request: || get("/json/small"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "layers-8",
        service: layers_8,
        request: || get("/json/small"),
        expected: StatusCode::OK,
    },
    Scenario {
        name: "not-found",
        service,
        request: || get("/nope"),
        expected: StatusCode::NOT_FOUND,
    },
    Scenario {
        name: "method-not-allowed",
        service,
        // POST, which `/echo` implements: a method no operation implements is
        // a 501, not the 405 this scenario measures.
        request: || request(Method::POST, "/plaintext", None, Body::empty()),
        expected: StatusCode::METHOD_NOT_ALLOWED,
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
    },
];

/// The shapes [`tests/alloc.rs`](../../kynos/tests/alloc.rs) records, over the
/// same handlers and stacks, so that DHAT and `alloc_counter` read the same
/// request.
///
/// `scripts/profile_report.py` pairs each with its row by request target or
/// stack depth, and reads the counts from those tables rather than repeating
/// them here.
pub const CALIBRATION: &[Scenario] = &[
    Scenario {
        name: "calibration-static",
        service,
        request: || get("/ping"),
        expected: StatusCode::NO_CONTENT,
    },
    Scenario {
        name: "calibration-capture",
        service,
        request: || get("/users/7"),
        expected: StatusCode::NO_CONTENT,
    },
    Scenario {
        name: "calibration-miss",
        service,
        request: || get("/nope"),
        expected: StatusCode::NOT_FOUND,
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
    },
    Scenario {
        name: "calibration-stacked-8",
        service: layers_8,
        request: || get("/ping"),
        expected: StatusCode::NO_CONTENT,
    },
];

/// Looks a scenario up by name, from either table.
///
/// # Panics
///
/// When no scenario has that name, which is a benchmark naming one that was
/// removed.
#[must_use]
pub fn scenario(name: &str) -> Scenario {
    SCENARIOS
        .iter()
        .copied()
        .chain(CALIBRATION.iter().copied())
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("no scenario is named {name:?}"))
}

/// Serves one request to completion on the calling thread.
///
/// Polled once with a no-op waker rather than driven by a runtime, for the
/// reason [`tests/support/counting.rs`](../../kynos/tests/support/counting.rs)
/// gives: an executor on the measuring thread is measured with it, and nothing
/// here touches a socket, timer or task, so the future is ready on its first
/// poll. This is the whole of what a benchmark measures.
///
/// # Panics
///
/// When the future is pending, which means something on the request path now
/// needs a runtime and the count stopped covering the whole request.
pub fn serve(service: &Service<()>, request: Request) -> Response {
    let mut future = pin!(service.call(request));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(response) => response,
        Poll::Pending => panic!("a request was not served on its first poll"),
    }
}
