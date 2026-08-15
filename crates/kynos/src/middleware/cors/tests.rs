use super::{CorsConfig, CorsDocumentation, Documented, Undocumented};
use crate::extract::params::header::HeaderParams;

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
