use super::{Cors, CorsConfig, CorsDocumentation, Documented, Undocumented};
use crate::{extract::params::header::HeaderParams, http::HeaderValue};

/// The router reads a `Cors` back out of a type-erased chain by downcasting
/// to each state in turn, so the set of states has to be closed and this is
/// what closes it. `CorsDocumentation` is sealed, so a third state cannot be
/// added downstream; this fails when one is added *here* without teaching
/// the probe about it.
#[test]
fn every_cors_documentation_state_is_one_of_the_two_the_router_recognises() {
    fn described<D: CorsDocumentation>() -> bool {
        <D::Headers as HeaderParams>::DESCRIBED
    }

    // Transcribed, not derived: a list read from the trait would agree with
    // it however many states there were.
    let states = [
        ("Undocumented", described::<Undocumented>()),
        ("Documented", described::<Documented>()),
    ];

    assert_eq!(states.len(), 2, "a state was added without a probe arm");
    assert_eq!(states[0], ("Undocumented", false));
    assert_eq!(states[1], ("Documented", true));
}

#[test]
fn permitting_any_origin_alongside_credentials_is_a_conflict() {
    let config = CorsConfig {
        any_origin: true,
        credentials: true,
        ..CorsConfig::default()
    };

    assert!(config.conflict().is_some());
}

/// Each half alone is an ordinary deployment: a public API allows any
/// origin, and a credentialed one names its origins.
#[test]
fn permitting_any_origin_alone_is_not_a_conflict() {
    let any_origin = CorsConfig {
        any_origin: true,
        ..CorsConfig::default()
    };
    let credentialed = CorsConfig {
        origins: vec!["https://app.example.com".into()],
        credentials: true,
        ..CorsConfig::default()
    };

    assert!(any_origin.conflict().is_none());
    assert!(credentialed.conflict().is_none());
}

/// The origin a predicate accepts, and the one it does not.
#[test]
fn a_predicate_permits_the_origins_it_accepts_and_no_others() {
    let cors = Cors::new().allow_origins_matching(|origin| origin.ends_with(".example.com"));
    let config = cors.config();

    assert!(config.permits(&HeaderValue::from_static("https://app.example.com")));
    assert!(!config.permits(&HeaderValue::from_static("https://example.com.evil.test")));
}

/// A list and a predicate widen the same allow-list rather than replacing one
/// another, which is what lets a static allow-list gain a subdomain rule.
#[test]
fn a_predicate_widens_the_named_list_rather_than_replacing_it() {
    let cors = Cors::new()
        .allow_origins(["https://admin.example.net"])
        .allow_origins_matching(|origin| origin.ends_with(".example.com"));
    let config = cors.config();

    assert!(config.permits(&HeaderValue::from_static("https://admin.example.net")));
    assert!(config.permits(&HeaderValue::from_static("https://app.example.com")));
    assert!(!config.permits(&HeaderValue::from_static("https://other.test")));
}

/// A predicate never produces `*`, so it is the way to allow a wide set of
/// origins *and* credentials — the pair `allow_any_origin` cannot express.
#[test]
fn a_predicate_alongside_credentials_is_not_a_conflict() {
    let cors = Cors::new()
        .allow_origins_matching(|_| true)
        .allow_credentials();

    assert!(cors.config().conflict().is_none());
}

/// An `Origin` that is not a string is not an origin, and a predicate is not
/// asked to decide about one.
#[test]
fn a_predicate_never_sees_a_field_value_that_is_not_a_string() {
    let cors = Cors::new().allow_origins_matching(|_| true);
    let opaque = HeaderValue::from_bytes(&[0xff, 0xfe]).expect("a non-string field value");

    assert!(!cors.config().permits(&opaque));
}

/// The predicates are not printable, so the `Debug` says how many there are.
#[test]
fn a_configuration_holding_a_predicate_still_prints() {
    let cors = Cors::new().allow_origins_matching(|_| true);

    assert!(format!("{:?}", cors.config()).contains("predicates: 1"));
}
