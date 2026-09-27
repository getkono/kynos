use super::{identity_quality, preferred, quality};

/// The aliases the specification asks a recipient to honour.
#[test]
fn x_gzip_is_gzip() {
    assert_eq!(quality("x-gzip", "gzip"), Some(1_000));
    assert_eq!(quality("X-GZIP;q=0.5", "gzip"), Some(500));
    // And no other coding has one.
    assert_eq!(quality("x-br", "br"), None);
}

/// Not mentioned and mentioned-then-refused are different answers.
#[test]
fn absence_and_refusal_are_distinguishable() {
    assert_eq!(quality("gzip", "br"), None);
    assert_eq!(quality("br;q=0", "br"), Some(0));
}

/// A weight RFC 9110 section 12.4.2 cannot express is a refusal, not a number.
///
/// The grammar is the one `Accept` and `Accept-Language` read, so a value
/// above 1 is refused rather than clamped: it must not outrank a legitimate
/// `q=1`, and a client that wrote it did not write a qvalue.
#[test]
fn a_weight_that_is_not_a_qvalue_is_a_refusal() {
    for field in [
        "gzip;q=NaN",    // a float, not a qvalue
        "gzip;q=inf",    // nor is infinity
        "gzip;q=1e-1",   // an exponent the grammar has no room for
        "gzip;q=0.0001", // a fourth decimal place
        "gzip;q=-0.5",   // a sign
        "gzip;q=1.5",    // above the bound
        "gzip;q=abc",    // not a number at all
    ] {
        assert_eq!(quality(field, "gzip"), Some(0), "{field}");
    }

    // A decimal point with no digits after it is still a qvalue.
    assert_eq!(quality("gzip;q=0.", "gzip"), Some(0));
    assert_eq!(quality("gzip;q=1.", "gzip"), Some(1_000));
    assert_eq!(quality("gzip;q=0.5", "gzip"), Some(500));
}

#[test]
fn a_wildcard_answers_for_anything_not_named() {
    assert_eq!(quality("*;q=0.3", "br"), Some(300));
    // A specific entry wins over the wildcard.
    assert_eq!(quality("br;q=0.9, *;q=0.3", "br"), Some(900));
}

/// A tie goes to the coding, which is what plain `Accept-Encoding: gzip` means.
#[test]
fn a_tie_goes_to_the_encoded_coding() {
    assert_eq!(preferred("gzip", &["gzip"]), Some("gzip"));
}

/// Preferring identity outright is honoured.
#[test]
fn identity_preferred_more_strongly_wins() {
    assert_eq!(preferred("gzip;q=0.5, identity;q=1.0", &["gzip"]), None);
}

/// Only what is on offer can be chosen.
#[test]
fn a_coding_that_is_not_available_is_not_chosen() {
    assert_eq!(preferred("br", &["gzip"]), None);
}

/// Among what is offered, the client's own weights decide.
#[test]
fn the_clients_preference_orders_the_available_codings() {
    assert_eq!(preferred("gzip;q=0.5, br", &["gzip", "br"]), Some("br"));
    assert_eq!(preferred("gzip, br;q=0.5", &["gzip", "br"]), Some("gzip"));
}

/// Identity's default weight is 1, so a downweighted coding does not beat it.
///
/// The case that reads as a bug and is not. `Accept-Encoding: br;q=0.9` names
/// no weight for identity, so section 12.5.3 rule 2 gives it 1 — and the client
/// has therefore said it prefers the unencoded representation. Sending `br`
/// would be answering a preference the client did not express.
#[test]
fn a_downweighted_coding_loses_to_identitys_default() {
    assert_eq!(preferred("br;q=0.9", &["br"]), None);
    // Unless identity is downweighted too, or excluded outright.
    assert_eq!(preferred("br;q=0.9, identity;q=0.5", &["br"]), Some("br"));
    assert_eq!(preferred("br;q=0.9, identity;q=0", &["br"]), Some("br"));
}

/// A caller states its own preference by ordering `available`.
#[test]
fn a_tie_between_codings_goes_to_the_callers_order() {
    assert_eq!(preferred("gzip, br", &["br", "gzip"]), Some("br"));
    assert_eq!(preferred("gzip, br", &["gzip", "br"]), Some("gzip"));
}

#[test]
fn identity_is_acceptable_unless_it_is_excluded() {
    assert_eq!(identity_quality("gzip"), 1_000);
    assert_eq!(identity_quality("identity;q=0"), 0);
    assert_eq!(identity_quality("*;q=0"), 0);
    // A more specific entry beats the wildcard.
    assert_eq!(identity_quality("*;q=0, identity;q=1"), 1_000);
}

/// An identity weight that is not a qvalue does not exclude identity.
///
/// RFC 9110 section 12.5.3 rule 2 excludes identity only on an explicit
/// `identity;q=0` or `*;q=0`, and `identity;q=1.5` states neither, so identity
/// keeps its default. It is still the more specific entry, so a wildcard
/// refusal beside it does not reach identity either.
#[test]
fn a_malformed_identity_weight_does_not_exclude_identity() {
    for field in ["identity;q=1.5", "identity;q=inf", "identity;q=NaN"] {
        assert_eq!(identity_quality(field), 1_000, "{field}");
    }
    assert_eq!(identity_quality("identity;q=1.5, *;q=0"), 1_000);
}
