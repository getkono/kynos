use super::{Malformed, MalformedReason, Presented};
use crate::http::StatusCode;

/// A malformed credential, for `reason`.
fn malformed(reason: MalformedReason) -> Presented<&'static str> {
    Presented::Malformed(Malformed::new(reason))
}

/// `require` refuses absence and malformation with the guard's own 401, and
/// hands a present credential back.
#[test]
fn require_refuses_whatever_is_not_a_well_formed_credential() {
    assert_eq!(Presented::Present("t-1").require().ok(), Some("t-1"));
    for refused in [Presented::Absent, malformed(MalformedReason::OtherScheme)] {
        let rejection = refused.require().expect_err("refused");
        assert_eq!(rejection.status(), StatusCode::UNAUTHORIZED);
    }
}

/// `into_result` keeps absence a value, for a scheme that decides it, and
/// refuses only malformation.
#[test]
fn into_result_refuses_only_a_malformed_credential() {
    assert_eq!(
        Presented::Present("t-1").into_result().ok(),
        Some(Some("t-1"))
    );
    assert_eq!(Presented::<&str>::Absent.into_result().ok(), Some(None));
    let rejection = malformed(MalformedReason::Repeated)
        .into_result()
        .expect_err("refused");
    assert_eq!(rejection.status(), StatusCode::UNAUTHORIZED);
}

/// `map` reaches only a present credential, and keeps a malformed one's reason.
#[test]
fn map_reaches_only_a_present_credential() {
    assert_eq!(
        Presented::Present("t-1").map(str::len),
        Presented::Present(3)
    );
    assert_eq!(Presented::<&str>::Absent.map(str::len), Presented::Absent);
    assert_eq!(
        malformed(MalformedReason::NotText).map(str::len),
        Presented::Malformed(Malformed::new(MalformedReason::NotText))
    );
}

/// `and_then` reads further only from a present credential, and may find it
/// malformed there.
#[test]
fn and_then_reaches_only_a_present_credential() {
    let reread = |token: &'static str| {
        if token.is_empty() {
            malformed(MalformedReason::BadCredentials)
        } else {
            Presented::Present(token)
        }
    };
    assert_eq!(
        Presented::Present("t-1").and_then(reread),
        Presented::Present("t-1")
    );
    assert_eq!(
        Presented::Present("").and_then(reread),
        malformed(MalformedReason::BadCredentials)
    );
    assert_eq!(
        Presented::<&str>::Absent.and_then(reread),
        Presented::Absent
    );
    assert_eq!(
        malformed(MalformedReason::Repeated).and_then(reread),
        malformed(MalformedReason::Repeated)
    );
}

/// Every reason, each once: the exhaustive `match` stops compiling when a
/// variant is added, and the labels a counter keys on stay distinct.
#[test]
fn every_reason_has_a_distinct_stable_label_and_a_sentence() {
    const fn listed(reason: MalformedReason) {
        match reason {
            MalformedReason::Repeated
            | MalformedReason::NotText
            | MalformedReason::NoScheme
            | MalformedReason::OtherScheme
            | MalformedReason::BadCredentials
            | MalformedReason::ReservedField
            | MalformedReason::AccompanyingHeaders => {}
        }
    }
    let reasons = [
        (MalformedReason::Repeated, "repeated"),
        (MalformedReason::NotText, "not_text"),
        (MalformedReason::NoScheme, "no_scheme"),
        (MalformedReason::OtherScheme, "other_scheme"),
        (MalformedReason::BadCredentials, "bad_credentials"),
        (MalformedReason::ReservedField, "reserved_field"),
        (MalformedReason::AccompanyingHeaders, "accompanying_headers"),
    ];

    let mut sentences = Vec::new();
    for (reason, label) in reasons {
        listed(reason);
        assert_eq!(reason.as_str(), label);
        let malformed = Malformed::new(reason);
        assert_eq!(malformed.reason(), reason);
        assert_eq!(malformed.to_string(), reason.to_string());
        sentences.push(reason.to_string());
    }
    sentences.sort();
    sentences.dedup();
    assert_eq!(
        sentences.len(),
        reasons.len(),
        "each reason reads differently"
    );
}
