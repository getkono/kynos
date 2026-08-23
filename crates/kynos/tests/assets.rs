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
    // Summed rather than compared against a literal: what is worth asserting is
    // that the constant counts the bytes that were actually embedded, and a
    // literal would only restate the fixture's current size.
    assert_eq!(
        Fixture::TOTAL_BYTES,
        Fixture::ASSETS
            .iter()
            .map(|asset| asset.bytes().len())
            .sum::<usize>()
    );
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

// --- Byte ranges ----------------------------------------------------------

/// The stylesheet, whole. Every range case below is an offset into this.
const STYLESHEET: &str = "body { margin: 0 }\n";

/// A whole representation advertises the unit and names no part.
///
/// Section 14.4 gives `Content-Range` no meaning on a 200, and section 14.3
/// makes `Accept-Ranges` what tells a client the resumable download exists.
#[tokio::test]
async fn a_file_served_whole_advertises_that_it_ranges() {
    let service = served().build(()).expect("a describable router");

    let reply = get(&service, "/static/css/app.css").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.field(header::ACCEPT_RANGES.as_str()).as_deref(),
        Some("bytes")
    );
    assert_eq!(reply.field(header::CONTENT_RANGE.as_str()), None);
    assert_eq!(reply.text(), STYLESHEET);
}

/// A 206 carries exactly the octets its `Content-Range` names.
///
/// Both are asserted together because the failure worth catching is the two
/// disagreeing: section 14.4 forbids a client from recombining an invalid
/// field, so a correct field naming the wrong octets corrupts the client's copy
/// silently.
#[tokio::test]
async fn a_range_request_receives_the_bytes_it_named() {
    let service = served().build(()).expect("a describable router");

    for (field, first, last) in [
        ("bytes=5-10", 5, 10),
        // A suffix reaches the end, and an open-ended range the remainder.
        ("bytes=-4", 15, 18),
        ("bytes=12-", 12, 18),
        // A last offset past the end clamps rather than failing.
        ("bytes=17-99", 17, 18),
    ] {
        let reply = get(&service, "/static/css/app.css")
            .header("range", field)
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT, "{field}");
        assert_eq!(
            reply.field(header::CONTENT_RANGE.as_str()).as_deref(),
            Some(format!("bytes {first}-{last}/19").as_str()),
            "{field}"
        );
        assert_eq!(reply.text(), &STYLESHEET[first..=last], "{field}");
        // Section 15.3.7: a 206 carries the representation fields a 200 would.
        assert!(reply.field(header::ETAG.as_str()).is_some(), "{field}");
        assert_eq!(
            reply.field(header::CONTENT_TYPE.as_str()).as_deref(),
            Some("text/css; charset=utf-8"),
            "{field}"
        );
    }
}

/// A field naming nothing satisfiable is section 15.5.17's 416, stating how
/// long the representation actually is.
#[tokio::test]
async fn an_unsatisfiable_range_states_the_complete_length() {
    let service = served().build(()).expect("a describable router");

    let reply = get(&service, "/static/css/app.css")
        .header("range", "bytes=100-200")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        reply.field(header::CONTENT_RANGE.as_str()).as_deref(),
        Some("bytes */19")
    );
}

/// A field Kynos cannot apply is ignored, which is the whole file and a 200.
#[tokio::test]
async fn an_unusable_range_field_is_ignored_rather_than_refused() {
    let service = served().build(()).expect("a describable router");

    for field in ["items=0-1", "bytes=nonsense", "bytes=5-1"] {
        let reply = get(&service, "/static/css/app.css")
            .header("range", field)
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK, "{field}");
        assert_eq!(reply.text(), STYLESHEET, "{field}");
    }
}

/// Section 14.2: `Range` is evaluated *only if the result in absence of the
/// Range header field would be a 200*, so a matched precondition wins.
#[tokio::test]
async fn a_matched_precondition_beats_a_range() {
    let service = served().build(()).expect("a describable router");

    let etag = get(&service, "/static/css/app.css")
        .call()
        .await
        .field(header::ETAG.as_str())
        .expect("an entity tag");

    let reply = get(&service, "/static/css/app.css")
        .header("if-none-match", &etag)
        .header("range", "bytes=0-3")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);
    assert!(reply.body.is_empty());
}

