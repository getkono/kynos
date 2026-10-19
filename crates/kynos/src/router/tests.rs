//! What `nest` and `merge` carry from the router they absorb.
//!
//! Interceptors stay with the operations the absorbed router held, which
//! `tests/description.rs` asserts. The rest of what a router holds widens or
//! travels: an observer sees every request the absorbing router serves, a tag
//! reaches the absorbed operations alone while its metadata reaches the
//! document, a security scheme is registered in the document, and
//! `deny_unchecked_schemas` refuses the whole build.
//!
//! Below those, what a router makes of a malformed mount site, and the
//! `operationId`s a mounted reference publishes.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos_openapi::{Method, PathTemplate};

use super::{Router, endpoint::builder::EndpointBuilder, operation::Route};
use crate::{
    http::{Request, Response, body::Body},
    middleware::Observer,
    response::status::NoContent,
    router::error::RouterError,
};

async fn handled() -> NoContent {
    NoContent
}

/// A `GET` on `path`; a macro, since the builder's handler type is unnameable.
macro_rules! at {
    ($path:literal) => {
        EndpointBuilder::<(), _, _>::new(
            Method::Get,
            PathTemplate::parse($path).expect("a usable path template"),
            handled,
        )
    };
}

/// Each way one router absorbs another: under a prefix, and merged.
fn absorbed(outer: Router<()>, inner: Router<()>, how: &str) -> Router<()> {
    match how {
        "nest" => outer.nest("/inner", inner),
        "merge" => outer.merge(inner),
        _ => unreachable!("only two ways to absorb a router"),
    }
}

/// The path an absorbed router's `/held` operation ends up at.
fn held(how: &str) -> &'static str {
    match how {
        "nest" => "/inner/held",
        _ => "/held",
    }
}

/// Records the path of every request an observer was told of.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Observer<()> for Seen {
    fn on_request(&self, request: &Request, _: Option<Route<'_>>, (): &()) {
        self.0
            .lock()
            .expect("an unpoisoned record")
            .push(request.uri().path().to_owned());
    }

    fn on_response(&self, _: &Response, _: Option<Route<'_>>, _: Duration) {}
}

/// An observer sees whole requests rather than operations, so one an absorbed
/// router declared hears the absorbing router's own operations too.
#[tokio::test]
async fn an_absorbed_observer_widens_to_the_absorbing_router() {
    for how in ["nest", "merge"] {
        let seen = Seen::default();
        let service = absorbed(
            Router::<()>::new().mount(at!("/outer")),
            Router::<()>::new()
                .mount(at!("/held"))
                .observe(seen.clone()),
            how,
        )
        .build(())
        .expect("a describable router");

        for path in ["/outer", held(how)] {
            let mut request = Request::new(Body::empty());
            *request.uri_mut() = path.parse().expect("a usable request target");
            service.call(request).await;
        }

        assert_eq!(
            *seen.0.lock().expect("an unpoisoned record"),
            ["/outer", held(how)],
            "{how}"
        );
    }
}

struct Inner;

impl crate::router::operation::Tag for Inner {
    const NAME: &'static str = "inner";

    fn metadata() -> kynos_openapi::Tag {
        kynos_openapi::Tag::new("inner")
    }
}

/// An absorbed router's tag reaches the operations it held and no other, and
/// its metadata reaches the document.
#[test]
fn an_absorbed_tag_reaches_what_that_router_held_and_the_document() {
    for how in ["nest", "merge"] {
        let document = serde_json::to_value(
            absorbed(
                Router::<()>::new().mount(at!("/outer")),
                Router::<()>::new().tag::<Inner>().mount(at!("/held")),
                how,
            )
            .openapi()
            .expect("a describable router"),
        )
        .expect("a serializable document");

        assert_eq!(
            document["paths"][held(how)]["get"]["tags"],
            serde_json::json!(["inner"]),
            "{how}"
        );
        assert!(
            document["paths"]["/outer"]["get"]["tags"].is_null(),
            "{how}: {}",
            document["paths"]["/outer"]["get"]
        );
        assert_eq!(
            document["tags"],
            serde_json::json!([{ "name": "inner" }]),
            "{how}"
        );
    }
}

/// An absorbed router's security scheme is registered in the document.
#[test]
fn an_absorbed_security_scheme_is_registered() {
    for how in ["nest", "merge"] {
        let document = serde_json::to_value(
            absorbed(
                Router::<()>::new().mount(at!("/outer")),
                Router::<()>::new()
                    .security_scheme::<crate::security::schemes::Bearer<()>>()
                    .mount(at!("/held")),
                how,
            )
            .openapi()
            .expect("a describable router"),
        )
        .expect("a serializable document");

        assert_eq!(
            document["components"]["securitySchemes"],
            serde_json::json!({ "Bearer": { "type": "http", "scheme": "bearer" } }),
            "{how}"
        );
    }
}

