//! Negotiation, and the guard that keeps a strong validator honest.

use super::{Coding, Negotiated, negotiate, quality, strongly_tagged};
use crate::http::{HeaderMap, HeaderValue, header};

/// A request accepting `value`, or accepting nothing at all.
fn accepting(value: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(value) = value {
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// Every rule RFC 9110 section 12.5.3 states, and what each resolves to.
///
/// One table, because the rules interact: identity's default acceptability
/// is what decides two of the rows, and the wildcard's reach decides two
/// more.
#[test]
fn every_negotiation_rule_the_specification_states_is_applied() {
    let cases: &[(&str, Option<&str>, Negotiated)] = &[
        // Rule 1: absent means everything is acceptable.
        ("no field at all", None, Negotiated::Identity),
        // An empty value "implies that the user agent does not want any
        // content coding in response" -- it excludes nothing, so identity.
        ("an empty field value", Some(""), Negotiated::Identity),
        (
            "a plain coding",
            Some("gzip"),
            Negotiated::Encode(Coding::Gzip),
        ),
        (
            "the deprecated spelling of one",
            Some("x-gzip"),
            Negotiated::Encode(Coding::Gzip),
        ),
        (
            "a coding in another case",
            Some("GZIP"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Server preference breaks a tie: zstd is preferred over gzip.
        (
            "two codings weighted equally",
            Some("gzip, zstd"),
            Negotiated::Encode(Coding::Zstd),
        ),
        // The client's weighting overrides the server's preference.
        (
            "a client preferring the server's second choice",
            Some("gzip;q=1.0, zstd;q=0.5"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Rule 2, explicit form.
        (
            "identity refused by name",
            Some("gzip, identity;q=0"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // Rule 2, wildcard form -- the one an implementation misses.
        (
            "identity refused through the wildcard",
            Some("gzip, *;q=0"),
            Negotiated::Encode(Coding::Gzip),
        ),
        // A more specific identity entry beats the wildcard.
        (
            "a wildcard refusal with identity readmitted",
            Some("*;q=0, identity"),
            Negotiated::Identity,
        ),
        (
            "every coding refused",
            Some("gzip;q=0, br;q=0, zstd;q=0"),
            Negotiated::Identity,
        ),
        // Nothing left at all: this is the 406.
        ("everything refused", Some("*;q=0"), Negotiated::Nothing),
        (
            "every coding and identity refused by name",
            Some("gzip;q=0, br;q=0, zstd;q=0, identity;q=0"),
            Negotiated::Nothing,
        ),
        // A client preferring identity gets it.
        (
            "identity preferred over a coding",
            Some("gzip;q=0.5, identity;q=1.0"),
            Negotiated::Identity,
        ),
    ];

    for (description, accept, expected) in cases {
        assert_eq!(negotiate(&accepting(*accept)), *expected, "{description}");
    }
}

/// A weight above 1 is not a qvalue and must not outrank one.
///
/// RFC 9110 section 12.4.2 bounds it at 1. Read literally, `q=1.5` beats a
/// legitimate `q=1.0` -- a preference inversion no client can have meant.
#[test]
fn a_weight_outside_the_range_cannot_outrank_one_inside_it() {
    assert_eq!(quality("gzip;q=1.5", "gzip"), Some(1.0));
    assert_eq!(quality("gzip;q=-1", "gzip"), Some(0.0));

    // The inversion itself: with clamping, the server's preference decides.
    assert_eq!(
        negotiate(&accepting(Some("gzip;q=1.5, zstd;q=1.0"))),
        Negotiated::Encode(Coding::Zstd)
    );
}

/// An unparsable weight is a refusal, which the module already argued for:
/// a client that wrote something meaningless did not ask for this coding.
#[test]
fn an_unparsable_weight_is_a_refusal() {
    assert_eq!(quality("gzip;q=abc", "gzip"), Some(0.0));
}

fn tagged(value: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(value) = value {
        headers.insert(
            header::ETAG,
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// A strong tag stops the encoder; a weak one does not.
///
/// RFC 9110 section 8.8.1: a validator shared by a coded and an uncoded
/// representation *is* weak, so a response that already says `W/` is
/// telling the truth after encoding and one that does not is not.
#[test]
fn only_a_strong_validator_stops_the_encoder() {
    let cases: &[(&str, Option<&str>, bool)] = &[
        ("no validator at all", None, false),
        ("a strong tag", Some("\"rev-42\""), true),
        ("a weak tag", Some("W/\"rev-42\""), false),
        // Lowercase `w/` is not the weakness prefix: RFC 9110 section 8.8.3
        // writes it `W/`, case-sensitively.
        ("a lowercase weakness prefix", Some("w/\"rev-42\""), true),
        (
            "a strong tag with surrounding space",
            Some("  \"rev-42\"  "),
            true,
        ),
    ];

    for (description, tag, expected) in cases {
        assert_eq!(strongly_tagged(&tagged(*tag)), *expected, "{description}");
    }
}
