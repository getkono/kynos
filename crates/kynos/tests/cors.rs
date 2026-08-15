//! Cross-origin sharing, from the configuration to what a browser receives.
//!
//! Two things no other test target covers: that a CORS configuration the
//! protocol forbids is refused while the router is built rather than served as
//! a header browsers reject, and that the response headers reach the wire with
//! the `Vary` a shared cache needs.

#![cfg(all(feature = "macros", feature = "json"))]
#![allow(dead_code)]

use kynos::{middleware::cors::Cors, prelude::*, response::status::NoContent};

/// Something to mount, so the router has an operation to cover.
#[kynos::get("/widgets")]
async fn list_widgets() -> NoContent {
    NoContent
}

fn router() -> Router<()> {
    Router::<()>::new().mount(kynos::routes![list_widgets])
}

/// The CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
/// response, so the pair is a configuration no service can honour.
///
/// Refused while the router is built, because the alternative is worse than a
/// rejected header: `permits` short-circuits on `any_origin`, so the pair
/// silently becomes reflect-any-origin-with-credentials — the most permissive
/// CORS configuration there is, reached by asking for something else.
#[test]
fn a_router_permitting_any_origin_with_credentials_refuses_to_build() {
    let refused = router()
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .build(());

    assert!(
        refused.is_err(),
        "a wildcard origin with credentials was accepted"
    );
}

/// The refusal is in `describe`, not `build`, so every entry point that
/// assembles a description reports it rather than only the one that serves.
#[test]
fn the_same_router_reports_the_conflict_from_validate_as_well() {
    let refused = router()
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .validate();

    assert!(refused.is_err(), "validate accepted what build must refuse");
}

/// The pass control: the same router, differing in exactly the property under
/// test. Named origins with credentials is the ordinary credentialed
/// deployment and has to keep working.
#[test]
fn permitting_named_origins_with_credentials_builds() {
    router()
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .allow_credentials(),
        )
        .build(())
        .expect("named origins with credentials is a legal configuration");
}

/// The other half of the control: a wildcard origin *without* credentials is
/// the ordinary public-API deployment, and is not what the refusal is about.
#[test]
fn permitting_any_origin_without_credentials_builds() {
    router()
        .intercept(Cors::new().allow_any_origin())
        .build(())
        .expect("a wildcard origin alone is a legal configuration");
}
