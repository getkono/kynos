//! Instructions and heap blocks for one request of each
//! scenario, counted under Callgrind and DHAT.
//!
//! The region is [`Served::serve`] and nothing else: the request served and
//! its body drained, under the scenario's driver. Building the service, the
//! driver, the request and the frame buffer is `setup`, and dropping them, with
//! what was drained, is `teardown`, so none is attributed: gungraun centres
//! both tools on the benchmark function. That is the region `tests/alloc.rs`
//! counts too, which is what lets the calibration group hold one instrument to
//! the other.
//!
//! The `connection_h1` and `connection_h2` groups' region is instead [`Open::serve`]: one connection
//! through `Server::serve_io`, driven to its end, with the encoded request
//! written into its duplex in setup and the response read back in teardown.
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
use std::io;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use gungraun::{Dhat, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main};
use http_body::Frame;
use kynos::http::{Request, StatusCode};
use kynos_profile::{
    connection::{Open, connected},
    driver::{Drained, Runner, Served, frames},
    scenario,
};

/// What a benchmark receives: the service, its readied driver, the request,
/// the buffer its body is drained into, and the status the response must
/// carry. The service and the driver travel through the benchmark and out
/// again so that they are dropped in `teardown` rather than inside the region.
struct Prepared {
    service: Box<dyn Served>,
    runner: Runner,
    request: Request,
    frames: Vec<Frame<Bytes>>,
    expected: StatusCode,
}

/// What leaves the region, to be checked and dropped outside it.
type Answered = (Box<dyn Served>, Runner, Drained, StatusCode);

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
/// on first use, inside whatever region first reaches them, and so is a
/// runtime's per-thread context. That is a cost per process rather than per
/// request, and it is the host-dependent part of the program count, so one
/// identical request is served, drained and dropped here, outside the region.
/// What it cannot move is the routine chosen, which is why
/// `scripts/profile_report.py` records the host. Allocation counts are not
/// affected: `tests/alloc.rs` holds a replayed request to what the first cost.
fn prepared(name: &str) -> Prepared {
    let scenario = scenario(name);
    let service = (scenario.service)();
    let runner = scenario.driver.runner();
    drop(service.serve(&runner, (scenario.request)(), frames()));
    Prepared {
        service,
        runner,
        request: (scenario.request)(),
        frames: frames(),
        expected: scenario.expected,
    }
}

