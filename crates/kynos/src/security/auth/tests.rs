use kynos_openapi::SecurityRequirement;

use super::{Auth, MaybeAuth, Scoped, Scopes};
use crate::{
    extract::describe::Describe,
    router::operation::OperationCx,
    schema::registry::Registry,
    security::{
        carrier::Credentials,
        requirement::{AllOf, AnyOf, component_name},
        schemes::{Basic, Bearer, MutualTls},
    },
};

/// A scope set demanded by an operation rather than published by a scheme.
struct ReadReports;

impl Scopes for ReadReports {
    const SCOPES: &'static [&'static str] = &["reports:read"];
}

/// Describes one operation guarded by `D` and returns what it said.
fn described<D: Describe>() -> (kynos_openapi::Operation, kynos_openapi::Components) {
    let mut registry = Registry::new();
    let mut cx = OperationCx::new(&mut registry);
    D::describe(&mut cx);
    (cx.finish(), registry.into_components())
}

/// A credential is required and described by the same act, so an operation
/// taking one cannot be served without it and cannot be described without
/// saying so. All four halves at once, because leaving any one out is a
/// description that promises something the other three contradict.
#[test]
fn a_guard_declares_the_requirement_the_scheme_the_statuses_and_the_challenge() {
    let (operation, components) = described::<Auth<Bearer>>();

    // The requirement, under the same key the scheme is registered as.
    let name = component_name::<Bearer>();
    assert_eq!(
        operation.security.as_deref(),
        Some(
            &[SecurityRequirement::scoped(
                name.as_str(),
                Vec::<String>::new()
            )][..]
        )
    );

    // The registration, so the requirement names something the document
    // defines rather than a dangling key.
    assert!(
        components.security_schemes.contains_key(name.as_str()),
        "{name:?}"
    );

    // Both statuses the guard can produce.
    assert!(operation.responses.responses.contains_key("401"));
    assert!(operation.responses.responses.contains_key("403"));

    // The challenge, which RFC 9110 section 11.6.1 requires on a 401, and
    // which is the scheme's own string rather than one rebuilt here.
    let unauthorized = operation.responses.responses["401"]
        .as_item()
        .expect("an inline 401");
    assert!(
        unauthorized.headers.contains_key("WWW-Authenticate"),
        "{:?}",
        unauthorized.headers.keys().collect::<Vec<_>>()
    );
}

/// A scheme carried outside the `Authorization` header advertises nothing,
/// because there is no challenge a client could answer.
///
/// The control for the case above: without it, that test would pass against
/// a `declare` that attached a challenge to every scheme.
#[test]
fn a_scheme_with_no_challenge_declares_no_www_authenticate() {
    let (operation, _) = described::<Auth<MutualTls>>();

    assert!(operation.responses.responses.contains_key("401"));
    assert!(
        !operation.responses.responses["401"]
            .as_item()
            .expect("an inline 401")
            .headers
            .contains_key("WWW-Authenticate")
    );
}

