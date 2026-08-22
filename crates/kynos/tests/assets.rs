//! Serving files, and the document that describes them.
//!
//! One reason: an asset set is a *routing* surface rather than an interceptor,
//! so what it registers and what it emits are only visible once a router is
//! built from one.

#![cfg(all(feature = "macros", feature = "assets"))]

use kynos::{
    Router,
    http::{Method, StatusCode, header},
    router::group::Group,
};

#[path = "support/mod.rs"]
mod support;

use support::{get, send};

kynos::assets! {
    /// The fixture set: two indexes, one stylesheet, and a source map excluded.
    struct Fixture;
    dir = "tests/assets",
    exclude = [".map"],
}

/// A router serving the fixture under `/static`.
fn served() -> Router<()> {
    Router::<()>::new().group(Group::new("/static").mount(Fixture::assets()))
}

// --- What was embedded ----------------------------------------------------

/// The excluded file is not in the binary, and the rest are.
#[test]
fn the_set_holds_what_the_directory_held_minus_what_was_excluded() {
    let paths: Vec<&str> = Fixture::ASSETS.iter().map(|asset| asset.path()).collect();

    assert_eq!(paths, ["css/app.css", "docs/index.html", "index.html"]);
    assert_eq!(Fixture::COUNT, 3);
    assert!(Fixture::TOTAL_BYTES > 0);
}

/// The order is sorted, so the emitted document is byte-identical across
/// machines.
///
/// A document that differed by directory-read order is one a `--check` mode
/// could not use, which is the whole reason the walk sorts.
#[test]
fn the_set_is_in_a_stable_order() {
    let mut sorted: Vec<&str> = Fixture::ASSETS.iter().map(|asset| asset.path()).collect();
    let listed = sorted.clone();
    sorted.sort_unstable();

    assert_eq!(listed, sorted);
}

// --- What is served -------------------------------------------------------

#[tokio::test]
async fn a_file_is_served_with_the_media_type_its_name_implies() {
    let service = served().build(()).expect("a describable router");

    let reply = get(&service, "/static/css/app.css").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::CONTENT_TYPE.as_str()).as_deref(),
        Some("text/css; charset=utf-8")
    );
    assert_eq!(reply.text(), "body { margin: 0 }\n");
}

/// The index is served at its own path *and* at the directory it indexes.
#[tokio::test]
async fn an_index_is_served_at_both_urls_it_answers_for() {
    let service = served().build(()).expect("a describable router");

    for path in ["/static/index.html", "/static/"] {
        let reply = get(&service, path).call().await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(reply.text().contains("<title>Kynos</title>"), "{path}");
    }

    for path in ["/static/docs/index.html", "/static/docs/"] {
        assert!(
            get(&service, path).call().await.text().contains("Docs"),
            "{path}"
        );
    }
}

/// An excluded file is not served, because it was never embedded.
#[tokio::test]
async fn an_excluded_file_is_not_served() {
    let service = served().build(()).expect("a describable router");

    assert_eq!(
        get(&service, "/static/app.css.map").call().await.status,
        StatusCode::NOT_FOUND
    );
}

/// Only `GET`. A `POST` to a file is a 405, with the `Allow` a 405 owes.
#[tokio::test]
async fn a_file_answers_only_the_method_it_declares() {
    let service = served().build(()).expect("a describable router");

    let reply = send(&service, Method::POST, "/static/css/app.css")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.field(header::ALLOW.as_str()).as_deref(), Some("GET"));
}

// --- Conditional requests -------------------------------------------------

/// A client holding the current tag is told so rather than sent the bytes.
#[tokio::test]
async fn a_client_holding_the_current_tag_receives_no_body() {
    let service = served().build(()).expect("a describable router");

    let first = get(&service, "/static/css/app.css").call().await;
    let etag = first.field(header::ETAG.as_str()).expect("an entity tag");

    let second = get(&service, "/static/css/app.css")
        .header("if-none-match", &etag)
        .call()
        .await;

    assert_eq!(second.status, StatusCode::NOT_MODIFIED);
    assert!(second.body.is_empty());
    // RFC 9110 section 15.4.5: a 304 carries the validator it matched on.
    assert_eq!(second.field(header::ETAG.as_str()).as_deref(), Some(&*etag));
}

