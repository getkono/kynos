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
        Authenticates, Authenticator, Authorizer, Guard, Head, Scope, SecurityScheme,
        carrier::{self, BearerToken, Carries, Credentials, presented::Presented},
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

    fn present(parts: &Parts) -> Presented<BearerToken> {
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
    async fn authenticate(
        &self,
        presented: Presented<S::Presented>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        // Refuses a malformed credential as an authenticator wanting the
        // guard's old answer does, and admits the rest.
        if let Presented::Malformed(malformed) = presented {
            return Err(malformed.into_rejection());
        }
        Ok("caller".to_owned())
    }
}

/// Every scheme with `Grant` scopes at once, which the scope type in the header
/// keeps apart from the impl every unscoped scheme's authenticator has.
impl<S, C> Authorizer<S, C, Grant> for Recorder
where
    S: Carries<Credential = String, Scope = Grant>,
    C: Sync,
{
    async fn authorize(&self, _: &String, scopes: &[Grant], _: &C) -> Result<(), AuthRejection> {
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

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
    }
}

/// Refuses every credential as unauthenticated.
struct Rejecting;

impl<C: Sync> Authenticator<Revoked, C> for Rejecting {
    async fn authenticate(
        &self,
        _: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }
}

impl<C: Sync> Authorizer<Revoked, C, Grant> for Rejecting {
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

/// A scheme whose carrier reads the required header `HEADER` beside its
/// credential.
struct Reading<const HEADER: char>;

impl<const HEADER: char> SecurityScheme for Reading<HEADER> {
    const NAME: &'static str = "Reading";
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn reads(_: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        vec![
            kynos_openapi::Parameter::header(HEADER.to_string(), kynos_openapi::Schema::any())
                .required(true),
        ]
    }
}

/// The header parameters an operation guarded by `D` declares, each with
/// whether it is required.
fn declared_headers<D: Describe>() -> Vec<(String, Option<bool>)> {
    let (operation, _) = described::<D>();
    operation
        .parameters
        .iter()
        .map(|parameter| {
            let parameter = parameter.as_item().expect("an inline parameter");
            (parameter.name.clone(), parameter.required)
        })
        .collect()
}

/// What a scheme reads beside its credential is declared by every guard naming
/// it, and required only where every request the guard admits presents that
/// scheme's credential: not under `MaybeAuth`, nor for an `AnyOf` alternative.
#[test]
fn every_guard_declares_what_its_schemes_read_and_requires_it_only_where_presented() {
    let required = |header: &str| (header.to_owned(), Some(true));
    let optional = |header: &str| (header.to_owned(), None);

    assert_eq!(declared_headers::<Auth<Bearer>>(), []);
    assert_eq!(declared_headers::<Auth<Reading<'A'>>>(), [required("A")]);
    assert_eq!(
        declared_headers::<Scoped<Reading<'A'>, ReadReports>>(),
        [required("A")]
    );
    assert_eq!(
        declared_headers::<Auth<AllOf<(Reading<'A'>, Bearer, Reading<'B'>)>>>(),
        [required("A"), required("B")]
    );

    assert_eq!(
        declared_headers::<MaybeAuth<Reading<'A'>>>(),
        [optional("A")]
    );
    assert_eq!(
        declared_headers::<Auth<AnyOf<(Bearer, Reading<'A'>)>>>(),
        [optional("A")]
    );
    assert_eq!(
        declared_headers::<Scoped<AnyOf<(Reading<'A'>, Reading<'B'>)>, ReadReports>>(),
        [optional("A"), optional("B")]
    );
    assert_eq!(
        declared_headers::<MaybeAuth<AllOf<(Reading<'A'>, Reading<'B'>)>>>(),
        [optional("A"), optional("B")]
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
/// absence reaches the authenticator as `Presented::Absent`.
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
    type Presented = BearerToken;

    const OPTIONAL: bool = true;

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
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

    fn present(parts: &Parts) -> Presented<BearerToken> {
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
    async fn authenticate(
        &self,
        _: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        Err(AuthRejection::forbidden())
    }
}

impl<C: Sync> Authorizer<Bearer<String, Grant>, C, Grant> for Suspending {
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

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
    }
}

/// Cannot reach its store, and expects it back in a second and a half.
struct Down;

impl<C: Sync> Authenticator<Outage, C> for Down {
    async fn authenticate(
        &self,
        _: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        Err(AuthRejection::unavailable::<Outage>().with_retry_after(Duration::from_millis(1_500)))
    }
}

impl<C: Sync> Authorizer<Outage, C, Grant> for Down {
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

/// A password, or a token holding `reports:read`: the set scopes the
/// alternative it is written on and no other.
type PasswordOrReader = Auth<AnyOf<(Basic<String>, Scoped<Bearer<String, Grant>, ReadReports>)>>;

/// A scoped member declares its set on its own scheme, after that scheme's
/// defaults, and leaves every other member as written, under each guard that
/// takes a combination.
#[test]
fn a_scoped_member_declares_its_set_on_its_own_scheme_alone() {
    let password = || bare::<Basic<String>>();
    let reader = || {
        SecurityRequirement::scoped(
            component_name::<Bearer<String, Grant>>().as_str(),
            ["reports:read"],
        )
    };

    let (either, components) = described::<PasswordOrReader>();
    assert_eq!(either.security, Some(vec![password(), reader()]));
    for key in [
        component_name::<Basic<String>>(),
        component_name::<Bearer<String, Grant>>(),
    ] {
        assert!(
            components.security_schemes.contains_key(key.as_str()),
            "{key:?}"
        );
    }

    let (maybe, _) = described::<
        MaybeAuth<AnyOf<(Basic<String>, Scoped<Bearer<String, Grant>, ReadReports>)>>,
    >();
    assert_eq!(
        maybe.security,
        Some(vec![SecurityRequirement::anonymous(), password(), reader()])
    );

    let (both, _) =
        described::<Auth<AllOf<(Scoped<Audited, EditReports>, MutualTls<String, Grant>)>>>();
    let mut together = SecurityRequirement::scoped(
        component_name::<Audited>().as_str(),
        ["audit", "reports:read", "reports:write"],
    );
    together.0.extend(bare::<MutualTls<String, Grant>>().0);
    assert_eq!(both.security, Some(vec![together]));

    // One scheme scoped two ways is two alternatives, not one.
    let (twice, _) = described::<
        Auth<
            AnyOf<(
                Scoped<Bearer<String, Grant>, ReadReports>,
                Scoped<Bearer<String, Grant>, EditReports>,
            )>,
        >,
    >();
    assert_eq!(
        twice.security,
        Some(vec![
            reader(),
            SecurityRequirement::scoped(
                component_name::<Bearer<String, Grant>>().as_str(),
                ["reports:read", "reports:write"],
            ),
        ])
    );
}

/// A scoped member makes its guard one that may refuse a scope: the 403 is
/// declared even beside schemes that never forbid, carries the member's
/// challenge, and stays the un-narrowed `Problem` beside a bare member, whose
/// refusals may carry any type.
#[test]
fn a_scoped_member_declares_the_403_a_scope_refusal_is() {
    /// Names its refusal.
    struct Named;

    impl Scopes for Named {
        type Scope = Grant;
        const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
        const FORBIDDEN_TYPE: Option<&'static str> = Some("https://errors.example.com/no-scope");
    }

    assert!(!declares_forbidden::<Auth<AnyOf<(Probe, Scrape)>>>());
    assert!(declares_forbidden::<
        Auth<AnyOf<(Probe, Scoped<Scrape, ReadReports>)>>,
    >());
    assert!(declares_forbidden::<
        Auth<AllOf<(Probe, Scoped<Scrape, ReadReports>)>>,
    >());

    assert_eq!(
        declared_scope_challenge::<Auth<AnyOf<(Basic<String>, Bearer<String, Grant>)>>>(),
        None
    );
    let header = declared_scope_challenge::<PasswordOrReader>().expect("a declared challenge");
    assert_eq!(
        header["example"],
        serde_json::json!(r#"Bearer error="insufficient_scope""#),
        "{header}"
    );

    let body = |operation: kynos_openapi::Operation| {
        serde_json::to_value(&operation.responses.responses["403"])
            .expect("a serializable 403")["content"]
            .take()
    };
    let (named, _) =
        described::<Auth<AnyOf<(Basic<String>, Scoped<Bearer<String, Grant>, Named>)>>>();
    let (unscoped, _) = described::<Auth<Bearer>>();
    assert_eq!(body(named), body(unscoped));
}

/// Scoping every member by sets naming one refusal narrows the 403 as the
/// `Scoped` guard does, so `Auth<AnyOf<(Scoped<A, R>, Scoped<B, R>)>>`
/// describes exactly what `Scoped<AnyOf<(A, B)>, R>` does. Members naming
/// different refusals, or a member naming none, leave it wide.
#[test]
fn members_naming_one_refusal_narrow_the_403_as_the_scoped_guard_does() {
    /// Names its refusal.
    struct Named;

    impl Scopes for Named {
        type Scope = Grant;
        const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
        const FORBIDDEN_TYPE: Option<&'static str> = Some("https://errors.example.com/no-scope");
    }

    /// Names another refusal.
    struct Other;

    impl Scopes for Other {
        type Scope = Grant;
        const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
        const FORBIDDEN_TYPE: Option<&'static str> = Some("https://errors.example.com/other");
    }

    let (guard, _) = described::<Scoped<AnyOf<(Audited, Bearer<String, Grant>)>, Named>>();
    let (members, _) =
        described::<Auth<AnyOf<(Scoped<Audited, Named>, Scoped<Bearer<String, Grant>, Named>)>>>();
    assert_eq!(members, guard);

    // The body alone: `Auth<Bearer>` authorizes no scope, so it owes no
    // `insufficient_scope` header the scoped guards declare.
    let forbidden = |operation: kynos_openapi::Operation| {
        serde_json::to_value(&operation.responses.responses["403"])
            .expect("a serializable 403")["content"]
            .take()
    };
    let (single, _) = described::<Scoped<Bearer<String, Grant>, Named>>();
    let (together, _) =
        described::<Auth<AllOf<(Scoped<Audited, Named>, Scoped<Bearer<String, Grant>, Named>)>>>();
    assert_eq!(forbidden(together), forbidden(single));

    let (wide, _) = described::<Auth<Bearer>>();
    let (disagreeing, _) =
        described::<Auth<AnyOf<(Scoped<Audited, Named>, Scoped<Bearer<String, Grant>, Other>)>>>();
    assert_eq!(forbidden(disagreeing), forbidden(wide.clone()));
    let (unnamed, _) = described::<
        Auth<
            AnyOf<(
                Scoped<Audited, Named>,
                Scoped<Bearer<String, Grant>, ReadReports>,
            )>,
        >,
    >();
    assert_eq!(forbidden(unnamed), forbidden(wide));
}

/// Each alternative is authorized for its own set: a scoped member for its
/// defaults and its set together, and a bare member with no defaults not at
/// all, so the member a refused set fails is passed over for the next.
#[tokio::test]
async fn any_of_authorizes_each_alternative_for_its_own_set() {
    type EditorOrBearer = Auth<AnyOf<(Scoped<Audited, EditReports>, Bearer<String, Grant>)>>;

    let refusing = Recording::refusing();
    let admitted = <EditorOrBearer as Guard<_>>::guard(&presenting_a_token(), &refusing).await;
    assert!(
        matches!(
            admitted.map(Auth::into_inner),
            Ok(Either2::Second(caller)) if caller == "caller"
        ),
        "the bare alternative was not admitted"
    );
    assert_eq!(
        refusing.0.authorized(),
        [vec!["audit", "reports:read", "reports:write"]]
    );

    let admitting = Recording::admitting();
    let admitted = <EditorOrBearer as Guard<_>>::guard(&presenting_a_token(), &admitting).await;
    assert!(
        matches!(admitted.map(Auth::into_inner), Ok(Either2::First(_))),
        "the scoped alternative was not admitted"
    );
    assert_eq!(
        admitting.0.authorized(),
        [vec!["audit", "reports:read", "reports:write"]]
    );

    // A scoped member is authorized even for an empty set, as `Scoped` is.
    let refusing = Recording::refusing();
    let refused =
        <Auth<AnyOf<(Scoped<Bearer<String, Grant>, NoScopes>, Basic<String>)>> as Guard<_>>::guard(
            &presenting_a_token(),
            &refusing,
        )
        .await;
    assert_eq!(
        refused.err().map(|r| r.status()),
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(refusing.0.authorized(), [Vec::<&str>::new()]);
}

/// `AllOf` authorizes each scheme for its own set, in tuple order, once every
/// scheme has authenticated.
#[tokio::test]
async fn all_of_authorizes_each_scheme_for_its_own_set() {
    let admitting = Recording::admitting();
    let admitted = <Auth<AllOf<(Scoped<Bearer<String, Grant>, ReadReports>, Audited)>> as Guard<
        _,
    >>::guard(&presenting_a_token(), &admitting)
    .await;
    assert!(admitted.is_ok(), "both schemes were not admitted");
    assert_eq!(
        admitting.0.authorized(),
        [vec!["reports:read"], vec!["audit", "reports:read"]]
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

/// A bearer scheme whose authenticator never refuses a valid credential, with
/// a scope challenge a declared 403 would carry; its scope type is `Grant` so
/// a `Scoped` argument can still name it.
struct Scrape;

impl SecurityScheme for Scrape {
    const NAME: &'static str = "Scrape";
    const FORBIDS: bool = false;
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }

    fn challenge() -> Option<&'static str> {
        Some("Bearer")
    }

    fn insufficient_scope_challenge() -> Option<&'static str> {
        <Bearer as SecurityScheme>::insufficient_scope_challenge()
    }
}

impl Carries for Scrape {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
    }
}

/// A second scheme that never forbids, to combine with the first.
struct Probe;

impl SecurityScheme for Probe {
    const NAME: &'static str = "Probe";
    const FORBIDS: bool = false;
    type Credential = String;
    type Scope = Grant;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }
}

impl Carries for Probe {
    type Presented = BearerToken;

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
    }
}

/// Whether guard `D`'s operation declares a 403, beside the 401 it always
/// declares.
fn declares_forbidden<D: Describe>() -> bool {
    let (operation, _) = described::<D>();
    assert!(operation.responses.responses.contains_key("401"));
    operation.responses.responses.contains_key("403")
}

/// A guard none of whose schemes forbids declares the 401 alone, without the
/// 403 its `insufficient_scope` challenge would otherwise invent, while one
/// naming a scheme that forbids, or demanding a scope set, keeps the 403.
#[test]
fn only_a_guard_whose_schemes_may_forbid_declares_a_403() {
    assert!(!declares_forbidden::<Auth<Scrape>>());
    assert!(!declares_forbidden::<MaybeAuth<Scrape>>());
    assert!(!declares_forbidden::<Auth<AnyOf<(Scrape, Probe)>>>());
    assert!(!declares_forbidden::<Auth<AllOf<(Scrape, Probe)>>>());

    assert!(declares_forbidden::<Auth<Bearer>>());
    assert!(declares_forbidden::<Auth<AnyOf<(Scrape, Bearer)>>>());
    assert!(declares_forbidden::<Auth<AllOf<(Bearer, Scrape)>>>());
    assert!(declares_forbidden::<Scoped<Scrape, ReadReports>>());
}

// --- What the written-out implementations say ------------------------------

/// Witnesses the four implementations a guard's value writes out.
fn assert_clone_debug_eq<T: Clone + std::fmt::Debug + Eq>() {}

/// Witnesses the `Copy` beside them.
fn assert_copy<T: Copy>() {}

/// `Auth`, `MaybeAuth` and `Scoped` print, compare and clone their credential
/// and nothing else, bounded on the credential alone.
///
/// `Audited` and `ReadReports` derive nothing, so these witnesses are refused
/// the moment a bound lands on the scheme or the scope set; the values below
/// hold what each implementation does once it exists.
#[test]
fn a_guard_value_prints_compares_and_clones_its_credential_alone() {
    assert_clone_debug_eq::<Auth<Audited>>();
    assert_clone_debug_eq::<MaybeAuth<Audited>>();
    assert_clone_debug_eq::<Scoped<Audited, ReadReports>>();
    assert_copy::<Auth<Bearer<u32, Grant>>>();
    assert_copy::<MaybeAuth<Bearer<u32, Grant>>>();
    assert_copy::<Scoped<Bearer<u32, Grant>, ReadReports>>();

    let auth = Auth::<Audited>("alice".to_owned());
    assert_eq!(format!("{auth:?}"), r#"Auth("alice")"#);
    assert_eq!(auth.clone(), auth);
    assert_ne!(auth, Auth("bob".to_owned()));

    let maybe = MaybeAuth::<Audited>(Some("alice".to_owned()));
    assert_eq!(format!("{maybe:?}"), r#"MaybeAuth(Some("alice"))"#);
    assert_eq!(
        format!("{:?}", MaybeAuth::<Audited>(None)),
        "MaybeAuth(None)"
    );
    assert_eq!(maybe.clone(), maybe);
    assert_ne!(maybe, MaybeAuth(None));

    let scoped = Scoped::<Audited, ReadReports>::new("alice".to_owned());
    assert_eq!(format!("{scoped:?}"), r#"Scoped("alice")"#);
    assert_eq!(scoped.clone(), scoped);
    assert_ne!(scoped, Scoped::new("bob".to_owned()));
}

/// A bearer scheme whose authenticator reports what reached it, handing
/// absence on where `OPTIONAL` is set.
struct Witnessed<const OPTIONAL: bool>;

impl<const OPTIONAL: bool> SecurityScheme for Witnessed<OPTIONAL> {
    const NAME: &'static str = "Witnessed";
    type Credential = String;
    type Scope = crate::security::Unscoped;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::bearer(None)
    }
}

impl<const OPTIONAL: bool> Carries for Witnessed<OPTIONAL> {
    type Presented = BearerToken;

    const OPTIONAL: bool = OPTIONAL;

    fn present(parts: &Parts) -> Presented<BearerToken> {
        carrier::bearer(parts)
    }
}

/// Admits whatever reaches it, as a credential naming what that was and the
/// request line it arrived on.
struct Witness;

impl<const OPTIONAL: bool> Authenticator<Witnessed<OPTIONAL>, Witness> for Witness {
    async fn authenticate(
        &self,
        presented: Presented<BearerToken>,
        head: Head<'_>,
        _: &Witness,
    ) -> Result<String, AuthRejection> {
        let found = match presented {
            Presented::Absent => "absent".to_owned(),
            Presented::Malformed(malformed) => format!("malformed {}", malformed.reason().as_str()),
            Presented::Present(token) => format!("present {}", token.as_str()),
        };
        Ok(format!("{found} on {} {}", head.method(), head.uri()))
    }
}

impl<const OPTIONAL: bool> Authenticates<Witnessed<OPTIONAL>> for Witness {
    type Authenticator = Self;

    fn authenticator(&self) -> &Self {
        self
    }
}

/// A `POST /reports` head carrying `authorization`, if any.
fn posting(authorization: Option<&'static str>) -> Parts {
    let mut request = Request::new(Body::empty());
    *request.method_mut() = crate::http::Method::POST;
    *request.uri_mut() = crate::http::Uri::from_static("/reports");
    if let Some(authorization) = authorization {
        request
            .headers_mut()
            .insert(AUTHORIZATION, HeaderValue::from_static(authorization));
    }
    request.into_parts().0
}

/// A malformed credential is the authenticator's to judge under every guard,
/// `MaybeAuth` included, and arrives classified, beside the request head.
#[tokio::test]
async fn a_malformed_credential_reaches_the_authenticator_with_its_reason() {
    let other_scheme = posting(Some("Basic dTpw"));
    let expected = "malformed other_scheme on POST /reports";

    let auth = <Auth<Witnessed<false>> as Guard<_>>::guard(&other_scheme, &Witness).await;
    assert_eq!(auth.ok().map(Auth::into_inner).as_deref(), Some(expected));

    let maybe = <MaybeAuth<Witnessed<false>> as Guard<_>>::guard(&other_scheme, &Witness).await;
    assert_eq!(
        maybe.ok().and_then(|maybe| maybe.0).as_deref(),
        Some(expected)
    );
}

/// A well-formed credential arrives with the head of the request it arrived
/// on.
#[tokio::test]
async fn a_present_credential_reaches_the_authenticator_beside_the_request_head() {
    let guarded =
        <Auth<Witnessed<false>> as Guard<_>>::guard(&posting(Some("Bearer t")), &Witness).await;
    assert_eq!(
        guarded.ok().map(Auth::into_inner).as_deref(),
        Some("present t on POST /reports")
    );
}

/// Absence is the guard's to answer, so the authenticator never sees it,
/// unless the scheme is optional, when it sees nothing else.
#[tokio::test]
async fn absence_reaches_the_authenticator_only_from_an_optional_carrier() {
    let bare = posting(None);

    let refused = <Auth<Witnessed<false>> as Guard<_>>::guard(&bare, &Witness).await;
    assert_eq!(
        refused.err().map(|rejection| rejection.status()),
        Some(StatusCode::UNAUTHORIZED)
    );
    let anonymous = <MaybeAuth<Witnessed<false>> as Guard<_>>::guard(&bare, &Witness).await;
    assert!(matches!(anonymous, Ok(MaybeAuth(None))));

    let expected = "absent on POST /reports";
    let auth = <Auth<Witnessed<true>> as Guard<_>>::guard(&bare, &Witness).await;
    assert_eq!(auth.ok().map(Auth::into_inner).as_deref(), Some(expected));
    let maybe = <MaybeAuth<Witnessed<true>> as Guard<_>>::guard(&bare, &Witness).await;
    assert_eq!(
        maybe.ok().and_then(|maybe| maybe.0).as_deref(),
        Some(expected)
    );
}

/// An API-key scheme reading `x-key`, beside which `Witnessed` reads the
/// bearer token, so one request can present each in a different state.
struct Keyed;

impl SecurityScheme for Keyed {
    const NAME: &'static str = "Keyed";
    type Credential = String;
    type Scope = crate::security::Unscoped;

    fn describe() -> kynos_openapi::SecurityScheme {
        kynos_openapi::SecurityScheme::api_key_header("x-key")
    }
}

impl Carries for Keyed {
    type Presented = carrier::ApiKey;

    fn present(parts: &Parts) -> Presented<carrier::ApiKey> {
        carrier::api_key(parts, carrier::KeyLocation::Header, "x-key")
    }
}

/// Keeps what reached each authenticator, in call order, and admits it, but
/// for a key reading `refused`.
#[derive(Default)]
struct Ledger(Mutex<Vec<String>>);

impl Ledger {
    /// What reached the authenticators so far.
    fn seen(&self) -> Vec<String> {
        self.0.lock().expect("an unpoisoned lock").clone()
    }

    /// Records that `presented`, shown by `show`, reached scheme `S`.
    fn record<S: SecurityScheme, T>(
        &self,
        presented: &Presented<T>,
        show: impl Fn(&T) -> &str,
    ) -> String {
        let found = match presented {
            Presented::Absent => "absent".to_owned(),
            Presented::Malformed(malformed) => format!("malformed {}", malformed.reason().as_str()),
            Presented::Present(credential) => format!("present {}", show(credential)),
        };
        let seen = format!("{} {found}", S::NAME);
        self.0
            .lock()
            .expect("an unpoisoned lock")
            .push(seen.clone());
        seen
    }
}

impl Authenticator<Witnessed<false>, Ledger> for Ledger {
    async fn authenticate(
        &self,
        presented: Presented<BearerToken>,
        _: Head<'_>,
        _: &Ledger,
    ) -> Result<String, AuthRejection> {
        Ok(self.record::<Witnessed<false>, _>(&presented, BearerToken::as_str))
    }
}

impl Authenticator<Keyed, Ledger> for Ledger {
    async fn authenticate(
        &self,
        presented: Presented<carrier::ApiKey>,
        _: Head<'_>,
        _: &Ledger,
    ) -> Result<String, AuthRejection> {
        let seen = self.record::<Keyed, _>(&presented, carrier::ApiKey::as_str);
        match presented {
            Presented::Present(key) if key.as_str() == "refused" => {
                Err(AuthRejection::unauthenticated())
            }
            _ => Ok(seen),
        }
    }
}

impl Authenticates<Witnessed<false>> for Ledger {
    type Authenticator = Self;

    fn authenticator(&self) -> &Self {
        self
    }
}

impl Authenticates<Keyed> for Ledger {
    type Authenticator = Self;

    fn authenticator(&self) -> &Self {
        self
    }
}

/// A head presenting `authorization`, if any, and each of `keys` as an `x-key`
/// field.
fn presenting(authorization: Option<&'static str>, keys: &[&'static str]) -> Parts {
    let mut request = Request::new(Body::empty());
    if let Some(authorization) = authorization {
        request
            .headers_mut()
            .insert(AUTHORIZATION, HeaderValue::from_static(authorization));
    }
    for key in keys {
        request
            .headers_mut()
            .append("x-key", HeaderValue::from_static(key));
    }
    request.into_parts().0
}

/// With every credential of an `AllOf` presented, a malformed one reaches its
/// authenticator in tuple order, wherever it stands, and none after a refusal
/// is asked.
#[tokio::test]
async fn all_of_hands_a_malformed_credential_to_its_authenticator_in_tuple_order() {
    let malformed_token = presenting(Some("Basic dTpw"), &["k"]);

    let ledger = Ledger::default();
    let first =
        <Auth<AllOf<(Witnessed<false>, Keyed)>> as Guard<_>>::guard(&malformed_token, &ledger)
            .await;
    assert!(first.is_ok());
    assert_eq!(
        ledger.seen(),
        ["Witnessed malformed other_scheme", "Keyed present k"]
    );

    let ledger = Ledger::default();
    let second =
        <Auth<AllOf<(Keyed, Witnessed<false>)>> as Guard<_>>::guard(&malformed_token, &ledger)
            .await;
    assert!(second.is_ok());
    assert_eq!(
        ledger.seen(),
        ["Keyed present k", "Witnessed malformed other_scheme"]
    );

    let ledger = Ledger::default();
    let refused = <Auth<AllOf<(Keyed, Witnessed<false>)>> as Guard<_>>::guard(
        &presenting(Some("Basic dTpw"), &["refused"]),
        &ledger,
    )
    .await;
    assert_eq!(
        refused.err().map(|rejection| rejection.status()),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(ledger.seen(), ["Keyed present refused"]);
}

/// A malformed credential beside an absent one is a partial set, so the 401
/// comes before any authenticator runs, under `MaybeAuth` too.
#[tokio::test]
async fn all_of_refuses_a_malformed_credential_beside_an_absent_one_unasked() {
    let malformed_key = presenting(None, &["a", "b"]);
    let malformed_token = presenting(Some("Basic dTpw"), &[]);

    for parts in [&malformed_key, &malformed_token] {
        let ledger = Ledger::default();
        let auth =
            <Auth<AllOf<(Witnessed<false>, Keyed)>> as Guard<_>>::guard(parts, &ledger).await;
        assert_eq!(
            auth.err().map(|rejection| rejection.status()),
            Some(StatusCode::UNAUTHORIZED)
        );
        let maybe =
            <MaybeAuth<AllOf<(Witnessed<false>, Keyed)>> as Guard<_>>::guard(parts, &ledger).await;
        assert_eq!(
            maybe.err().map(|rejection| rejection.status()),
            Some(StatusCode::UNAUTHORIZED)
        );
        assert!(ledger.seen().is_empty());
    }
}
