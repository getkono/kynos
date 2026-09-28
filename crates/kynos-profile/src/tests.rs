use std::collections::BTreeSet;

use http_body_util::BodyExt;
use kynos::http::body::Body;

use super::{CALIBRATION, COMPRESSION, SCENARIOS, Scenario, scenario, serve};

/// The octets a response carries, read outside any measurement.
fn octets(body: Body) -> Vec<u8> {
    let mut collected = std::pin::pin!(body.collect());
    match collected
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(Ok(collected)) => collected.to_bytes().to_vec(),
        other => panic!(
            "an in-memory body did not collect on its first poll: {:?}",
            other.is_ready()
        ),
    }
}

fn answered(scenario: Scenario) -> Vec<u8> {
    let service = (scenario.service)();
    let response = serve(&service, (scenario.request)());
    assert_eq!(
        response.status(),
        scenario.expected,
        "{} answered {} where its count is meant to be of a {}",
        scenario.name,
        response.status(),
        scenario.expected,
    );
    octets(response.into_body())
}

/// A benchmark that measured a refusal would read as a cheap request, so every
/// fixture is held to the status its name promises where no Valgrind is needed.
#[test]
fn every_scenario_is_answered_with_the_status_it_is_measured_at() {
    for scenario in SCENARIOS.iter().chain(CALIBRATION).chain(COMPRESSION) {
        answered(*scenario);
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
        let response = serve(&service, request);
        let applied = response
            .headers()
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
    let names: Vec<_> = SCENARIOS
        .iter()
        .chain(CALIBRATION)
        .chain(COMPRESSION)
        .map(|scenario| scenario.name)
        .collect();
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
            octets(serve(&service, crate::app::get_encoded(target, coding)).into_body()).len()
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
        let response = serve(&service, crate::app::get_encoded(target, "identity"));
        let length = octets(response.into_body()).len();
        assert!(
            (low..=high).contains(&length),
            "{target} wrote {length} octets, outside {low}..={high}"
        );
    }
}
