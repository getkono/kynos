use std::collections::BTreeSet;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use kynos::{
    Router,
    di::inject::Inject,
    extract::body::json::Json,
    http::{Request, StatusCode, media::OctetStream},
    response::stream::binary::BinaryStream,
};

use tracing::subscriber::NoSubscriber;

use super::{
    CODECS, COMPRESSION, EXTRACT, INTERCEPTORS, REALISTIC, RESPOND, ROUTING, SCOPE, STATIC, STREAM,
    Scenario, catalog,
    connection::{
        CONNECTION, connected,
        wire::{http1, http1_answered},
    },
    driver::{Drained, Driver, FRAMES, Served, frames},
    scenario,
};

/// One request through `service`, driven as the benchmark drives it.
fn drive(service: &dyn Served, driver: Driver, request: Request) -> Drained {
    service.serve(&driver.runner(), request, frames())
}

/// What `scenario` answers, held to the status it is measured at and to a
/// drain that stayed within the frames reserved for it.
fn answered(scenario: Scenario) -> Vec<u8> {
    let drained = drive(
        &*(scenario.service)(),
        scenario.driver,
        (scenario.request)(),
    );
    assert_eq!(
        drained.status(),
        scenario.expected,
        "{} answered {} where its count is meant to be of a {}",
        scenario.name,
        drained.status(),
        scenario.expected,
    );
    assert!(
        !drained.regrew(),
        "{} produced more frames than the region reserves",
        scenario.name
    );
    drained.octets()
}

/// A benchmark that measured a refusal would read as a cheap request, so every
/// fixture is held to the status its name promises where no Valgrind is needed,
/// under the driver it is measured with.
#[test]
fn every_scenario_is_answered_with_the_status_it_is_measured_at() {
    let mut drivers = BTreeSet::new();
    for scenario in catalog() {
        answered(scenario);
        drivers.insert(format!("{:?}", scenario.driver));
    }
    assert_eq!(drivers.len(), 2, "both drivers are exercised: {drivers:?}");
}

/// A base names the scenario a report subtracts, so one naming nothing would
/// leave a row's marginal cost unreported, and one naming itself would report
/// a zero.
#[test]
fn every_base_names_another_existing_scenario() {
    let names: BTreeSet<_> = catalog().map(|scenario| scenario.name).collect();
    for scenario in catalog() {
        if let Some(base) = scenario.base {
            assert!(
                names.contains(base) && base != scenario.name,
                "{} names {base:?} as its base",
                scenario.name
            );
        }
    }
}

