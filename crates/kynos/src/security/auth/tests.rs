use std::{sync::Mutex, time::Duration};

use kynos_openapi::SecurityRequirement;

use super::{Auth, MaybeAuth, Scoped, Scopes};
use crate::{
    error::rejection::AuthRejection,
    extract::describe::Describe,
    http::{HeaderValue, Parts, Request, StatusCode, body::Body, header::AUTHORIZATION},
    router::operation::OperationCx,
    schema::registry::Registry,
    security::{
        Authenticates, Authenticator, Guard, Scope, SecurityScheme,
        carrier::{self, BearerToken, Carries, Credentials},
        requirement::{AllOf, AnyOf, Either2, component_name},
        schemes::{Basic, Bearer, MutualTls},
    },
};

/// The scopes the schemes below may demand.
#[derive(Clone, Copy)]
enum Grant {
    ReadReports,
    WriteReports,
    Audit,
}

impl Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::ReadReports => "reports:read",
            Self::WriteReports => "reports:write",
            Self::Audit => "audit",
        }
    }
}

/// A scope set demanded by an operation rather than published by a scheme.
struct ReadReports;

impl Scopes for ReadReports {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
}

/// A scope set naming one scope `Audited`'s defaults already name and one
/// they do not.
struct EditReports;

impl Scopes for EditReports {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::ReadReports, Grant::WriteReports];
}

/// A scope set requiring nothing.
struct NoScopes;

impl Scopes for NoScopes {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[];
}

/// A scheme demanding `audit` and `reports:read` of every operation by default.
struct Audited;

impl SecurityScheme for Audited {
    const NAME: &'static str = "Audited";
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn scopes() -> &'static [Grant] {
        &[Grant::Audit, Grant::ReadReports]
    }

    fn insufficient_scope_challenge() -> Option<&'static str> {
        <Bearer as SecurityScheme>::insufficient_scope_challenge()
    }
}

impl Carries for Audited {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
        carrier::bearer(parts)
    }
}

/// Authenticates every token and answers `authorize` with `admit`, keeping the
/// scope names each `authorize` call was handed, in call order.
struct Recorder {
    admit: bool,
    authorized: Mutex<Vec<Vec<&'static str>>>,
}

impl Recorder {
    /// What `authorize` was handed so far.
    fn authorized(&self) -> Vec<Vec<&'static str>> {
        self.authorized.lock().expect("an unpoisoned lock").clone()
    }

    /// Records `scopes` and answers.
    fn answer<P: Scope>(&self, scopes: &[P]) -> Result<(), AuthRejection> {
        self.authorized
            .lock()
            .expect("an unpoisoned lock")
            .push(scopes.iter().map(Scope::name).collect());
        if self.admit {
            Ok(())
        } else {
            Err(AuthRejection::forbidden())
        }
    }
}

impl<S, C> Authenticator<S, C> for Recorder
where
    S: Carries<Credential = String>,
    C: Sync,
{
    async fn authenticate(&self, _: S::Presented, _: &C) -> Result<String, AuthRejection> {
        Ok("caller".to_owned())
    }

    async fn authorize(&self, _: &String, scopes: &[S::Scope], _: &C) -> Result<(), AuthRejection> {
        self.answer(scopes)
    }
}

/// A context verifying `Audited` and the unscoped `Bearer` through one
/// [`Recorder`].
struct Recording(Recorder);

impl Recording {
    fn admitting() -> Self {
        Self(Recorder {
            admit: true,
            authorized: Mutex::new(Vec::new()),
        })
    }

    fn refusing() -> Self {
        Self(Recorder {
            admit: false,
            authorized: Mutex::new(Vec::new()),
        })
    }
}

impl Authenticates<Audited> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

impl Authenticates<Bearer> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// A scheme reading the bearer token every request below presents, which its
/// authenticator never accepts.
struct Revoked;

impl SecurityScheme for Revoked {
    const NAME: &'static str = "Revoked";
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }
}

impl Carries for Revoked {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
        carrier::bearer(parts)
    }
}

/// Refuses every credential as unauthenticated.
struct Rejecting;

impl<C: Sync> Authenticator<Revoked, C> for Rejecting {
    async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }

    async fn authorize(&self, _: &String, _: &[Grant], _: &C) -> Result<(), AuthRejection> {
        Ok(())
    }
}

impl Authenticates<Revoked> for Recording {
    type Authenticator = Rejecting;

    fn authenticator(&self) -> &Rejecting {
        &Rejecting
    }
}