/// Section 13.1.5: an `If-Range` naming this representation honours the range.
#[tokio::test]
async fn an_if_range_naming_this_representation_honours_the_range() {
    let service = served().build(()).expect("a describable router");

    let etag = get(&service, "/static/css/app.css")
        .call()
        .await
        .field(header::ETAG.as_str())
        .expect("an entity tag");
    assert!(!etag.starts_with("W/"), "an embedded tag is strong: {etag}");

    let reply = get(&service, "/static/css/app.css")
        .header("if-range", &etag)
        .header("range", "bytes=0-3")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(reply.text(), &STYLESHEET[0..=3]);
}

/// A condition that does not hold ignores the `Range` and sends the whole
/// representation, which is the point of `If-Range`: the client is replacing
/// its copy rather than splicing a part into a stale one.
///
/// The weak spelling of the *current* tag is in the list deliberately. Section
/// 13.1.5 takes the strong comparison, under which `W/"x"` matches nothing at
/// all — which is exactly why the `assets-fs` mode, whose tag is weak by
/// construction, can never honour an `If-Range`.
#[tokio::test]
async fn an_if_range_that_does_not_hold_sends_the_whole_file() {
    let service = served().build(()).expect("a describable router");

    let etag = get(&service, "/static/css/app.css")
        .call()
        .await
        .field(header::ETAG.as_str())
        .expect("an entity tag");

    for condition in [
        "\"something-else\"".to_owned(),
        format!("W/{etag}"),
        // An `HTTP-date` is compared against `Last-Modified`, which Kynos never
        // sends, so it can never match either.
        "Wed, 15 Nov 1995 04:58:08 GMT".to_owned(),
    ] {
        let reply = get(&service, "/static/css/app.css")
            .header("if-range", &condition)
            .header("range", "bytes=0-3")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK, "{condition}");
        assert_eq!(reply.text(), STYLESHEET, "{condition}");
        assert_eq!(
            reply.field(header::CONTENT_RANGE.as_str()),
            None,
            "{condition}"
        );
    }
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

/// Each operation declares the four statuses it can produce, and no more.
#[test]
fn each_operation_declares_every_status_a_file_can_answer_with() {
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

    assert_eq!(statuses, ["200", "206", "304", "416"]);

    // The 200 says what the file is, carries the validator that makes the 304
    // reachable, and advertises the unit that makes the 206 askable for.
    let ok = operation.responses.responses["200"]
        .as_item()
        .expect("an inline 200");
    assert!(ok.content.contains_key("text/css; charset=utf-8"));
    assert!(ok.headers.contains_key("ETag"));
    assert!(ok.headers.contains_key("Cache-Control"));
    assert!(ok.headers.contains_key("Accept-Ranges"));

    // Section 15.3.7 requires a 206 to carry what the 200 would have, and
    // section 15.3.7.1 to name the part it encloses.
    let partial = operation.responses.responses["206"]
        .as_item()
        .expect("an inline 206");
    assert!(partial.content.contains_key("text/css; charset=utf-8"));
    assert!(partial.headers.contains_key("ETag"));
    assert!(partial.headers.contains_key("Cache-Control"));
    assert!(partial.headers.contains_key("Accept-Ranges"));
    assert!(partial.headers.contains_key("Content-Range"));

    let not_modified = operation.responses.responses["304"]
        .as_item()
        .expect("an inline 304");
    assert!(not_modified.headers.contains_key("ETag"));
    // Section 14.3 advertises a range of a representation, and a 304 carries
    // none -- so the field is not declared where it is not sent.
    assert!(!not_modified.headers.contains_key("Accept-Ranges"));

    // Section 15.5.17's 416 states the complete length instead of a part.
    let unsatisfiable = operation.responses.responses["416"]
        .as_item()
        .expect("an inline 416");
    assert!(unsatisfiable.headers.contains_key("Content-Range"));
}