/// A body the description cannot constrain, which `deny_unchecked_schemas`
/// refuses.
#[cfg(feature = "json")]
async fn unconstrained(
    crate::extract::body::json::Json(_): crate::extract::body::json::Json<
        crate::schema::unchecked::Unchecked<serde_json::Value>,
    >,
) -> NoContent {
    NoContent
}

/// `deny_unchecked_schemas` on an absorbed router refuses an unconstrained
/// schema anywhere in the build, the absorbing router's own operations
/// included; without it the same build succeeds.
#[cfg(feature = "json")]
#[test]
fn an_absorbed_denial_of_unchecked_schemas_refuses_the_whole_build() {
    let outer = || {
        Router::<()>::new().mount(EndpointBuilder::<(), _, _>::new(
            Method::Post,
            PathTemplate::parse("/outer").expect("a usable path template"),
            unconstrained,
        ))
    };

    for how in ["nest", "merge"] {
        absorbed(outer(), Router::<()>::new().mount(at!("/held")), how)
            .openapi()
            .expect("an unconstrained schema alone only warns");

        let refused = absorbed(
            outer(),
            Router::<()>::new()
                .deny_unchecked_schemas()
                .mount(at!("/held")),
            how,
        )
        .openapi();
        let Err(crate::Error::Invalid { violations }) = refused else {
            panic!("{how}: the absorbed denial did not refuse the build");
        };
        assert!(
            violations.iter().any(|violation| violation.error
                == RouterError::Spec(kynos_openapi::SpecError::UncheckedSchema)
                && violation.severity == kynos_openapi::Severity::Error),
            "{how}: {violations:?}"
        );
    }
}

// --- Malformed mount sites -----------------------------------------------------

/// The path-template violations a refused router reports, in order.
fn template_violations(router: &Router<()>) -> Vec<kynos_openapi::Violation<RouterError>> {
    let Err(crate::Error::Invalid { violations }) = router.openapi() else {
        panic!("a malformed mount site did not refuse the router");
    };
    violations
        .into_iter()
        .filter(|violation| {
            matches!(
                violation.error,
                RouterError::Spec(kynos_openapi::SpecError::InvalidPathTemplate { .. })
            )
        })
        .collect()
}

/// The violation a malformed path literal at a mount site records.
fn malformed(
    location: &str,
    template: &str,
    prefixed: &str,
) -> kynos_openapi::Violation<RouterError> {
    kynos_openapi::Violation {
        location: location.to_owned(),
        severity: kynos_openapi::Severity::Error,
        error: RouterError::Spec(kynos_openapi::SpecError::InvalidPathTemplate {
            template: template.to_owned(),
            reason: kynos_openapi::model::paths::template::InvalidPathTemplate::MissingLeadingSlash(
                prefixed.to_owned(),
            ),
        }),
    }
}

/// A group's prefix that is no path template is recorded where the group is
/// made, and refuses the router rather than panicking or mounting it at the
/// root.
#[test]
fn a_malformed_group_prefix_refuses_the_router() {
    let router =
        Router::<()>::new().group(super::group::Group::<()>::new("v1").mount(at!("/held")));

    assert_eq!(
        template_violations(&router),
        [malformed("#/paths", "v1", "v1")]
    );
}

/// A nesting prefix that is no path template refuses each operation it would
/// have prefixed, at that operation's own path.
#[test]
fn a_malformed_nesting_prefix_refuses_each_operation_it_prefixes() {
    let router = Router::<()>::new().nest(
        "v1",
        Router::<()>::new().mount(at!("/held")).mount(at!("/other")),
    );

    assert_eq!(
        template_violations(&router),
        [
            malformed("#/paths/~1held", "/held", "v1/held"),
            malformed("#/paths/~1other", "/other", "v1/other"),
        ]
    );
}

/// The two paths a reference is placed at are each held to the template
/// grammar, and a malformed one refuses the router.
#[cfg(feature = "docs")]
#[test]
fn a_malformed_reference_path_refuses_the_router() {
    use super::docs::Docs;

    let page = Router::<()>::new().docs(Docs::scalar().at("reference"));
    assert_eq!(
        template_violations(&page),
        [malformed("#/paths", "reference", "reference")]
    );

    let description =
        Router::<()>::new().docs(Docs::description_only().description_at("spec.json"));
    assert_eq!(
        template_violations(&description),
        [malformed("#/paths", "spec.json", "spec.json")]
    );
}

/// Each `GET` operation's path and the `operationId` it publishes.
#[cfg(feature = "docs")]
fn published_ids(router: &Router<()>) -> Vec<String> {
    let document = router.openapi().expect("a describable router");
    document
        .paths
        .items
        .iter()
        .filter_map(|(path, item)| {
            let operation = item.get.as_deref()?;
            Some(format!("{path} {}", operation.operation_id.as_deref()?))
        })
        .collect()
}

