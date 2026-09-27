//! Instructions, estimated cycles and heap blocks for one request of each
//! scenario, counted under Callgrind and DHAT.
//!
//! The region is [`kynos_profile::serve`] and nothing else. Building the
//! service and the request is `setup`, and dropping both, with the response,
//! is `teardown`, so neither is attributed: gungraun centres both tools on the
//! benchmark function. That is the region `tests/alloc.rs` counts too, which is
//! what lets the calibration group hold one instrument to the other.
//!
//! What gungraun prints is not the number of record: its Callgrind total
//! includes glibc's allocator, whose cost depends on the heap's history and
//! moves between runs. `scripts/profile_report.py` reads the output files and
//! splits them by object; see it for why and for what is compared.

// gungraun's macros expand to public modules, constants and functions with no
// documentation or `#[must_use]` of their own, and the workspace denies both
// lints. What they generate is the harness's plumbing rather than a surface
// anyone reads or calls.
#![allow(missing_docs, clippy::must_use_candidate)]

use std::hint::black_box;

use gungraun::{Dhat, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main};
use kynos::{
    http::{Request, Response, StatusCode},
    router::service::Service,
};
use kynos_profile::{scenario, serve};

/// What a benchmark receives: the service, the request, and the status the
/// response must carry. The service travels through the benchmark and out
/// again so that it is dropped in `teardown` rather than inside the region.
type Prepared = (Service<()>, Request, StatusCode);

/// What leaves the region, to be checked and dropped outside it.
type Served = (Service<()>, Response, StatusCode);

/// DHAT alongside Callgrind, keeping enough of each backtrace to reach the
/// benchmark frame.
///
/// gungraun attributes an allocation to the region by finding the benchmark
/// function in its backtrace, and DHAT keeps twelve frames by default. A
/// request through eight interceptors allocates deeper than that — each layer
/// is an `async` frame or two — so at the default an eight-layer stack counted
/// *fewer* blocks than no stack at all, silently. 500 is Valgrind's maximum;
/// every scenario today counts the same at 100, and the calibration's stacked
/// rows are what would notice a request deep enough to need more.
fn heap() -> LibraryBenchmarkConfig {
    let mut config = LibraryBenchmarkConfig::default();
    config.tool(Dhat::with_args(["--num-callers=500"]));
    config
}

/// The service, and the request the region serves — the *second* one.
///
/// The first request a process serves pays for things no later request does:
/// `std`'s CPU-feature cache and `memchr`'s choice of routine are initialised
/// on first use, inside whatever region first reaches them. That is a cost per
/// process rather than per request, and it is the host-dependent part of the
/// program count, so one identical request is served and dropped here, outside
/// the region. What it cannot move is the routine chosen, which is why
/// `scripts/profile_report.py` records the host. Allocation counts are not
/// affected: `tests/alloc.rs` holds a replayed request to what the first cost.
fn prepared(name: &str) -> Prepared {
    let scenario = scenario(name);
    let service = (scenario.service)();
    drop(serve(&service, (scenario.request)()));
    let request = (scenario.request)();
    (service, request, scenario.expected)
}

/// A count of the wrong response reads as a cheap request, so the status is
/// checked here, where checking it costs the count nothing.
fn checked((service, response, expected): Served) {
    assert_eq!(
        response.status(),
        expected,
        "the profiled request was answered wrongly"
    );
    drop((response, service));
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::plaintext("plaintext")]
#[bench::json_small("json-small")]
#[bench::json_large("json-large")]
#[bench::echo_post("echo-post")]
#[bench::path_params("path-params")]
#[bench::headers("headers")]
#[bench::layers_0("layers-0")]
#[bench::layers_4("layers-4")]
#[bench::layers_8("layers-8")]
#[bench::not_found("not-found")]
#[bench::method_not_allowed("method-not-allowed")]
#[bench::rejection("rejection")]
fn request((service, request, expected): Prepared) -> Served {
    let response = serve(black_box(&service), black_box(request));
    (service, black_box(response), expected)
}

// The shapes and stacks `tests/alloc.rs` counts with `alloc_counter`.
// `scripts/profile_report.py` pairs each with its row by request target or
// depth, and holds DHAT's block count to the count that row records. A plain
// comment because `library_benchmark` refuses any attribute but its own.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::static_match("calibration-static")]
#[bench::capture("calibration-capture")]
#[bench::miss("calibration-miss")]
#[bench::stacked_4("calibration-stacked-4")]
#[bench::stacked_8("calibration-stacked-8")]
fn calibration((service, request, expected): Prepared) -> Served {
    let response = serve(black_box(&service), black_box(request));
    (service, black_box(response), expected)
}

library_benchmark_group!(name = scenarios, benchmarks = request);
library_benchmark_group!(name = alloc_counter_agreement, benchmarks = calibration);

main!(
    library_benchmark_groups = scenarios,
    alloc_counter_agreement
);