/// A request head presenting `Authorization: Bearer t`, which every scheme
/// above reads.
fn presenting_a_token() -> Parts {
    let mut request = Request::new(Body::empty());
    request
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_static("Bearer t"));
    request.into_parts().0
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

/// A scheme declaring no challenge advertises none, rather than one its 401
/// does not send.
///
/// The control for the case above: without it, that test would pass against
/// a `declare` that attached a challenge to every scheme.
#[test]
fn a_scheme_with_no_challenge_declares_no_www_authenticate() {
    let (operation, _) = described::<Auth<Audited>>();

    assert!(operation.responses.responses.contains_key("401"));
    assert!(
        !operation.responses.responses["401"]
            .as_item()
            .expect("an inline 401")
            .headers
            .contains_key("WWW-Authenticate")
    );
}

/// Every guard naming a scheme demands its default scopes; `Scoped` adds the
/// operation's own on top of them.
///
/// `Auth` demands the scheme's defaults alone, and `Scoped` demands those
/// defaults followed by each operation scope they do not already name.
#[test]
fn the_scopes_declared_are_the_ones_the_guard_demands() {
    let (bare, _) = described::<Auth<Bearer>>();
    let (scoped, _) = described::<Scoped<Bearer<String, Grant>, ReadReports>>();

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

    // A scheme's defaults come first and a scope both name is listed once,
    // spelled by `Scope::name` on both sides.
    let (audited, _) = described::<Scoped<Audited, ReadReports>>();
    assert_eq!(
        demanded(&audited),
        ["audit".to_owned(), "reports:read".to_owned()]
    );
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

/// A guard with no scope set of its own still declares its scheme's default
/// scopes, in the scheme's order and spelled by `Scope::name`, whether it
/// names the scheme alone or inside a combination.
#[test]
fn a_scheme_declares_its_default_scopes_under_every_guard() {
    let defaults = || {
        SecurityRequirement::scoped(
            component_name::<Audited>().as_str(),
            ["audit", "reports:read"],
        )
    };

    let (alone, _) = described::<Auth<Audited>>();
    assert_eq!(alone.security, Some(vec![defaults()]));

    let (either, _) = described::<Auth<AnyOf<(Audited, Bearer)>>>();
    assert_eq!(either.security, Some(vec![defaults(), bare::<Bearer>()]));

    let (both, _) = described::<Auth<AllOf<(Audited, Bearer)>>>();
    let together = defaults().and(component_name::<Bearer>().as_str(), Vec::<String>::new());
    assert_eq!(both.security, Some(vec![together]));
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
/// scheme declaring none leading the tuple does not leave the 401 without one.
#[test]
fn a_combination_challenges_with_its_first_scheme_that_has_one() {
    let (operation, _) = described::<Auth<AnyOf<(Audited, Bearer)>>>();

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

impl Authenticates<Basic<String>> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// RFC 7617 section 2: "The authentication parameter 'realm' is REQUIRED", so
/// a `Basic` guard's 401 names one, on the wire and in the description alike.
#[tokio::test]
async fn a_basic_challenge_names_a_realm() {
    let anonymous = Request::new(Body::empty()).into_parts().0;
    let Err(refused) =
        <Auth<Basic<String>> as Guard<_>>::guard(&anonymous, &Recording::admitting()).await
    else {
        panic!("an anonymous request is refused");
    };
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);

    let sent = refused.challenge().expect("a 401 carries a challenge");
    let parameters = sent.strip_prefix("Basic ").expect("a Basic challenge");
    assert!(
        parameters
            .split(',')
            .any(|parameter| parameter.trim_start().starts_with("realm=\"")),
        "{sent}"
    );

    let (operation, _) = described::<Auth<Basic<String>>>();
    let header = serde_json::to_value(
        operation.responses.responses["401"]
            .as_item()
            .expect("an inline 401")
            .headers
            .get("WWW-Authenticate")
            .expect("a challenge"),
    )
    .expect("a serializable header");
    assert_eq!(header["example"], serde_json::json!(sent), "{header}");
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

/// What a guard declares of a scheme's default scopes it hands to
/// `authorize`, so a caller lacking them is a 403 under every guard naming the
/// scheme, alone or combined.
#[tokio::test]
async fn every_guard_authorizes_the_default_scopes_it_declares() {
    let parts = presenting_a_token();
    let defaults = vec![vec!["audit", "reports:read"]];

    let refusing = Recording::refusing();
    let refused = <Auth<Audited> as Guard<_>>::guard(&parts, &refusing).await;
    assert_eq!(
        refused.err().map(|r| r.status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(refusing.0.authorized(), defaults);

    let refusing = Recording::refusing();
    let refused = <MaybeAuth<Audited> as Guard<_>>::guard(&parts, &refusing).await;
    assert_eq!(
        refused.err().map(|r| r.status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(refusing.0.authorized(), defaults);

    // Both schemes authenticate the one token; only `Audited` demands scopes.
    let refusing = Recording::refusing();
    let refused = <Auth<AllOf<(Audited, Bearer)>> as Guard<_>>::guard(&parts, &refusing).await;
    assert_eq!(
        refused.err().map(|r| r.status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(refusing.0.authorized(), defaults);

    let admitting = Recording::admitting();
    let admitted = <Auth<AllOf<(Audited, Bearer)>> as Guard<_>>::guard(&parts, &admitting).await;
    assert!(admitted.is_ok());
    assert_eq!(admitting.0.authorized(), defaults);
}

/// `AllOf` authenticates every scheme before authorizing any, so a credential
/// that fails to authenticate answers 401 even where an earlier scheme's
/// default scopes would be refused, and `authorize` never runs for a request
/// that is not authenticated.
#[tokio::test]
async fn all_of_authenticates_every_scheme_before_authorizing_any() {
    let refusing = Recording::refusing();
    let refused =
        <Auth<AllOf<(Audited, Revoked)>> as Guard<_>>::guard(&presenting_a_token(), &refusing)
            .await;

    assert_eq!(
        refused.err().map(|r| r.status()),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert!(refusing.0.authorized().is_empty());
}

/// A scheme whose carrier is optional, as `carrier = optional` writes one:
/// absence reaches the authenticator as `Some(None)`.
struct OptionalBearer;

impl SecurityScheme for OptionalBearer {
    const NAME: &'static str = "OptionalBearer";
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }
}

impl Carries for OptionalBearer {
    type Presented = Option<BearerToken>;

    fn present(parts: &Parts) -> Result<Option<Option<BearerToken>>, AuthRejection> {
        carrier::bearer(parts).map(Some)
    }
}

impl Authenticates<OptionalBearer> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// Under an optional carrier nothing reports absence, so an `AllOf` holding
/// one treats a request presenting nothing as a partial set: `MaybeAuth`
/// answers it 401 rather than as anonymous, as `docs/security.md` states.
#[tokio::test]
async fn maybe_auth_over_all_of_with_an_optional_carrier_refuses_a_bare_request() {
    let admitting = Recording::admitting();
    let guarded = <MaybeAuth<AllOf<(OptionalBearer, Bearer)>> as Guard<_>>::guard(
        &Request::new(Body::empty()).into_parts().0,
        &admitting,
    )
    .await;

    assert_eq!(
        guarded.err().map(|r| r.status()),
        Some(StatusCode::UNAUTHORIZED)
    );
}

/// An `AnyOf` alternative whose default scopes are refused is not satisfied,
/// so the next alternative is tried rather than the refused one admitted.
#[tokio::test]
async fn any_of_moves_past_an_alternative_whose_default_scopes_are_refused() {
    let refusing = Recording::refusing();
    let guarded =
        <Auth<AnyOf<(Audited, Bearer)>> as Guard<_>>::guard(&presenting_a_token(), &refusing).await;

    assert!(
        matches!(guarded.map(Auth::into_inner), Ok(Either2::Second(_))),
        "`Audited` was admitted although its default scopes were refused"
    );
    assert_eq!(refusing.0.authorized(), [vec!["audit", "reports:read"]]);
}

/// A scheme demanding no default scopes is authenticated and never authorized
/// under a guard naming no scope set, so such a guard's 403 surface stays
/// `authenticate`'s alone.
#[tokio::test]
async fn a_scheme_with_no_default_scopes_is_not_authorized() {
    let refusing = Recording::refusing();
    let guarded = <Auth<Bearer> as Guard<_>>::guard(&presenting_a_token(), &refusing).await;

    assert!(guarded.is_ok());
    assert!(refusing.0.authorized().is_empty());
}

/// `Scoped` hands `authorize` the scopes it declares: the scheme's defaults
/// first, then its own set's, each once, in one call.
#[tokio::test]
async fn scoped_authorizes_the_defaults_and_its_set_together() {
    let admitting = Recording::admitting();
    let guarded =
        <Scoped<Audited, ReadReports> as Guard<_>>::guard(&presenting_a_token(), &admitting).await;

    assert!(guarded.is_ok());
    assert_eq!(admitting.0.authorized(), [vec!["audit", "reports:read"]]);
}

/// A `Scoped` set's scope the defaults do not name joins them, after the
/// defaults and once, so `Scoped` still authorizes what its own set requires.
#[tokio::test]
async fn scoped_adds_its_own_scopes_to_the_defaults() {
    let admitting = Recording::admitting();
    let guarded =
        <Scoped<Audited, EditReports> as Guard<_>>::guard(&presenting_a_token(), &admitting).await;

    assert!(guarded.is_ok());
    assert_eq!(
        admitting.0.authorized(),
        [vec!["audit", "reports:read", "reports:write"]]
    );
}

/// A `Scoped` whose demanded set is empty still calls `authorize`, once and
/// with no scopes, and its refusal answers 403: unlike `Auth`, `Scoped` never
/// skips `authorize`.
#[tokio::test]
async fn scoped_with_an_empty_demand_still_authorizes() {
    let refusing = Recording::refusing();
    let refused = <Scoped<Bearer<String, Grant>, NoScopes> as Guard<_>>::guard(
        &presenting_a_token(),
        &refusing,
    )
    .await;

    assert_eq!(
        refused.err().map(|r| r.into_inner().status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(refusing.0.authorized(), [Vec::<&str>::new()]);
}

impl Authenticates<Bearer<String, Grant>> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

impl Authenticates<MutualTls<String>> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// A scheme whose challenge would splice a second header field into the
/// response, were it written as it is.
struct Spliced;

impl SecurityScheme for Spliced {
    const NAME: &'static str = "Spliced";
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn challenge() -> Option<&'static str> {
        Some("Bearer\r\nX-Injected: 1")
    }
}

impl Carries for Spliced {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
        carrier::bearer(parts)
    }
}

impl Authenticates<Spliced> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// The `WWW-Authenticate` values on the response guard `G` refuses `parts`
/// with, which must be a 401.
async fn challenges_sent<G: Guard<Recording>>(parts: &Parts) -> Vec<String> {
    use crate::response::IntoResponse as _;

    let Err(refused) = G::guard(parts, &Recording::admitting()).await else {
        panic!("the request is refused");
    };
    let response = refused.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    response
        .headers()
        .get_all(crate::http::header::WWW_AUTHENTICATE)
        .iter()
        .map(|value| value.to_str().expect("a visible challenge").to_owned())
        .collect()
}

/// RFC 9110 section 11.6.1: a 401 sends `WWW-Authenticate`. What each built-in
/// scheme sends on the wire, under each guard that attaches it, is exactly
/// the challenge it declares, or nothing where it declares none.
///
/// `MutualTls` has no registered HTTP authentication scheme, so its 401 names
/// the unregistered `MutualTLS`, and an `AnyOf` it leads challenges with it as
/// the first in tuple order. A scheme declaring none sends none, and an `AnyOf`
/// it leads challenges with the first scheme after it that has one.
#[tokio::test]
async fn every_built_in_scheme_sends_its_own_challenge_on_a_401() {
    let anonymous = Request::new(Body::empty()).into_parts().0;
    let basic = r#"Basic realm="api", charset="UTF-8""#;

    assert_eq!(
        challenges_sent::<Auth<Bearer>>(&anonymous).await,
        ["Bearer"]
    );
    assert_eq!(
        challenges_sent::<Auth<Basic<String>>>(&anonymous).await,
        [basic]
    );
    assert_eq!(
        challenges_sent::<Auth<MutualTls<String>>>(&anonymous).await,
        ["MutualTLS"]
    );
    assert_eq!(
        challenges_sent::<Auth<AnyOf<(MutualTls<String>, Bearer)>>>(&anonymous).await,
        ["MutualTLS"]
    );
    assert!(
        challenges_sent::<Auth<Audited>>(&anonymous)
            .await
            .is_empty()
    );
    assert_eq!(
        challenges_sent::<Auth<AnyOf<(Audited, Bearer)>>>(&anonymous).await,
        ["Bearer"]
    );
    assert_eq!(
        challenges_sent::<Scoped<Bearer<String, Grant>, ReadReports>>(&anonymous).await,
        ["Bearer"]
    );

    // `MaybeAuth` admits absence, so its 401 is a credential present and
    // unreadable: a Basic credential where a bearer token is carried.
    let mut request = Request::new(Body::empty());
    request
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_static("Basic dTpw"));
    let unreadable = request.into_parts().0;
    assert_eq!(
        challenges_sent::<MaybeAuth<Bearer>>(&unreadable).await,
        ["Bearer"]
    );
}

/// A challenge that is no field value is dropped from the 401 rather than
/// written or panicked on, and from the description alike, so the two never
/// disagree about whether one is sent.
#[tokio::test]
async fn a_challenge_that_is_no_field_value_is_neither_sent_nor_declared() {
    let anonymous = Request::new(Body::empty()).into_parts().0;
    assert!(
        challenges_sent::<Auth<Spliced>>(&anonymous)
            .await
            .is_empty()
    );

    let (operation, _) = described::<Auth<Spliced>>();
    assert!(
        !operation.responses.responses["401"]
            .as_item()
            .expect("an inline 401")
            .headers
            .contains_key("WWW-Authenticate")
    );
}

// --- A scope refusal's challenge --------------------------------------------

impl Authenticates<Basic<String, Grant>> for Recording {
    type Authenticator = Recorder;

    fn authenticator(&self) -> &Recorder {
        &self.0
    }
}

/// Declines every bearer token in `authenticate`, as a suspended account is: a
/// valid credential refused, not a scope.
struct Suspending;

impl<C: Sync> Authenticator<Bearer<String, Grant>, C> for Suspending {
    async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
        Err(AuthRejection::forbidden())
    }

    async fn authorize(&self, _: &String, _: &[Grant], _: &C) -> Result<(), AuthRejection> {
        Ok(())
    }
}

/// A context whose authenticator declines every credential before any scope
/// is checked.
struct Suspended;

impl Authenticates<Bearer<String, Grant>> for Suspended {
    type Authenticator = Suspending;

    fn authenticator(&self) -> &Suspending {
        &Suspending
    }
}

/// The `WWW-Authenticate` values on the response guard `G` refuses `parts`
/// with under `context`, which must be a 403.
async fn forbidden_challenges<C: Sync, G: Guard<C>>(parts: &Parts, context: &C) -> Vec<String> {
    use crate::response::IntoResponse as _;

    let Err(refused) = G::guard(parts, context).await else {
        panic!("the request is refused");
    };
    let response = refused.into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    response
        .headers()
        .get_all(crate::http::header::WWW_AUTHENTICATE)
        .iter()
        .map(|value| value.to_str().expect("a visible challenge").to_owned())
        .collect()
}

/// The `WWW-Authenticate` an operation guarded by `D` declares on its 403, as
/// JSON, if it declares one.
fn declared_scope_challenge<D: Describe>() -> Option<serde_json::Value> {
    let (operation, _) = described::<D>();
    operation.responses.responses["403"]
        .as_item()
        .expect("an inline 403")
        .headers
        .get("WWW-Authenticate")
        .map(|header| serde_json::to_value(header).expect("a serializable header"))
}

/// A request head presenting `Authorization: Basic dTpw`, `u:p`.
fn presenting_credentials() -> Parts {
    let mut request = Request::new(Body::empty());
    request
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_static("Basic dTpw"));
    request.into_parts().0
}

/// RFC 6750 section 3: a resource server MUST send `WWW-Authenticate` when a
/// bearer token does not enable access, and section 3.1 names
/// `insufficient_scope` for the 403 a missing scope earns. A `Bearer` scope
/// refusal sends exactly that, and a `Basic` one, which no such error is
/// registered for, sends nothing.
#[tokio::test]
async fn a_bearer_scope_refusal_challenges_with_insufficient_scope() {
    let refusing = Recording::refusing();

    assert_eq!(
        forbidden_challenges::<_, Scoped<Bearer<String, Grant>, ReadReports>>(
            &presenting_a_token(),
            &refusing,
        )
        .await,
        [r#"Bearer error="insufficient_scope""#]
    );
    assert!(
        forbidden_challenges::<_, Scoped<Basic<String, Grant>, ReadReports>>(
            &presenting_credentials(),
            &refusing,
        )
        .await
        .is_empty()
    );
}

/// A 403 from `authenticate` declines the credential, not a scope, so even a
/// bearer scheme's carries no challenge: `insufficient_scope` would send the
/// client off for a token it could never be granted.
#[tokio::test]
async fn a_credential_declined_before_any_scope_sends_no_challenge() {
    assert!(
        forbidden_challenges::<_, Scoped<Bearer<String, Grant>, ReadReports>>(
            &presenting_a_token(),
            &Suspended,
        )
        .await
        .is_empty()
    );
}

/// The description says what the wire sends: a `Bearer` guard that refuses
/// scopes declares the challenge on its 403, not required, since a 403 from
/// `authenticate` sends none. `Auth<Bearer>` authorizes no scope, and `Basic`
/// has no challenge for one, so neither declares it.
#[test]
fn a_scope_refusal_declares_the_challenge_it_sends() {
    let header = declared_scope_challenge::<Scoped<Bearer<String, Grant>, ReadReports>>()
        .expect("a declared challenge");
    assert_eq!(
        header["example"],
        serde_json::json!(r#"Bearer error="insufficient_scope""#),
        "{header}"
    );
    assert_ne!(header["required"], serde_json::json!(true), "{header}");

    assert_eq!(declared_scope_challenge::<Auth<Bearer>>(), None);
    assert_eq!(
        declared_scope_challenge::<Scoped<Basic<String, Grant>, ReadReports>>(),
        None
    );
}

/// A guard naming no scope set refuses a scope only through a scheme's default
/// scopes, so that scheme's challenge is the one sent and declared, wherever
/// it sits in a combination.
#[tokio::test]
async fn a_default_scope_refusal_challenges_with_the_refusing_schemes_challenge() {
    const INSUFFICIENT: &str = r#"Bearer error="insufficient_scope""#;
    let refusing = Recording::refusing();

    assert_eq!(
        forbidden_challenges::<_, Auth<Audited>>(&presenting_a_token(), &refusing).await,
        [INSUFFICIENT]
    );
    assert_eq!(
        forbidden_challenges::<_, Auth<AllOf<(Bearer, Audited)>>>(&presenting_a_token(), &refusing)
            .await,
        [INSUFFICIENT]
    );

    // `Bearer` leads, but owes no scope, so it is `Audited`'s that is declared.
    for declared in [
        declared_scope_challenge::<Auth<Audited>>(),
        declared_scope_challenge::<MaybeAuth<Audited>>(),
        declared_scope_challenge::<Auth<AnyOf<(Bearer, Audited)>>>(),
        declared_scope_challenge::<Auth<AllOf<(Bearer, Audited)>>>(),
    ] {
        let header = declared.expect("a declared challenge");
        assert_eq!(
            header["example"],
            serde_json::json!(INSUFFICIENT),
            "{header}"
        );
    }
}

/// A bearer scheme whose credential store can be unreachable.
struct Outage;

impl SecurityScheme for Outage {
    const NAME: &'static str = "Outage";
    const MAY_BE_UNAVAILABLE: bool = true;
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn challenge() -> Option<&'static str> {
        Some("Bearer")
    }
}

impl Carries for Outage {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Result<Option<BearerToken>, AuthRejection> {
        carrier::bearer(parts)
    }
}

/// Cannot reach its store, and expects it back in a second and a half.
struct Down;

impl<C: Sync> Authenticator<Outage, C> for Down {
    async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
        Err(AuthRejection::unavailable::<Outage>().with_retry_after(Duration::from_millis(1_500)))
    }

    async fn authorize(&self, _: &String, _: &[Grant], _: &C) -> Result<(), AuthRejection> {
        Ok(())
    }
}

impl Authenticates<Outage> for Recording {
    type Authenticator = Down;

    fn authenticator(&self) -> &Down {
        &Down
    }
}

/// The 503 guard `D`'s operation declares, if it declares one.
fn declared_unavailable<D: Describe>() -> Option<kynos_openapi::Response> {
    let (operation, _) = described::<D>();
    operation
        .responses
        .responses
        .get("503")
        .map(|response| response.as_item().expect("an inline 503").clone())
}

/// The opt-in is the scheme's, and every guard naming it declares the 503 its
/// authenticator may send: alone, anonymously, scoped, or as either half of a
/// combination, since each scheme's authenticator is consulted. The 503 is the
/// `about:blank` problem `Problem::new` writes, with the optional
/// `Retry-After` it may carry.
#[test]
fn a_scheme_that_may_be_unavailable_declares_the_503_under_every_guard() {
    for declared in [
        declared_unavailable::<Auth<Outage>>(),
        declared_unavailable::<MaybeAuth<Outage>>(),
        declared_unavailable::<Scoped<Outage, ReadReports>>(),
        declared_unavailable::<Auth<AnyOf<(Bearer, Outage)>>>(),
        declared_unavailable::<Auth<AllOf<(Outage, Bearer)>>>(),
        declared_unavailable::<Scoped<AnyOf<(Bearer<String, Grant>, Outage)>, ReadReports>>(),
    ] {
        let response = serde_json::to_value(declared.expect("a declared 503"))
            .expect("a serializable response");

        assert_eq!(
            response["content"]["application/problem+json"]["schema"]["allOf"][1]["properties"]["type"]
                ["const"],
            serde_json::json!("about:blank"),
            "{response}"
        );
        let retry_after = &response["headers"]["Retry-After"];
        assert!(retry_after.is_object(), "{response}");
        assert_ne!(
            retry_after["required"],
            serde_json::json!(true),
            "{response}"
        );
        // An outage challenges nothing.
        assert!(
            response["headers"].get("WWW-Authenticate").is_none(),
            "{response}"
        );
    }
}

/// The control: a guard none of whose schemes opted in keeps its refusals to
/// 401 and 403, which is what every operation declared before the opt-in.
#[test]
fn a_guard_whose_schemes_never_answer_503_declares_none() {
    assert!(declared_unavailable::<Auth<Bearer>>().is_none());
    assert!(declared_unavailable::<MaybeAuth<Bearer>>().is_none());
    assert!(declared_unavailable::<Scoped<Bearer<String, Grant>, ReadReports>>().is_none());
    assert!(declared_unavailable::<Auth<AnyOf<(Bearer, Audited)>>>().is_none());
    assert!(declared_unavailable::<Auth<AllOf<(Bearer, Audited)>>>().is_none());
    assert!(
        declared_unavailable::<Scoped<AnyOf<(Audited, Bearer<String, Grant>)>, ReadReports>>()
            .is_none()
    );
}

// --- A scope set over a combined requirement ---------------------------------

/// Either `Audited` or a bearer token, each holding `reports:read`.
type EitherReader = Scoped<AnyOf<(Audited, Bearer<String, Grant>)>, ReadReports>;

/// `Scoped` over an `AnyOf` declares one alternative per scheme, each carrying
/// its scheme's defaults followed by the set's scopes they do not name, and
/// registers every scheme.
#[test]
fn scoped_any_of_declares_the_set_on_every_alternative() {
    let (operation, components) = described::<EitherReader>();

    assert_eq!(
        operation.security,
        Some(vec![
            SecurityRequirement::scoped(
                component_name::<Audited>().as_str(),
                ["audit", "reports:read"],
            ),
            SecurityRequirement::scoped(
                component_name::<Bearer<String, Grant>>().as_str(),
                ["reports:read"],
            ),
        ])
    );
    for key in [
        component_name::<Audited>(),
        component_name::<Bearer<String, Grant>>(),
    ] {
        assert!(
            components.security_schemes.contains_key(key.as_str()),
            "{key:?}"
        );
    }

    // Two schemes under one key are one alternative, as under `Auth`.
    let (once, _) =
        described::<Scoped<AnyOf<(Bearer<String, Grant>, Bearer<u64, Grant>)>, ReadReports>>();
    assert_eq!(
        once.security,
        Some(vec![SecurityRequirement::scoped(
            component_name::<Bearer<String, Grant>>().as_str(),
            ["reports:read"],
        )])
    );
}

/// A set naming one scope twice declares it once on every alternative, even
/// where no default scope is there to be joined.
#[test]
fn a_scope_named_twice_is_declared_once() {
    /// `reports:read`, twice.
    struct Twice;

    impl Scopes for Twice {
        type Scope = Grant;
        const REQUIRED: &'static [Grant] = &[Grant::ReadReports, Grant::ReadReports];
    }

    let (operation, _) = described::<Scoped<AnyOf<(Audited, Bearer<String, Grant>)>, Twice>>();
    assert_eq!(
        operation.security,
        Some(vec![
            SecurityRequirement::scoped(
                component_name::<Audited>().as_str(),
                ["audit", "reports:read"],
            ),
            SecurityRequirement::scoped(
                component_name::<Bearer<String, Grant>>().as_str(),
                ["reports:read"],
            ),
        ])
    );
}

/// What refusing a scope set is called narrows the 403 a combined requirement
/// declares exactly as it does a single scheme's.
#[test]
fn scoped_any_of_narrows_the_403_to_the_sets_refusal() {
    /// Names its refusal.
    struct Named;

    impl Scopes for Named {
        type Scope = Grant;
        const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
        const FORBIDDEN_TYPE: Option<&'static str> = Some("https://errors.example.com/no-scope");
    }

    let forbidden = |operation: kynos_openapi::Operation| {
        serde_json::to_value(&operation.responses.responses["403"]).expect("a serializable 403")
    };
    let (single, _) = described::<Scoped<Bearer<String, Grant>, Named>>();
    let (combined, _) = described::<Scoped<AnyOf<(Audited, Bearer<String, Grant>)>, Named>>();
    let (unnamed, _) = described::<EitherReader>();

    let combined = forbidden(combined);
    assert_eq!(combined, forbidden(single));
    assert!(
        combined
            .to_string()
            .contains("https://errors.example.com/no-scope"),
        "{combined}"
    );
    assert_ne!(combined, forbidden(unnamed));
}

/// The set is demanded of whichever alternative is tried: an alternative
/// refused it is not satisfied, so the next is tried with the same set, and
/// the first that holds it wins.
#[tokio::test]
async fn scoped_any_of_demands_the_set_of_each_alternative_tried() {
    let refusing = Recording::refusing();
    let refused = <EitherReader as Guard<_>>::guard(&presenting_a_token(), &refusing).await;
    assert_eq!(
        refused.err().map(|r| r.into_inner().status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        refusing.0.authorized(),
        [vec!["audit", "reports:read"], vec!["reports:read"]]
    );

    // `Revoked` refuses the token outright, so `Bearer` is tried, and holds it.
    let admitting = Recording::admitting();
    let admitted =
        <Scoped<AnyOf<(Revoked, Bearer<String, Grant>)>, ReadReports> as Guard<_>>::guard(
            &presenting_a_token(),
            &admitting,
        )
        .await;
    assert!(
        matches!(
            admitted.map(Scoped::into_inner),
            Ok(Either2::Second(caller)) if caller == "caller"
        ),
        "the bearer alternative was not admitted"
    );
    assert_eq!(admitting.0.authorized(), [vec!["reports:read"]]);
}

/// A request presenting nothing either alternative reads is `Scoped`'s 401,
/// challenged as the combination's first scheme with a challenge, and a scope
/// refusal sends and declares the first scheme's `insufficient_scope`.
#[tokio::test]
async fn scoped_any_of_challenges_as_the_combination() {
    const INSUFFICIENT: &str = r#"Bearer error="insufficient_scope""#;
    let anonymous = Request::new(Body::empty()).into_parts().0;

    assert_eq!(
        challenges_sent::<EitherReader>(&anonymous).await,
        ["Bearer"]
    );
    assert_eq!(
        forbidden_challenges::<_, EitherReader>(&presenting_a_token(), &Recording::refusing())
            .await,
        [INSUFFICIENT]
    );

    let header = declared_scope_challenge::<EitherReader>().expect("a declared challenge");
    assert_eq!(
        header["example"],
        serde_json::json!(INSUFFICIENT),
        "{header}"
    );
}

/// The response guard `G` refuses `parts` with under `context`.
async fn refused<C: Sync, G: Guard<C>>(parts: &Parts, context: &C) -> crate::http::Response {
    use crate::response::IntoResponse as _;

    let Err(refused) = G::guard(parts, context).await else {
        panic!("the request is refused");
    };
    refused.into_response()
}

/// An unreachable store is a 503 under every guard, never the 401 that would
/// tell a client to discard its credential: no guard attaches a challenge to
/// it, and the `Retry-After` it carries is whole seconds, rounded up.
#[tokio::test]
async fn an_unreachable_store_answers_503_rather_than_401() {
    let context = Recording::admitting();
    let parts = presenting_a_token();

    for response in [
        refused::<_, Auth<Outage>>(&parts, &context).await,
        refused::<_, MaybeAuth<Outage>>(&parts, &context).await,
        refused::<_, Scoped<Outage, ReadReports>>(&parts, &context).await,
        refused::<_, Auth<AllOf<(Bearer, Outage)>>>(&parts, &context).await,
        // `Revoked` refuses after the outage, so the first refusal answers.
        refused::<_, Scoped<AnyOf<(Outage, Revoked)>, ReadReports>>(&parts, &context).await,
    ] {
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(crate::http::header::RETRY_AFTER),
            Some(&HeaderValue::from_static("2"))
        );
        assert!(
            !response
                .headers()
                .contains_key(crate::http::header::WWW_AUTHENTICATE)
        );
    }
}
