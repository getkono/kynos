//! A mounted reference, and the document that describes it.
//!
//! One reason: a docs mount is a *routing* surface whose operations and whose
//! served bytes only exist once a router is built. The description it serves is
//! the description that router emits, and it cannot be rendered any earlier
//! because it has to describe these two routes -- so nothing short of a built
//! service can check either half.

#![cfg(all(feature = "docs", feature = "macros", feature = "json"))]

use kynos::{Router, http::StatusCode, openapi::Info, router::docs::Docs};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, post};

/// The fixture's four operations plus a reference, built.
fn served(docs: Docs) -> kynos::router::service::Service<App> {
    support::router()
        .info(Info::new("Example API", "1.0.0"))
        .docs(docs)
        .build(App::new())
        .expect("a describable router")
}

#[test]
fn both_docs_routes_are_described_where_they_are_mounted() {
    let document = support::router()
        .docs(Docs::scalar())
        .openapi()
        .expect("a describable router");

    for path in ["/docs", "/openapi.json"] {
        let item = document
            .paths
            .items
            .get(path)
            .unwrap_or_else(|| panic!("{path} is missing from the document"));
        assert!(item.get.is_some(), "{path} declares no GET");
    }
}

#[test]
fn a_router_that_mounts_no_docs_describes_neither_path() {
    // The control for the case above. Without it, that test passes against a
    // `describe` pass emitting the two keys unconditionally.
    let document = support::router().openapi().expect("a describable router");

    for path in ["/docs", "/openapi.json"] {
        assert!(
            !document.paths.items.contains_key(path),
            "{path} is described by a router that mounts no reference",
        );
    }
}

#[test]
fn the_page_is_described_as_html_and_the_description_as_json() {
    let document = support::router()
        .docs(Docs::scalar())
        .openapi()
        .expect("a describable router");

    for (path, media_type) in [
        ("/docs", "text/html; charset=utf-8"),
        ("/openapi.json", "application/json"),
    ] {
        let operation = document.paths.items[path].get.as_ref().expect("a GET");
        let ok = operation
            .responses
            .responses
            .get("200")
            .and_then(kynos::openapi::RefOr::as_item)
            .expect("a described 200");

        assert!(
            ok.content.contains_key(media_type),
            "{path} does not declare {media_type}, only {:?}",
            ok.content.keys().collect::<Vec<_>>(),
        );
    }
}

#[tokio::test]
async fn every_shipped_reference_is_served_as_html() {
    // Each constructor carries its own marker, so one wired to the other's page
    // fails here rather than rendering something plausible.
    for (docs, marker) in [
        (Docs::scalar(), "Scalar.createApiReference"),
        (Docs::redoc(), "Redoc.init"),
    ] {
        let service = served(docs);
        let reply = get(&service, "/docs").call().await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field("content-type").as_deref(),
            Some("text/html; charset=utf-8"),
        );
        assert!(
            reply.text().contains(marker),
            "the page does not boot the renderer that built it",
        );
    }
}

#[tokio::test]
async fn the_description_route_serves_the_document_this_router_emits() {
    // The strongest form of the claim, and it subsumes "parses as JSON" and
    // "carries both docs routes". Sound only because emission is byte-stable,
    // which `determinism.rs` is what establishes.
    let router = support::router()
        .info(Info::new("Example API", "1.0.0"))
        .docs(Docs::scalar());
    let emitted = router.openapi().expect("a describable router");
    let expected = emitted.to_json().expect("a serializable document");

    let service = router.build(App::new()).expect("a describable router");
    let reply = get(&service, "/openapi.json").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field("content-type").as_deref(),
        Some("application/json"),
    );
    assert_eq!(reply.text(), expected);
}

