use std::collections::BTreeSet;

use http_body_util::BodyExt;
use kynos::http::body::Body;

use super::{CALIBRATION, SCENARIOS, Scenario, scenario, serve};

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
    for scenario in SCENARIOS.iter().chain(CALIBRATION.iter()) {
        answered(*scenario);
    }
}

/// A report is read by name, so two scenarios sharing one would put two
/// requests' numbers under one heading.
#[test]
fn every_scenario_name_is_unique_and_resolves_to_itself() {
    let names: Vec<_> = SCENARIOS
        .iter()
        .chain(CALIBRATION.iter())
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
}