/// A sweep row counts the encode its name promises, so each response carries
/// exactly the coding its request offered, and the identity rows carry none.
/// A row answered in identity where a coding was asked for would read as a
/// free encode.
#[test]
fn every_compression_row_is_encoded_as_it_asks() {
    for scenario in COMPRESSION {
        let service = (scenario.service)();
        let request = (scenario.request)();
        let offered = request
            .headers()
            .get(kynos::http::header::ACCEPT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .expect("every sweep row offers one coding");
        let drained = drive(&*service, scenario.driver, request);
        let applied = drained
            .head
            .headers
            .get(kynos::http::header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        let expected = (offered != "identity").then_some(offered);
        assert_eq!(applied, expected, "{}", scenario.name);
    }
}

/// A report is read by name, so two scenarios sharing one would put two
/// requests' numbers under one heading.
#[test]
fn every_scenario_name_is_unique_and_resolves_to_itself() {
    let names: Vec<_> = catalog().map(|scenario| scenario.name).collect();
    let unique: BTreeSet<_> = names.iter().collect();
    assert_eq!(
        names.len(),
        unique.len(),
        "a scenario name is repeated: {names:?}"
    );

    for name in names {
        assert_eq!(scenario(name).name, name);
    }
}

/// The sizes kynos-bench's catalog states, so a number read here describes the
/// payload a number read there does: `json-small` roughly 100 octets,
/// `echo-post` roughly 1 KiB each way, `json-large` roughly 64 KiB.
#[test]
fn every_payload_is_the_size_the_catalog_names() {
    let within = |name: &str, low: usize, high: usize| {
        let length = answered(scenario(name)).len();
        assert!(
            (low..=high).contains(&length),
            "{name} wrote {length} octets, outside {low}..={high}"
        );
    };

    within("json-small", 64, 160);
    within("echo-post", 768, 1_536);
    within("json-large", 48 * 1_024, 80 * 1_024);
    // The sweep's two ends, read unencoded.
    within("compressed-identity-small", 64, 160);
    within("compressed-identity-large", 48 * 1_024, 80 * 1_024);
}

/// The rule `Compression`'s default `min_size` is read from, held to the sweep:
/// the 2 KiB row is the smallest at which every coding saves at least one
/// Ethernet segment's payload, 1460 octets.
///
/// This holds the reason rather than the number, and in one direction only. A
/// codec upgrade or a level change that moved the crossing past the 2 KiB row
/// fails here and sends someone back to `middleware.md`'s table, where
/// `tests/middleware.rs` would still pass on the constant alone. No row can
/// show it moving down: `every_sweep_size_is_the_size_it_names` holds the
/// 1 KiB row under a segment unencoded, so it can never save one.
#[test]
fn the_default_threshold_is_where_every_coding_first_saves_a_segment() {
    const SEGMENT: usize = 1_460;

    let service = crate::app::sweep::compressed();
    let saved = |target: &str, coding: &'static str| {
        let length = |coding| {
            drive(
                &*service,
                Driver::Poll,
                crate::app::get_encoded(target, coding),
            )
            .octets()
            .len()
        };
        length("identity").saturating_sub(length(coding))
    };

    for coding in ["gzip", "br", "zstd"] {
        let at_2k = saved("/json/2k", coding);
        assert!(
            at_2k >= SEGMENT,
            "{coding} saves {at_2k} octets at the 2 KiB row, under a segment"
        );
    }
}

/// The sweep's middle rows are the sizes their names say, measured through the
/// identity coding of the same routes the sweep encodes.
#[test]
fn every_sweep_size_is_the_size_it_names() {
    let service = crate::app::sweep::compressed();
    for (target, low, high) in [
        ("/json/1k", 896, 1_280),
        ("/json/2k", 1_792, 2_304),
        ("/json/4k", 3_840, 4_608),
    ] {
        let length = drive(
            &*service,
            Driver::Poll,
            crate::app::get_encoded(target, "identity"),
        )
        .octets()
        .len();
        assert!(
            (low..=high).contains(&length),
            "{target} wrote {length} octets, outside {low}..={high}"
        );
    }
}

/// The ablation rows whose answer is meant to differ from their base's,
/// and why: a body each writes in another shape, or none.
const ANSWERED_OTHERWISE: &[&str] = &[
    // One capture more, which the handler adds into its sum.
    "captures-4",
    // A `HEAD`, answered without content.
    "head-get",
    // The description, not `plaintext`'s octets.
    "docs-json",
    // `echo-post`'s document in another codec's octets.
    "form-echo",
    "multipart-echo",
    "protobuf-echo",
    // `ndjson-16`'s lines, gzip-coded.
    "compressed-stream-gzip",
];

/// A row is meant to differ from its base in one variable, and that variable
/// is never what the response says: so every row answered as its base is
/// writes its base's octets, and a fixture that drifted to another payload is
/// caught here rather than read as that variable's cost.
#[test]
fn every_ablation_row_writes_what_its_base_writes() {
    let ablation = [
        ROUTING,
        SCOPE,
        EXTRACT,
        CODECS,
        RESPOND,
        INTERCEPTORS,
        STREAM,
        STATIC,
        REALISTIC,
    ];
    for &row in ablation.iter().copied().flatten() {
        let Some(base) = row.base.map(scenario) else {
            continue;
        };
        if row.expected != StatusCode::OK
            || base.expected != StatusCode::OK
            || ANSWERED_OTHERWISE.contains(&row.name)
        {
            continue;
        }
        assert_eq!(
            answered(row),
            answered(base),
            "{} writes other octets than {}",
            row.name,
            base.name
        );
    }
}

/// Each stream row produces the items its name counts, one data frame each,
/// so a stream that ended early would not read as cheap framing.
#[test]
fn every_stream_row_produces_its_items_one_frame_each() {
    for scenario in STREAM.iter().filter(|scenario| scenario.base.is_none()) {
        let drained = drive(
            &*(scenario.service)(),
            scenario.driver,
            (scenario.request)(),
        );
        let data = drained
            .frames
            .iter()
            .filter(|frame| frame.is_data())
            .count();
        assert_eq!(data, crate::app::stream::ITEMS, "{}", scenario.name);
    }
}

/// The value of `name` in what `scenario` answers.
fn field(scenario: Scenario, name: &str) -> Option<String> {
    drive(
        &*(scenario.service)(),
        scenario.driver,
        (scenario.request)(),
    )
    .head
    .headers
    .get(name)
    .and_then(|value| value.to_str().ok())
    .map(str::to_owned)
}

/// `compression-default` and the composite decline: `json-small` is under the
/// default `min_size`, which is what they count applying. The compressed
/// stream is encoded, since a stream states no length to be under it.
#[test]
fn the_default_compression_declines_json_small_and_encodes_a_stream() {
    assert_eq!(
        field(scenario("compression-default"), "content-encoding"),
        None
    );
    assert_eq!(field(scenario("stack-typical"), "content-encoding"), None);
    assert_eq!(
        field(scenario("compressed-stream-gzip"), "content-encoding").as_deref(),
        Some("gzip")
    );
}

/// An interceptor row counts its interceptor's work only if the request
/// reaches it, so each one whose work shows in the response is held to
/// showing it.
#[test]
fn every_interceptor_row_reaches_its_interceptor() {
    for (name, shown) in [
        ("cors-simple", "access-control-allow-origin"),
        ("cors-preflight", "access-control-allow-methods"),
        ("request-id-counter", "x-request-id"),
        ("request-id-random", "x-request-id"),
        ("security-headers", "x-content-type-options"),
        ("set-cookies", "set-cookie"),
        ("cache-miss", "age"),
        ("cache-hit", "age"),
        ("stack-typical", "access-control-allow-origin"),
        ("stack-typical", "x-request-id"),
    ] {
        assert!(
            field(scenario(name), shown).is_some(),
            "{name} answered without `{shown}`"
        );
    }
}

/// `trace-sub` differs from `trace-nosub` in a subscriber alone, so one is
/// the thread's default while its service lives, and none is once it is
/// dropped.
#[test]
fn trace_sub_serves_under_a_subscriber_of_its_own() {
    let unsubscribed = || tracing::dispatcher::get_default(tracing::Dispatch::is::<NoSubscriber>);
    assert!(unsubscribed());
    let service = (scenario("trace-sub").service)();
    assert!(!unsubscribed());
    drop(service);
    assert!(unsubscribed());
}

#[derive(Clone)]
struct Pool(u64);

/// A context that is not `()`, which is what erasing the service is for.
#[derive(kynos::Provider)]
struct App {
    pool: Pool,
}

#[kynos::get("/pool")]
async fn pool(Inject(pool): Inject<Pool>) -> Json<u64> {
    Json(pool.0)
}

/// A scenario's service may carry the context DI and authentication read, and
/// is served through the same erased handle as one of context `()`.
#[test]
fn a_service_of_any_context_is_served_through_the_erased_handle() {
    let service: Box<dyn Served> = Box::new(
        Router::<App>::new()
            .mount(kynos::routes![pool])
            .build(App { pool: Pool(7) })
            .expect("a describable router"),
    );
    let drained = drive(&*service, Driver::Poll, crate::app::get("/pool"));
    assert_eq!(drained.status(), StatusCode::OK);
    assert_eq!(drained.octets(), b"7");
}

/// Three chunks, each produced on the poll after one that returned `Pending`,
/// so no single poll drains the body.
struct Chunks {
    left: usize,
    yielded: bool,
}

impl Stream for Chunks {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.left == 0 {
            return Poll::Ready(None);
        }
        if !self.yielded {
            self.yielded = true;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.yielded = false;
        self.left -= 1;
        Poll::Ready(Some(Ok(Bytes::from_static(b"chunk"))))
    }
}

#[kynos::get("/chunks")]
async fn chunks() -> BinaryStream<Chunks, OctetStream> {
    BinaryStream::new(Chunks {
        left: 3,
        yielded: false,
    })
}

/// One frame more than [`FRAMES`] reserves.
#[kynos::get("/chunks-past-reserve")]
async fn chunks_past_reserve() -> BinaryStream<Chunks, OctetStream> {
    BinaryStream::new(Chunks {
        left: FRAMES + 1,
        yielded: false,
    })
}

fn chunked() -> Box<dyn Served> {
    Box::new(
        Router::<()>::new()
            .mount(kynos::routes![chunks, chunks_past_reserve])
            .build(())
            .expect("a describable router"),
    )
}

/// The region drains the body: a streamed body's every frame is produced
/// inside it, so its production is counted with the request.
#[test]
fn a_runtime_driver_drains_a_streamed_body_frame_by_frame() {
    let drained = drive(&*chunked(), Driver::Runtime, crate::app::get("/chunks"));
    assert_eq!(drained.frames.len(), 3);
    assert_eq!(drained.octets(), b"chunkchunkchunk");
    assert!(!drained.regrew());
}

/// A body producing more frames than were reserved reallocates the buffer
/// inside the region, which `regrew` reports so the benchmark's teardown
/// refuses the count.
#[test]
fn a_body_past_the_reserved_frames_regrew() {
    let drained = drive(
        &*chunked(),
        Driver::Runtime,
        crate::app::get("/chunks-past-reserve"),
    );
    assert_eq!(drained.frames.len(), FRAMES + 1);
    assert!(drained.regrew());
}

/// A `Poll` scenario that is not ready on its one poll would count part of a
/// request, so it fails instead.
#[test]
#[should_panic(expected = "not served and drained on its first poll")]
fn a_poll_driver_refuses_a_body_that_is_not_ready_on_one_poll() {
    drive(&*chunked(), Driver::Poll, crate::app::get("/chunks"));
}

/// A connection row counts a request that entered through `serve_io`, so each
/// is answered over its wire with the status it is measured at, and its
/// connection ends cleanly on its own rather than leaving the region pending
/// or ending in an error a count would include.
#[test]
fn every_connection_row_is_answered_over_its_wire_and_closes_cleanly() {
    for connected in CONNECTION {
        let scenario = connected.scenario;
        assert_eq!(scenario.driver, Driver::Runtime, "{}", scenario.name);
        let runner = scenario.driver.runner();
        let mut open = connected.open(&runner);
        open.serve(&runner)
            .unwrap_or_else(|error| panic!("{} ended with {error}", scenario.name));
        assert_eq!(open.status(), scenario.expected, "{}", scenario.name);
    }
}

/// The chunked row differs from its base in framing alone: the same request,
/// framed by `transfer-encoding` where its base is framed by `content-length`.
#[test]
fn the_chunked_row_frames_its_body_by_chunks_and_its_base_by_length() {
    let chunked = String::from_utf8(http1(
        (connected("conn-h1-echo-chunked").scenario.request)(),
        true,
        true,
    ))
    .expect("an ASCII request");
    let length = String::from_utf8(http1(
        (connected("conn-h1-echo-cl").scenario.request)(),
        false,
        true,
    ))
    .expect("an ASCII request");
    assert!(chunked.contains("transfer-encoding: chunked\r\n"));
    assert!(!chunked.contains("content-length"));
    assert!(chunked.ends_with("\r\n0\r\n\r\n"));
    assert!(length.contains("content-length: "));
    assert!(!length.contains("transfer-encoding"));
}

/// The keep-alive row's first request leaves its connection open and only
/// its second closes it, so the region serves one request on a connection
/// setup already paid for; and setup reads the first response as whole only
/// once its body has arrived.
#[test]
fn the_keep_alive_row_closes_only_behind_its_second_request() {
    let request = connected("conn-h1-second").scenario.request;
    let first = String::from_utf8(http1(request(), false, false)).expect("an ASCII request");
    let second = String::from_utf8(http1(request(), false, true)).expect("an ASCII request");
    assert!(!first.contains("connection:"));
    assert!(second.contains("connection: close\r\n"));
    let response = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";
    assert!(!http1_answered(&response[..response.len() - 1]));
    assert!(http1_answered(response));
}