#[tokio::test]
async fn the_configured_paths_move_the_routes_and_the_pointer_together() {
    let service = served(
        Docs::scalar()
            .at("/reference")
            .description_at("/v1/openapi.json"),
    );

    let page = get(&service, "/reference").call().await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(
        get(&service, "/v1/openapi.json").call().await.status,
        StatusCode::OK,
    );

    // A page pointing at a path nothing serves is the failure the two setters
    // share, so the pointer is checked rather than only the routes.
    assert!(
        page.text().contains("/v1/openapi.json"),
        "the page still fetches somewhere else",
    );

    for stale in ["/docs", "/openapi.json"] {
        assert_eq!(
            get(&service, stale).call().await.status,
            StatusCode::NOT_FOUND,
            "{stale} is still served",
        );
    }
}

#[tokio::test]
async fn nesting_moves_both_routes_and_the_page_follows_them() {
    // The prefix is applied while the outer router builds, long after the
    // `Docs` value was constructed -- so a page rendered at construction time
    // is broken and nothing else in this file notices.
    let service = Router::<App>::new()
        .info(Info::new("Example API", "1.0.0"))
        .nest("/api", support::router().docs(Docs::scalar()))
        .build(App::new())
        .expect("a describable router");

    let page = get(&service, "/api/docs").call().await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(
        get(&service, "/api/openapi.json").call().await.status,
        StatusCode::OK,
    );
    assert_eq!(
        get(&service, "/docs").call().await.status,
        StatusCode::NOT_FOUND,
    );

    assert!(
        page.text().contains("/api/openapi.json"),
        "the nested page fetches the unprefixed path",
    );

    let described = get(&service, "/api/openapi.json").call().await.json();
    let paths = described["paths"].as_object().expect("a paths object");
    assert!(paths.contains_key("/api/docs"));
    assert!(paths.contains_key("/api/openapi.json"));
}

#[tokio::test]
async fn a_custom_page_is_served_verbatim() {
    // The control for the two tests above: without it, "the page names the
    // configured path" passes against an implementation that appends the path
    // to whatever page it was handed.
    let written = "<!doctype html><p>hi</p>";
    let service = served(Docs::custom(written));
    let reply = get(&service, "/docs").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.text(), written);
}

#[tokio::test]
async fn the_title_defaults_to_the_document_title() {
    let service = served(Docs::scalar());

    assert!(
        get(&service, "/docs")
            .call()
            .await
            .text()
            .contains("Example API"),
        "the page does not carry the document's own title",
    );
}

#[tokio::test]
async fn a_configured_title_wins_over_the_document() {
    let service = served(Docs::scalar().title("Widgets"));

    assert!(
        get(&service, "/docs")
            .call()
            .await
            .text()
            .contains("Widgets"),
        "the configured title did not reach the page",
    );
}

#[tokio::test]
async fn either_docs_route_answers_only_get() {
    // These are hand-written endpoints rather than `#[kynos::get]` expansions,
    // because the paths are configured at run time -- so `method()` is their
    // own claim and is worth checking.
    let service = served(Docs::scalar());

    for path in ["/docs", "/openapi.json"] {
        let reply = post(&service, path).call().await;

        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{path}");
        assert_eq!(reply.field("allow").as_deref(), Some("GET"), "{path}");
    }
}

/// A malformed docs path is reported, not thrown.
///
/// `Docs::at` used to panic on a literal `PathTemplate` could not parse, which
/// made a path written at a mount site the one malformed path in a description
/// that stopped the program rather than being collected with the rest.
/// `Group::new` already recorded a `Violation` for the same situation, and this
/// is the case that pins the two to one answer.
#[test]
fn a_docs_path_that_is_not_a_template_is_reported_rather_than_panicking() {
    // No leading slash, which is `MissingLeadingSlash` and the simplest thing
    // a Paths key can be wrong about.
    let router = support::router().docs(Docs::scalar().at("docs"));

    let violations = router.validate().expect("validation itself succeeds");
    assert!(
        violations.iter().any(|violation| matches!(
            &violation.error,
            kynos::openapi::SpecError::InvalidPathTemplate { template, .. } if template == "docs"
        )),
        "expected the bad path among the violations, got {violations:?}"
    );

    // And the router refuses to build, rather than serving a reference at a
    // path it could not describe.
    assert!(router.build(App::new()).is_err());
}