/// A count of the wrong response reads as a cheap request, so the status is
/// checked here, where checking it costs the count nothing; and a body that
/// outgrew its reserved frames reallocated inside the region.
fn checked((service, runner, drained, expected): Answered) {
    assert_eq!(
        drained.status(),
        expected,
        "the profiled request was answered wrongly"
    );
    assert!(
        !drained.regrew(),
        "the profiled body produced more frames than the region reserves"
    );
    drop((drained, runner, service));
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
fn request(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
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
fn calibration(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// `lib.rs`'s `COMPRESSION` sweep: what an encode costs at each size, read
// against the octets it saves to choose `Compression`'s default `min_size`.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::identity_small("compressed-identity-small")]
#[bench::identity_1k("compressed-identity-1k")]
#[bench::identity_2k("compressed-identity-2k")]
#[bench::identity_4k("compressed-identity-4k")]
#[bench::identity_large("compressed-identity-large")]
#[bench::gzip_small("compressed-gzip-small")]
#[bench::gzip_1k("compressed-gzip-1k")]
#[bench::gzip_2k("compressed-gzip-2k")]
#[bench::gzip_4k("compressed-gzip-4k")]
#[bench::gzip_large("compressed-gzip-large")]
#[bench::br_small("compressed-br-small")]
#[bench::br_1k("compressed-br-1k")]
#[bench::br_2k("compressed-br-2k")]
#[bench::br_4k("compressed-br-4k")]
#[bench::br_large("compressed-br-large")]
#[bench::zstd_small("compressed-zstd-small")]
#[bench::zstd_1k("compressed-zstd-1k")]
#[bench::zstd_2k("compressed-zstd-2k")]
#[bench::zstd_4k("compressed-zstd-4k")]
#[bench::zstd_large("compressed-zstd-large")]
fn sweep(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// `lib.rs`'s `RUNTIME` table: what `Driver::Runtime` costs over one poll.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::json_small("json-small-runtime")]
fn driven(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// The ablation tables, `lib.rs`'s `ROUTING` through `REALISTIC`: each row
// differs from the base it names in one variable, so the report's difference
// between them is that variable's marginal cost. One group per table.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::captures_4("captures-4")]
#[bench::not_implemented("not-implemented")]
#[bench::redirect_slash("redirect-slash")]
#[bench::head_get("head-get")]
#[bench::not_found_empty("not-found-empty")]
#[bench::not_found_layered_4("not-found-layered-4")]
fn routed(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::group_layers_4("group-layers-4")]
#[bench::endpoint_layers_4("endpoint-layers-4")]
#[bench::catch_panics_router("catch-panics-router")]
#[bench::catch_panics_endpoint("catch-panics-endpoint")]
#[bench::observer_noop("observer-noop")]
#[bench::trace_nosub("trace-nosub")]
#[bench::trace_sub("trace-sub")]
fn scoped(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::inject("inject")]
#[bench::auth_bearer("auth-bearer")]
#[bench::auth_basic("auth-basic")]
#[bench::auth_apikey("auth-apikey")]
#[bench::auth_401("auth-401")]
#[bench::cookies("cookies")]
#[bench::bounds("bounds")]
#[bench::query_unused_8("query-unused-8")]
#[bench::querystring("querystring")]
#[bench::one_of("one-of")]
#[bench::text_body("text-body")]
fn extracted(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// `CODECS`' `form-echo` alone: `serde_html_form` groups the pairs it decodes
// in an `IndexMap` under `std`'s `RandomState`, keyed afresh in each process,
// so how far each field's lookup probes, and so its count, moves by a few
// instructions with the key the process drew; `requests.tsv` marks this group
// `trend` rather than the whole table's.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::form_echo("form-echo")]
fn form_coded(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::multipart_echo("multipart-echo")]
#[bench::protobuf_echo("protobuf-echo")]
#[bench::reject_415("reject-415")]
#[bench::reject_413("reject-413")]
#[bench::reject_422("reject-422")]
fn coded(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::binary_full("binary-full")]
#[bench::ranged("ranged")]
#[bench::conditional_304("conditional-304")]
#[bench::negotiated("negotiated")]
#[bench::created("created")]
fn responded(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::cors_simple("cors-simple")]
#[bench::cors_preflight("cors-preflight")]
#[bench::csrf("csrf")]
#[bench::request_id_counter("request-id-counter")]
#[bench::security_headers("security-headers")]
#[bench::body_size("body-size")]
#[bench::concurrency("concurrency")]
#[bench::timeout("timeout")]
#[bench::body_timeout("body-timeout")]
#[bench::decompress_gzip("decompress-gzip")]
#[bench::cache_miss("cache-miss")]
#[bench::cache_hit("cache-hit")]
#[bench::set_cookies("set-cookies")]
#[bench::compression_default("compression-default")]
#[bench::unchecked_layer("unchecked-layer")]
fn intercepted(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// `INTERCEPTORS`' `rate-limit` alone: its quota keys, its window arithmetic
// and the remaining count and reset it writes are read off the wall clock,
// so its count moves by a few instructions with the time it ran at, and
// `requests.tsv` marks this group `trend` rather than the whole table's.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::rate_limit("rate-limit")]
fn limited(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

// `INTERCEPTORS`' `request-id-random` alone: `Random` keys its hash afresh in
// each process, and although it executes and allocates the same whatever its
// key, glibc's `malloc` serves its block along a longer path in some
// processes, so its instruction count moves while its heap does not;
// `requests.tsv` marks this group `trend` rather than the whole table's.
#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::request_id_random("request-id-random")]
fn random_id(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::sse_16("sse-16")]
#[bench::sse_keepalive("sse-keepalive")]
#[bench::ndjson_16("ndjson-16")]
#[bench::json_seq_16("json-seq-16")]
#[bench::binary_stream_16("binary-stream-16")]
#[bench::compressed_stream_gzip("compressed-stream-gzip")]
fn streamed(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::assets_hit("assets-hit")]
#[bench::docs_json("docs-json")]
fn served_static(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

#[library_benchmark(setup = prepared, teardown = checked, config = heap())]
#[bench::stack_typical("stack-typical")]
fn composed(prepared: Prepared) -> Answered {
    let Prepared {
        service,
        runner,
        request,
        frames,
        expected,
    } = prepared;
    let drained = black_box(&*service).serve(black_box(&runner), black_box(request), frames);
    (service, runner, black_box(drained), expected)
}

/// A connection row's setup: its driver and the connection made ready, the
/// request already in the duplex, and the status its response must carry.
type Opened = (Runner, Open, StatusCode);

/// What leaves a connection row's region, to be checked and dropped outside it.
type Closed = (Runner, Open, io::Result<()>, StatusCode);

/// The connection, and the request the region serves — over the *second*
/// connection, for [`prepared`]'s reason: one identical connection is served
/// to its end and dropped here first.
///
/// That connection is served at the start of a wall-clock second, for hyper's
/// `Date` cache: it renders the header again, allocating, on the first
/// response written in each new second, so a region that a second boundary
/// separated from the warming connection would count one more block than one
/// it did not, by the clock alone. Served at the start of a second, the warming
/// connection renders it and the region, milliseconds later even under
/// Valgrind, reuses it: the counts are a request's, and the render, a cost per
/// second rather than per request, is left out of every row alike.
fn opened(name: &str) -> Opened {
    let connected = connected(name);
    let runner = connected.scenario.driver.runner();
    // Both are opened first: building a service is the slowest part of setup
    // under Valgrind, and it must not stand between the render and the region.
    let mut warm = connected.open(&runner);
    let open = connected.open(&runner);
    let into_second = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .subsec_nanos();
    thread::sleep(Duration::from_nanos(u64::from(1_000_000_000 - into_second)));
    warm.serve(&runner)
        .expect("the warming connection closes cleanly");
    drop(warm);
    (runner, open, connected.scenario.expected)
}

/// As [`checked`], reading the status from what the server wrote; and a
/// connection that ended in an error counted that error's path.
fn closed((runner, mut open, ended, expected): Closed) {
    ended.expect("the profiled connection closes cleanly");
    assert_eq!(
        open.status(),
        expected,
        "the profiled request was answered wrongly"
    );
    drop((open, runner));
}

// `connection/mod.rs`'s `CONNECTION` table: a request through `Server::serve_io`
// over an in-memory duplex, the server path the rows above never enter. Split
// by protocol because HTTP/2 Huffman-codes the `Date` value into every
// response, which costs a few instructions more or fewer as the clock's
// characters change, so only the HTTP/1.1 group is exact; `requests.tsv` marks
// `connection_h2` `trend`.
#[library_benchmark(setup = opened, teardown = closed, config = heap())]
#[bench::plaintext("conn-h1-plaintext")]
#[bench::echo_cl("conn-h1-echo-cl")]
#[bench::echo_chunked("conn-h1-echo-chunked")]
#[bench::second("conn-h1-second")]
fn http1(opened: Opened) -> Closed {
    let (runner, mut open, expected) = opened;
    let ended = black_box(&mut open).serve(black_box(&runner));
    (runner, open, black_box(ended), expected)
}

#[library_benchmark(setup = opened, teardown = closed, config = heap())]
#[bench::plaintext("conn-h2-plaintext")]
#[bench::second("conn-h2-second")]
fn http2(opened: Opened) -> Closed {
    let (runner, mut open, expected) = opened;
    let ended = black_box(&mut open).serve(black_box(&runner));
    (runner, open, black_box(ended), expected)
}

library_benchmark_group!(name = scenarios, benchmarks = request);
library_benchmark_group!(name = alloc_counter_agreement, benchmarks = calibration);
library_benchmark_group!(name = compression, benchmarks = sweep);
library_benchmark_group!(name = runtime, benchmarks = driven);
library_benchmark_group!(name = connection_h1, benchmarks = http1);
library_benchmark_group!(name = connection_h2, benchmarks = http2);
library_benchmark_group!(name = routing, benchmarks = routed);
library_benchmark_group!(name = scope, benchmarks = scoped);
library_benchmark_group!(name = extract, benchmarks = extracted);
library_benchmark_group!(name = form_echo, benchmarks = form_coded);
library_benchmark_group!(name = codecs, benchmarks = coded);
library_benchmark_group!(name = respond, benchmarks = responded);
library_benchmark_group!(name = interceptors, benchmarks = intercepted);
library_benchmark_group!(name = rate_limit, benchmarks = limited);
library_benchmark_group!(name = request_id_random, benchmarks = random_id);
library_benchmark_group!(name = stream, benchmarks = streamed);
library_benchmark_group!(name = statics, benchmarks = served_static);
library_benchmark_group!(name = realistic, benchmarks = composed);

main!(
    library_benchmark_groups = scenarios,
    alloc_counter_agreement,
    compression,
    runtime,
    connection_h1,
    connection_h2,
    routing,
    scope,
    extract,
    form_echo,
    codecs,
    respond,
    interceptors,
    rate_limit,
    request_id_random,
    stream,
    statics,
    realistic
);
