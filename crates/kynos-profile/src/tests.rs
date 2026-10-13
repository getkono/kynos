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

use super::{
    COMPRESSION, Scenario, catalog,
    driver::{Drained, Driver, Served, frames},
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

    let service = crate::app::compressed();
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
    let service = crate::app::compressed();
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
    left: u8,
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

fn chunked() -> Box<dyn Served> {
    Box::new(
        Router::<()>::new()
            .mount(kynos::routes![chunks])
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

/// A `Poll` scenario that is not ready on its one poll would count part of a
/// request, so it fails instead.
#[test]
#[should_panic(expected = "not served and drained on its first poll")]
fn a_poll_driver_refuses_a_body_that_is_not_ready_on_one_poll() {
    drive(&*chunked(), Driver::Poll, crate::app::get("/chunks"));
}
