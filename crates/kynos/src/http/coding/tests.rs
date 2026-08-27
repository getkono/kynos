use super::{identity_quality, preferred, quality};

/// The aliases the specification asks a recipient to honour.
#[test]
fn x_gzip_is_gzip() {
    assert_eq!(quality("x-gzip", "gzip"), Some(1.0));
    assert_eq!(quality("X-GZIP;q=0.5", "gzip"), Some(0.5));
    // And no other coding has one.
    assert_eq!(quality("x-br", "br"), None);
}

/// Not mentioned and mentioned-then-refused are different answers.
#[test]
fn absence_and_refusal_are_distinguishable() {
    assert_eq!(quality("gzip", "br"), None);
    assert_eq!(quality("br;q=0", "br"), Some(0.0));
}

/// A qvalue above 1 is not a qvalue, and must not outrank a legitimate one.
#[test]
fn an_out_of_range_weight_is_clamped_rather_than_believed() {
    assert_eq!(quality("gzip;q=1.5", "gzip"), Some(1.0));
    assert_eq!(quality("gzip;q=-1", "gzip"), Some(0.0));
    // Unparsable is a refusal: the client wrote something it did not mean.
    assert_eq!(quality("gzip;q=abc", "gzip"), Some(0.0));
}

#[test]
fn a_wildcard_answers_for_anything_not_named() {
    assert_eq!(quality("*;q=0.3", "br"), Some(0.3));
    // A specific entry wins over the wildcard.
    assert_eq!(quality("br;q=0.9, *;q=0.3", "br"), Some(0.9));
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

/// The values compared are literals parsed from literals, so equality here is
/// exact: `float_cmp` is warning about a class of bug this cannot be an
/// instance of.
#[allow(clippy::float_cmp)]
#[test]
fn identity_is_acceptable_unless_it_is_excluded() {
    assert_eq!(identity_quality("gzip"), 1.0);
    assert_eq!(identity_quality("identity;q=0"), 0.0);
    assert_eq!(identity_quality("*;q=0"), 0.0);
    // A more specific entry beats the wildcard.
    assert_eq!(identity_quality("*;q=0, identity;q=1"), 1.0);
}