/// A stale tag gets the bytes.
#[tokio::test]
async fn a_client_holding_a_stale_tag_receives_the_file() {
    let service = served().build(()).expect("a describable router");

    let reply = get(&service, "/static/css/app.css")
        .header("if-none-match", "\"something-else\"")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(!reply.body.is_empty());
}

/// `*` matches whatever the server has, per RFC 9110 section 13.1.2.
#[tokio::test]
async fn a_wildcard_precondition_matches_anything_served() {
    let service = served().build(()).expect("a describable router");

    assert_eq!(
        get(&service, "/static/css/app.css")
            .header("if-none-match", "*")
            .call()
            .await
            .status,
        StatusCode::NOT_MODIFIED
    );
}

/// The comparison is weak, so `W/"x"` and `"x"` are the same representation.
#[tokio::test]
async fn the_precondition_comparison_is_weak() {
    let service = served().build(()).expect("a describable router");

    let etag = get(&service, "/static/css/app.css")
        .call()
        .await
        .field(header::ETAG.as_str())
        .expect("an entity tag");

    assert_eq!(
        get(&service, "/static/css/app.css")
            .header("if-none-match", &format!("W/{etag}"))
            .call()
            .await
            .status,
        StatusCode::NOT_MODIFIED
    );
}

/// A list of tags matches if any member does.
#[tokio::test]
async fn a_list_of_tags_matches_on_any_member() {
    let service = served().build(()).expect("a describable router");

    let etag = get(&service, "/static/css/app.css")
        .call()
        .await
        .field(header::ETAG.as_str())
        .expect("an entity tag");

    assert_eq!(
        get(&service, "/static/css/app.css")
            .header("if-none-match", &format!("\"other\", {etag}"))
            .call()
            .await
            .status,
        StatusCode::NOT_MODIFIED
    );
}

// --- What the document says -----------------------------------------------

/// Every served path is a literal `paths` key, and the document is
/// authoritative: nothing here is waived.
#[test]
fn every_file_is_a_described_operation() {
    let document = served().openapi().expect("a describable router");

    let mut keys: Vec<&str> = document.paths.0.keys().map(String::as_str).collect();
    keys.sort_unstable();

    assert_eq!(
        keys,
        [
            "/static/",
            "/static/css/app.css",
            "/static/docs/",
            "/static/docs/index.html",
            "/static/index.html",
        ]
    );

    assert!(
        document.is_authoritative(),
        "an embedded set waives nothing, so the document is authoritative"
    );
}

/// Each operation declares the two statuses it can produce, and no more.
#[test]
fn each_operation_declares_a_success_and_a_not_modified() {
    let document = served().openapi().expect("a describable router");
    let operation = document.paths.0["/static/css/app.css"]
        .get
        .as_ref()
        .expect("a GET");

    let mut statuses: Vec<&str> = operation
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();
    statuses.sort_unstable();

    assert_eq!(statuses, ["200", "304"]);

    // The 200 says what the file is, and carries the validator that makes the
    // 304 reachable.
    let ok = operation.responses.responses["200"]
        .as_item()
        .expect("an inline 200");
    assert!(ok.content.contains_key("text/css; charset=utf-8"));
    assert!(ok.headers.contains_key("ETag"));
    assert!(ok.headers.contains_key("Cache-Control"));

    let not_modified = operation.responses.responses["304"]
        .as_item()
        .expect("an inline 304");
    assert!(not_modified.headers.contains_key("ETag"));
}

/// Two files get two operation ids, and each is derived from its own path.
#[test]
fn every_operation_has_an_identifier_of_its_own() {
    let document = served().openapi().expect("a describable router");

    let mut ids: Vec<&str> = document
        .paths
        .0
        .values()
        .filter_map(|item| item.get.as_ref())
        .filter_map(|operation| operation.operation_id.as_deref())
        .collect();

    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(ids.len(), before, "two operations share an identifier");
    // Derived from the path rather than counted, so it does not move when a
    // file is added beside it — and reads as the file it serves.
    assert_eq!(
        ids,
        [
            "asset_css_app_css",
            "asset_docs",
            "asset_docs_index_html",
            "asset_index",
            "asset_index_html",
        ]
    );
}