/// Each half of a reference publishes an `operationId` under its prefix,
/// `docs` unless one is given; and two references nested beneath different
/// prefixes collide under the default and are told apart by giving one its
/// own.
#[cfg(feature = "docs")]
#[test]
fn a_reference_publishes_its_operation_ids_under_its_prefix() {
    use super::docs::Docs;

    assert_eq!(
        published_ids(&Router::<()>::new().docs(Docs::scalar())),
        ["/openapi.json docs_openapi_json", "/docs docs_docs"]
    );
    assert_eq!(
        published_ids(&Router::<()>::new().docs(Docs::scalar().operation_id_prefix("reference"))),
        [
            "/openapi.json reference_openapi_json",
            "/docs reference_docs"
        ]
    );

    let twice = |second: Docs<_>| {
        Router::<()>::new()
            .nest("/v1", Router::<()>::new().docs(Docs::scalar()))
            .nest("/v2", Router::<()>::new().docs(second))
    };
    let Err(crate::Error::Invalid { violations }) = twice(Docs::scalar()).openapi() else {
        panic!("two references under one prefix were described");
    };
    assert!(
        violations.iter().any(|violation| matches!(
            violation.error,
            RouterError::Spec(kynos_openapi::SpecError::DuplicateOperationId { .. })
        )),
        "{violations:?}"
    );
    twice(Docs::scalar().operation_id_prefix("v2_docs"))
        .openapi()
        .expect("two references under two prefixes");
}

/// One case per variant of [`RouterError`], what a router's violation says is
/// wrong.
mod router_errors {
    use std::collections::BTreeSet;

    use kynos_openapi::{Method, SpecError};

    use super::RouterError;

    /// Every variant, named. An exhaustive match, so a variant added to
    /// [`RouterError`] stops this file compiling until it is given a case.
    fn variant_name(error: &RouterError) -> &'static str {
        match error {
            RouterError::Spec(_) => "Spec",
            RouterError::DuplicateOperation { .. } => "DuplicateOperation",
            RouterError::ShortCircuitMismatch { .. } => "ShortCircuitMismatch",
            RouterError::RouteConflict { .. } => "RouteConflict",
            RouterError::InvalidRoutePattern { .. } => "InvalidRoutePattern",
            RouterError::NestedRouterPolicy { .. } => "NestedRouterPolicy",
        }
    }

    /// One constructed value per variant, with the whole of what it must say.
    ///
    /// `Spec` is transparent: a document's violation reads the same whether a
    /// validator or a router reports it.
    fn ledger() -> Vec<(RouterError, &'static str)> {
        vec![
            (
                SpecError::NoResponses.into(),
                "an operation must declare at least one response",
            ),
            (
                RouterError::DuplicateOperation {
                    method: Method::Get,
                    path: "/users".to_owned(),
                },
                "operation `GET /users` is described more than once; a path item holds one \
                 operation per method",
            ),
            (
                RouterError::ShortCircuitMismatch {
                    name: "app::Limited".to_owned(),
                    declared: vec![429],
                    described: vec![429, 503],
                },
                "`app::Limited` declares statuses [429] as a short circuit but describes \
                 [429, 503]",
            ),
            (
                RouterError::RouteConflict {
                    pattern: "/v{x}".to_owned(),
                    existing: "/{x}.txt".to_owned(),
                },
                "`/v{x}` conflicts with `/{x}.txt` in the router's match table",
            ),
            (
                RouterError::InvalidRoutePattern {
                    pattern: "/{*path}/raw".to_owned(),
                },
                "`/{*path}/raw` is not a pattern the router's match table accepts",
            ),
            (
                RouterError::NestedRouterPolicy {
                    policy: "trusted_proxies",
                },
                "a nested or merged router sets `trusted_proxies`, which only the root router's \
                 applies; set it on the root router instead",
            ),
        ]
    }

    #[test]
    fn each_variant_says_what_failed() {
        for (error, expected) in ledger() {
            let name = variant_name(&error);

            assert_eq!(error.to_string(), expected, "{name}");
        }
    }

    /// The ledger against the variants, transcribed, for the reason
    /// [`Error`](crate::Error)'s own ledger gives.
    #[test]
    fn every_variant_has_a_case() {
        let covered: BTreeSet<&str> = ledger()
            .iter()
            .map(|(error, _)| variant_name(error))
            .collect();
        let declared: BTreeSet<&str> = [
            "Spec",
            "DuplicateOperation",
            "ShortCircuitMismatch",
            "RouteConflict",
            "InvalidRoutePattern",
            "NestedRouterPolicy",
        ]
        .into_iter()
        .collect();

        assert_eq!(covered, declared);
    }
}