/// `Scoped` demands the operation's scopes; `Auth` demands the scheme's.
///
/// Two different questions with two different answers: what an
/// authorization server can grant, and what this endpoint needs.
#[test]
fn the_scopes_declared_are_the_ones_the_guard_demands() {
    let (bare, _) = described::<Auth<Bearer>>();
    let (scoped, _) = described::<Scoped<Bearer, ReadReports>>();

    let demanded = |operation: &kynos_openapi::Operation| {
        operation
            .security
            .as_ref()
            .and_then(|requirements| requirements.first())
            .map(|requirement| {
                requirement
                    .0
                    .values()
                    .flatten()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    assert!(demanded(&bare).is_empty());
    assert_eq!(demanded(&scoped), ["reports:read".to_owned()]);
}

/// One name for both halves, so the scheme a requirement demands and the
/// scheme a document defines cannot be different keys — including for a
/// scheme whose `NAME` is not a legal component key.
#[test]
fn the_requirement_and_the_registration_share_one_key() {
    for (operation, components) in [
        described::<Auth<Bearer>>(),
        described::<Auth<Basic<Credentials>>>(),
        described::<Auth<MutualTls>>(),
    ] {
        let demanded: Vec<String> = operation
            .security
            .expect("a requirement")
            .iter()
            .flat_map(|requirement| requirement.0.keys().cloned())
            .collect();

        for key in demanded {
            assert!(
                components.security_schemes.contains_key(&key),
                "`{key}` is demanded and never defined"
            );
        }
    }
}

/// The requirement naming `S` with no scopes, under its registered key.
fn bare<S: crate::security::SecurityScheme>() -> SecurityRequirement {
    SecurityRequirement::scheme(component_name::<S>().as_str())
}

/// `AnyOf` is OpenAPI's "either": one alternative per scheme, in order, and
/// every scheme registered.
#[test]
fn any_of_declares_one_alternative_per_scheme() {
    let (operation, components) = described::<Auth<AnyOf<(Bearer, Basic<Credentials>)>>>();

    assert_eq!(
        operation.security,
        Some(vec![bare::<Bearer>(), bare::<Basic<Credentials>>()])
    );
    assert!(
        components
            .security_schemes
            .contains_key(component_name::<Bearer>().as_str())
    );
    assert!(
        components
            .security_schemes
            .contains_key(component_name::<Basic<Credentials>>().as_str())
    );
}

/// Two schemes registered under one key are one alternative to a reader, so
/// `AnyOf` lists it once rather than repeating it.
#[test]
fn any_of_lists_an_alternative_once() {
    let (operation, _) = described::<Auth<AnyOf<(Bearer, Bearer<u64>)>>>();

    assert_eq!(operation.security, Some(vec![bare::<Bearer>()]));
}

/// `AllOf` is OpenAPI's "both": a single requirement naming every scheme,
/// which is what the guard enforces when it demands each one.
#[test]
fn all_of_declares_one_requirement_naming_every_scheme() {
    let (operation, components) = described::<Auth<AllOf<(Bearer, MutualTls)>>>();

    let together = SecurityRequirement::scheme(component_name::<Bearer>().as_str())
        .and(component_name::<MutualTls>().as_str(), Vec::<String>::new());
    assert_eq!(operation.security, Some(vec![together]));
    assert!(
        components
            .security_schemes
            .contains_key(component_name::<MutualTls>().as_str())
    );
}

/// `MaybeAuth` prepends the anonymous alternative to whatever its requirement
/// declares, and nothing else: the combination's own alternatives follow
/// unchanged.
#[test]
fn maybe_auth_leads_a_combination_with_the_anonymous_alternative() {
    let (either, _) = described::<MaybeAuth<AnyOf<(Bearer, Basic<Credentials>)>>>();
    assert_eq!(
        either.security,
        Some(vec![
            SecurityRequirement::anonymous(),
            bare::<Bearer>(),
            bare::<Basic<Credentials>>(),
        ])
    );

    let (both, _) = described::<MaybeAuth<AllOf<(Bearer, MutualTls)>>>();
    assert_eq!(
        both.security,
        Some(vec![
            SecurityRequirement::anonymous(),
            SecurityRequirement::scheme(component_name::<Bearer>().as_str())
                .and(component_name::<MutualTls>().as_str(), Vec::<String>::new()),
        ])
    );
}

/// A combination's 401 advertises the first challenge among its schemes, so a
/// scheme carried outside `Authorization` leading the tuple does not leave the
/// 401 without one.
#[test]
fn a_combination_challenges_with_its_first_scheme_that_has_one() {
    let (operation, _) = described::<Auth<AnyOf<(MutualTls, Bearer)>>>();

    let unauthorized = operation.responses.responses["401"]
        .as_item()
        .expect("an inline 401");
    let header = serde_json::to_value(
        unauthorized
            .headers
            .get("WWW-Authenticate")
            .expect("a challenge"),
    )
    .expect("a serializable header");
    assert_eq!(header["example"], serde_json::json!("Bearer"), "{header}");
}

/// The guard sets `security` whole, and a second declaration is refused rather
/// than appended as an alternative it never was.
#[test]
#[should_panic(expected = "its security is already set")]
fn a_second_guard_on_one_operation_is_refused() {
    let mut registry = Registry::new();
    let mut cx = OperationCx::new(&mut registry);
    Auth::<Bearer>::describe(&mut cx);
    Auth::<MutualTls>::describe(&mut cx);
}