/// Every request field the file reads is declared, and nothing else is.
#[test]
fn each_operation_declares_the_fields_it_reads() {
    let document = served().openapi().expect("a describable router");
    let operation = document.paths.0["/static/css/app.css"]
        .get
        .as_ref()
        .expect("a GET");

    let mut names: Vec<&str> = operation
        .parameters
        .iter()
        .filter_map(|parameter| parameter.as_item())
        .map(|parameter| parameter.name.as_str())
        .collect();
    names.sort_unstable();

    assert_eq!(names, ["If-None-Match", "If-Range", "Range"]);
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

// --- The filesystem mode --------------------------------------------------

#[cfg(feature = "assets-fs")]
mod directory {
    use kynos::{
        Router,
        http::StatusCode,
        openapi::{OpaqueReason, OpaqueRoute},
        router::assets::fs::Directory,
    };

    use super::support::get;

    /// A router serving the same fixture directory from disk.
    fn served() -> Router<()> {
        Router::<()>::new().assets_directory("/files", Directory::new("tests/assets"))
    }

    #[tokio::test]
    async fn a_file_is_served_from_disk() {
        let service = served().build(()).expect("a buildable router");

        let reply = get(&service, "/files/css/app.css").call().await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.text(), "body { margin: 0 }\n");
        assert_eq!(
            reply
                .field(kynos::http::header::CONTENT_TYPE.as_str())
                .as_deref(),
            Some("text/css; charset=utf-8")
        );
    }

    /// A file the embedded set excluded is served here, because membership is
    /// the directory's rather than the macro's.
    ///
    /// The whole difference between the two modes, in one case.
    #[tokio::test]
    async fn a_file_the_embedded_set_excluded_is_still_on_disk() {
        let service = served().build(()).expect("a buildable router");

        assert_eq!(
            get(&service, "/files/app.css.map").call().await.status,
            StatusCode::OK
        );
    }

    /// A directory serves its index.
    #[tokio::test]
    async fn a_directory_serves_its_index() {
        let service = served().build(()).expect("a buildable router");

        assert!(
            get(&service, "/files/docs/")
                .call()
                .await
                .text()
                .contains("Docs")
        );
    }

    /// Traversal is refused, end to end.
    #[tokio::test]
    async fn a_request_climbing_out_of_the_root_is_refused() {
        let service = served().build(()).expect("a buildable router");

        for path in [
            "/files/../Cargo.toml",
            "/files/css/../../Cargo.toml",
            "/files/%2e%2e/Cargo.toml",
        ] {
            let reply = get(&service, path).call().await;
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
            assert!(reply.body.is_empty(), "{path}");
        }
    }

    /// A file that is not there is a 404, and so is one that cannot be read.
    #[tokio::test]
    async fn a_file_that_is_not_there_is_not_found() {
        let service = served().build(()).expect("a buildable router");

        assert_eq!(
            get(&service, "/files/nothing.css").call().await.status,
            StatusCode::NOT_FOUND
        );
    }

    /// Conditional requests work the same way they do for an embedded set.
    #[tokio::test]
    async fn a_client_holding_the_current_tag_receives_no_body() {
        let service = served().build(()).expect("a buildable router");

        let etag = get(&service, "/files/css/app.css")
            .call()
            .await
            .field(kynos::http::header::ETAG.as_str())
            .expect("an entity tag");

        // Weak, because it is derived from the length and mtime rather than
        // from the contents -- reading every file to hash it would be the work
        // a conditional request exists to avoid.
        assert!(etag.starts_with("W/"), "{etag}");

        let second = get(&service, "/files/css/app.css")
            .header("if-none-match", &etag)
            .call()
            .await;

        assert_eq!(second.status, StatusCode::NOT_MODIFIED);
        assert!(second.body.is_empty());
    }

    // --- Byte ranges -------------------------------------------------------

    /// A served file ranges the same way an embedded one does.
    ///
    /// The read is seeked rather than sliced, which no response can show — what
    /// this pins is that the octets and the field agree, which is the part a
    /// client acts on.
    #[tokio::test]
    async fn a_served_file_answers_the_part_it_was_asked_for() {
        let service = served().build(()).expect("a buildable router");

        let reply = get(&service, "/files/css/app.css")
            .header("range", "bytes=5-10")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            reply
                .field(kynos::http::header::CONTENT_RANGE.as_str())
                .as_deref(),
            Some("bytes 5-10/19")
        );
        assert_eq!(reply.text(), &super::STYLESHEET[5..=10]);
        assert_eq!(
            reply
                .field(kynos::http::header::ACCEPT_RANGES.as_str())
                .as_deref(),
            Some("bytes")
        );
    }

    /// An unsatisfiable field is a 416 that never reads the file at all: the
    /// `stat` already said how long it is.
    #[tokio::test]
    async fn an_unsatisfiable_range_is_refused_before_the_file_is_read() {
        let service = served().build(()).expect("a buildable router");

        let reply = get(&service, "/files/css/app.css")
            .header("range", "bytes=100-200")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            reply
                .field(kynos::http::header::CONTENT_RANGE.as_str())
                .as_deref(),
            Some("bytes */19")
        );
    }

    /// A weak validator can never satisfy an `If-Range`, so this mode always
    /// answers a conditional range with the whole file.
    ///
    /// Recorded rather than endorsed: it is a consequence of deriving the tag
    /// from a `stat` instead of from the contents, and RFC 9110 section 13.1.5
    /// takes the strong comparison. The control beneath it is the same request
    /// without the condition, which is served as a 206 — so this is the
    /// precondition failing rather than the mode not ranging.
    #[tokio::test]
    async fn a_weak_validator_never_satisfies_an_if_range() {
        let service = served().build(()).expect("a buildable router");

        let etag = get(&service, "/files/css/app.css")
            .call()
            .await
            .field(kynos::http::header::ETAG.as_str())
            .expect("an entity tag");
        assert!(etag.starts_with("W/"), "{etag}");

        let conditional = get(&service, "/files/css/app.css")
            .header("if-range", &etag)
            .header("range", "bytes=0-3")
            .call()
            .await;

        assert_eq!(conditional.status, StatusCode::OK);
        assert_eq!(conditional.text(), super::STYLESHEET);

        let unconditional = get(&service, "/files/css/app.css")
            .header("range", "bytes=0-3")
            .call()
            .await;

        assert_eq!(unconditional.status, StatusCode::PARTIAL_CONTENT);
    }

    // --- What the document says ------------------------------------------

    /// No `paths` key, and a record at the root a generator cannot act on.
    #[test]
    fn a_served_directory_is_recorded_rather_than_described() {
        let document = served().openapi().expect("a describable router");

        assert!(
            document.paths.0.is_empty(),
            "a directory took a `paths` key, which is a claim about paths it does not honour"
        );

        let recorded = OpaqueRoute::all(&document).expect("a readable record");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].pattern, "/files/{*path}");
        assert_eq!(recorded[0].prefix.as_deref(), Some("/files"));
        assert_eq!(recorded[0].reason, OpaqueReason::StaticAssets);
        assert!(
            recorded[0]
                .note
                .as_deref()
                .is_some_and(|note| note.contains("membership is not fixed")),
            "{:?}",
            recorded[0].note
        );

        assert!(!document.is_authoritative());
    }

    /// The reason is its own, so a CI gate can tolerate this waiver and no
    /// other.
    ///
    /// `UntypedRoute` would have been true of it and would read identically to
    /// a business API someone wildcarded. The two deserve different amounts of
    /// alarm.
    #[test]
    fn the_waiver_names_itself() {
        assert_eq!(served().unchecked_reasons(), [OpaqueReason::StaticAssets]);
    }
}
