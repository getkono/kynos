use std::convert::Infallible;

use super::{ShortCircuit, described_statuses, mismatch_between, short_circuit_mismatch};
use crate::schema::registry::Registry;

fn responses(statuses: &[u16]) -> kynos_openapi::Responses {
    statuses
        .iter()
        .fold(kynos_openapi::Responses::new(), |responses, &status| {
            responses.with(status, kynos_openapi::Response::new("a response"))
        })
}

#[test]
fn only_exact_codes_are_compared() {
    let mut declared = responses(&[503]);
    declared = declared.with_pattern(
        kynos_openapi::StatusPattern::ServerError,
        kynos_openapi::RefOr::Item(kynos_openapi::Response::new("any server error")),
    );
    declared = declared.with_default(kynos_openapi::Response::new("anything else"));

    // `5XX` and `default` are ranges, not claims about one status, so a
    // short circuit has nothing exact to be compared against there.
    assert_eq!(described_statuses(&declared), vec![503]);
}

#[test]
fn agreement_in_any_order_is_agreement() {
    assert!(mismatch_between("Limits", &[503, 429], &responses(&[429, 503])).is_none());
    assert!(mismatch_between("Repeats", &[503, 503], &responses(&[503])).is_none());
}

#[test]
fn a_status_declared_but_not_described_is_reported() {
    let found = mismatch_between("Liar", &[418], &responses(&[503]))
        .expect("418 is declared and never described");

    assert!(matches!(
        found,
        kynos_openapi::SpecError::ShortCircuitMismatch { ref declared, ref described, .. }
            if declared == &[418] && described == &[503]
    ));
    assert!(found.to_string().contains("Liar"));
}

#[test]
fn a_status_described_but_not_declared_is_reported() {
    assert!(mismatch_between("Quiet", &[], &responses(&[500])).is_some());
}

#[test]
fn an_interceptor_that_never_answers_declares_nothing() {
    assert_eq!(<Infallible as ShortCircuit>::STATUSES, &[] as &[u16]);
    assert!(short_circuit_mismatch::<Infallible>(&mut Registry::default()).is_none());
}
