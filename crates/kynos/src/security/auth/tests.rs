use kynos_openapi::SecurityRequirement;

use super::{Auth, Scoped, Scopes, component_name};
use crate::{
    extract::describe::Describe,
    router::operation::OperationCx,
    schema::registry::Registry,
    security::schemes::{Basic, Bearer, Credentials, MutualTls},
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
