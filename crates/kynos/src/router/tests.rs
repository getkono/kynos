//! What `nest` and `merge` carry from the router they absorb.
//!
//! Interceptors stay with the operations the absorbed router held, which
//! `tests/description.rs` asserts. The rest of what a router holds widens or
//! travels: an observer sees every request the absorbing router serves, a tag
//! reaches the absorbed operations alone while its metadata reaches the
//! document, a security scheme is registered in the document, and
//! `deny_unchecked_schemas` refuses the whole build.

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
                == kynos_openapi::SpecError::UncheckedSchema
                && violation.severity == kynos_openapi::Severity::Error),
            "{how}: {violations:?}"
        );
    }
}
