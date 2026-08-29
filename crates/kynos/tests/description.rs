//! Scope in the router is scope in the document.
//!
//! One reason: `docs/middleware.md` states it as a rule — *"an interceptor
//! mounted on a subtree covers the operations in that subtree and nothing
//! else"* — and nothing in the type system enforces it. A `describe` pass that
//! visited every operation instead of the covered ones would still produce a
//! document that validates, and every other test in the suite would still pass.
//!
//! What is asserted is the declaration rather than the behaviour: the two are
//! the same associated types by construction, and the *behaviour* is already
//! covered by `limits.rs` and `interceptors.rs`. This is about which operations
//! the declaration reaches.
//!
//! The second half of the file is the same question one level down: which
//! *statuses* within an operation a declaration reaches. A response field
//! declared on a status that gives it no meaning is the same silent error as an
//! interceptor declared on an operation it does not cover.

#![cfg(all(feature = "macros", feature = "json"))]

use std::collections::BTreeSet;

use kynos::{
    Router,
    error::rejection::RangeRejection,
    extract::{body::binary::Binary, media::OctetStream},
    middleware::limits::{BodySize, Timeout},
    openapi::Document,
    response::{
        range::{Range, Ranged},
        status::NoContent,
    },
    router::group::Group,
};

#[kynos::get("/alpha")]
async fn alpha() -> NoContent {
    NoContent
}

#[kynos::get("/beta")]
async fn beta() -> NoContent {
    NoContent
}

#[kynos::get("/gamma")]
async fn gamma() -> NoContent {
    NoContent
}

/// Every `paths` key whose operation declares `status`.
fn declaring(document: &Document, status: &str) -> BTreeSet<String> {
    document
        .paths
        .items
        .iter()
        .filter(|(_, item)| {
            item.operations()
                .any(|(_, operation)| operation.responses.responses.contains_key(status))
        })
        .map(|(path, _)| path.clone())
        .collect()
}

fn paths(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Router scope reaches every operation, including ones mounted afterwards.
///
/// The order matters: an implementation collecting interceptors at `mount`
/// time rather than at `describe` time would cover `alpha` and miss `beta`.
#[test]
fn a_router_scoped_interceptor_declares_on_every_operation_either_side_of_it() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta])
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/alpha", "/beta"]));
}

/// Group scope reaches the group and stops there.
#[test]
fn a_group_scoped_interceptor_declares_on_its_group_alone() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .group(
            Group::<()>::new("/inner")
                .intercept(BodySize::new(1024))
                .mount(kynos::routes![beta]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/inner/beta"]));
}

/// Endpoint scope reaches one operation.
///
/// The innermost of the three, and the one where an over-broad `describe` pass
/// would be least visible: a document declaring 413 on two operations instead
/// of one still validates.
#[test]
fn an_endpoint_scoped_interceptor_declares_on_its_endpoint_alone() {
    let document = Router::<()>::new()
        .mount((
            kynos::routes![alpha],
            kynos::routes![beta].0.intercept(BodySize::new(1024)),
        ))
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/beta"]));
}

/// Two scopes compose: an operation under both declares both.
#[test]
fn an_operation_under_two_scopes_declares_what_each_contributes() {
    let document = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![alpha])
        .group(
            Group::<()>::new("/inner")
                .intercept(Timeout::new(std::time::Duration::from_secs(1)))
                .mount(kynos::routes![beta]),
        )
        .mount(kynos::routes![gamma])
        .openapi()
        .expect("a describable router");

    // The router's limit reaches all three.
    assert_eq!(
        declaring(&document, "413"),
        paths(&["/alpha", "/inner/beta", "/gamma"])
    );
    // The group's reaches one, and the one it reaches has both.
    assert_eq!(declaring(&document, "408"), paths(&["/inner/beta"]));
}

/// A nested router carries its own interceptors to exactly what it held.
///
/// `nest` and `merge` are the two ways one router absorbs another, and an
/// absorbed router's interceptors have to become part of what each of *its*
/// operations carries rather than of what the absorbing router applies to all
/// of them.
#[test]
fn a_nested_routers_interceptor_stays_with_what_that_router_held() {
    let inner = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta]);

    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .nest("/v1", inner)
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/v1/beta"]));
}

/// The same, through `merge`, which absorbs without a prefix.
#[test]
fn a_merged_routers_interceptor_stays_with_what_that_router_held() {
    let other = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta]);

    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .merge(other)
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/beta"]));
}

/// The control for all of the above: with no interceptor anywhere, no operation
/// declares the status. Without it every assertion here would pass against a
/// `describe` pass that declared nothing at all.
#[test]
fn nothing_declares_a_limits_status_when_no_limit_is_mounted() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha, beta, gamma])
        .openapi()
        .expect("a describable router");

    assert!(declaring(&document, "413").is_empty());
    assert!(declaring(&document, "408").is_empty());
}

// --- Scope in a status ------------------------------------------------------
//
// The same failure on a different axis. An interceptor's declaration can reach
// operations it does not cover; a response field's can reach statuses it has no
// meaning on. RFC 9110 section 14.4 says `Content-Range` "has no meaning for
// status codes that do not explicitly describe its semantic", and only 206 and
// 416 do — so a `WithHeaders` group, which joins every response the body
// declares, is the wrong tool and these assertions are what say so.

/// Serves a fixed recording, resumably.
#[kynos::get("/recordings/current")]
async fn recording(
    range: Range<Binary<OctetStream>>,
) -> Result<Ranged<Binary<OctetStream>>, RangeRejection> {
    range.apply(Binary::new(&b"0123456789"[..]))
}

/// The control: the same body, without the range.
#[kynos::get("/recordings/whole")]
async fn whole_recording() -> Binary<OctetStream> {
    Binary::new(&b"0123456789"[..])
}

/// Reads the field and answers whole anyway, which RFC 9110 section 14.2 allows
/// outright: *a server MAY ignore the Range header field*.
///
/// The return type cannot fail, so no request to this operation can produce a
/// 416 — and the description must not claim one.
#[kynos::get("/recordings/preview")]
async fn preview_recording(range: Range<Binary<OctetStream>>) -> Binary<OctetStream> {
    let _ = range;
    Binary::new(&b"0123456789"[..])
}

/// The one operation under `path`.
fn operation(document: &Document, path: &str) -> kynos::openapi::Operation {
    let item = document.paths.items.get(path).expect("a described path");
    let (_, operation) = item.operations().next().expect("one operation");
    operation.clone()
}

/// The header names declared on `status`, or `None` when it is not declared.
fn declared_headers(operation: &kynos::openapi::Operation, status: &str) -> Option<Vec<String>> {
    let kynos::openapi::RefOr::Item(response) = operation.responses.responses.get(status)? else {
        panic!("{status} is described as a `$ref`");
    };
    Some(response.headers.keys().cloned().collect())
}

/// The `Range` field is a parameter, which is the whole reason it is not
/// `Accept`: a consumer that cannot see it does not know the operation resumes.
#[test]
fn a_ranged_operation_declares_the_field_it_reads() {
    let document = Router::<()>::new()
        .mount(kynos::routes![
            recording,
            whole_recording,
            preview_recording
        ])
        .openapi()
        .expect("a describable router");

    let ranged = operation(&document, "/recordings/current");
    let declared = ranged.parameters.iter().find(
        |parameter| matches!(parameter, kynos::openapi::RefOr::Item(item) if item.name == "Range"),
    );
    let Some(kynos::openapi::RefOr::Item(parameter)) = declared else {
        panic!("the `Range` field is declared, and inline rather than as a `$ref`");
    };

    assert_eq!(parameter.location, kynos::openapi::ParameterIn::Header);
    assert_ne!(parameter.required, Some(true));

    let kynos::openapi::Schema::Object(schema) = parameter.schema().expect("a schema") else {
        panic!("described by a boolean schema");
    };
    let pattern = schema.pattern.clone().expect("a pattern");
    assert!(pattern.starts_with("^bytes="), "{pattern}");

    // The control declares no such parameter.
    let whole = operation(&document, "/recordings/whole");
    assert!(whole.parameters.is_empty());
}

/// The three statuses, and each field on exactly the statuses that give it a
/// meaning.
#[test]
fn a_ranged_operation_declares_each_field_on_the_statuses_that_carry_it() {
    let document = Router::<()>::new()
        .mount(kynos::routes![
            recording,
            whole_recording,
            preview_recording
        ])
        .openapi()
        .expect("a describable router");

    let ranged = operation(&document, "/recordings/current");

    // A set: which statuses are declared is the contract, and the order the map
    // holds them in is the order they were contributed.
    let statuses: BTreeSet<&str> = ranged
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(statuses, BTreeSet::from(["200", "206", "416"]));

    assert_eq!(
        declared_headers(&ranged, "200"),
        Some(vec!["Accept-Ranges".to_owned()])
    );
    assert_eq!(
        declared_headers(&ranged, "206"),
        Some(vec!["Accept-Ranges".to_owned(), "Content-Range".to_owned()])
    );
    assert_eq!(
        declared_headers(&ranged, "416"),
        Some(vec!["Content-Range".to_owned()])
    );
}

/// The control: an operation that does not range advertises nothing.
///
/// Without it every assertion above would pass against a `describe` pass that
/// put `Accept-Ranges` on every response in the service.
#[test]
fn an_operation_that_does_not_range_advertises_no_unit() {
    let document = Router::<()>::new()
        .mount(kynos::routes![
            recording,
            whole_recording,
            preview_recording
        ])
        .openapi()
        .expect("a describable router");

    let whole = operation(&document, "/recordings/whole");

    assert_eq!(
        whole
            .responses
            .responses
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["200"])
    );
    assert_eq!(declared_headers(&whole, "200"), Some(Vec::new()));
}

/// A handler that reads a `Range` but cannot fail declares no 416.
///
/// The 416 belongs to `RangeRejection`, which `Range::apply` raises — not to
/// reading the field, which RFC 9110 section 14.2 makes infallible. An
/// operation that declared it from the *argument* would advertise a status no
/// request to it can reach, which is the shape `docs/testing.md` records the
/// conformance harness finding on its first run: a 413 declared by every
/// body-reading operation and produced by none.
///
/// The `Range` parameter is still declared, and `Accept-Ranges` still is not:
/// this operation reads the field and answers whole, so it advertises no unit.
#[test]
fn an_operation_that_reads_a_range_but_cannot_fail_declares_no_416() {
    let document = Router::<()>::new()
        .mount(kynos::routes![
            recording,
            whole_recording,
            preview_recording
        ])
        .openapi()
        .expect("a describable router");

    let preview = operation(&document, "/recordings/preview");

    assert!(
        preview
            .parameters
            .iter()
            .any(|parameter| matches!(parameter, kynos::openapi::RefOr::Item(item) if item.name == "Range")),
        "the field it reads is still declared"
    );

    assert_eq!(
        preview
            .responses
            .responses
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["200"])
    );
    assert_eq!(declared_headers(&preview, "200"), Some(Vec::new()));

    // And the operation that *can* fail still declares it, so this is not a
    // pass against a description that stopped declaring the 416 anywhere.
    let ranged = operation(&document, "/recordings/current");
    assert!(ranged.responses.responses.contains_key("416"));
}

// --- `#[deprecated]` on a handler -------------------------------------------

/// Superseded by `beta`.
#[deprecated(note = "use `beta`")]
#[kynos::get("/retired")]
async fn retired() -> NoContent {
    NoContent
}

/// One rule reaches every place a description says `deprecated`.
///
/// The handler attribute and the `Schema` derive both read Rust's own
/// `#[deprecated]` through the same helper, so an operation and a field cannot
/// answer the question differently. Nothing covered the operation half before,
/// which is how it survived the helper moving.
#[test]
fn a_deprecated_handler_marks_its_operation() {
    #[allow(deprecated)]
    let document = Router::<()>::new()
        .mount(kynos::routes![retired, alpha])
        .openapi()
        .expect("the fixture describes itself");

    assert_eq!(
        operation(&document, "/retired").deprecated,
        Some(true),
        "a deprecated handler reached the document unmarked"
    );

    // The control: an operation nobody deprecated says nothing, rather than
    // saying `false` in every description Kynos emits.
    assert_eq!(operation(&document, "/alpha").deprecated, None);
}

// -- language negotiation, and the statuses it reaches ------------------------

/// Three languages, which is enough for an enumeration to be worth reading.
struct Supported;

impl kynos::response::language::Languages for Supported {
    const TAGS: &'static [&'static str] = &["en", "fr", "de"];
}

/// A failure with a status of its own, so the success and the error arm can be
/// told apart in the emitted responses.
#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[problem(status = 404, title = "No greeting")]
#[error("no greeting for that language")]
struct NoGreeting;

#[kynos::get("/greeting")]
async fn greeting(
    preferred: kynos::response::language::AcceptLanguage<Supported>,
) -> Result<
    kynos::response::language::Localized<kynos::extract::body::text::Text, Supported>,
    NoGreeting,
> {
    Ok(preferred.respond_with(|language| {
        kynos::extract::body::text::Text(
            match language {
                "fr" => "Bonjour",
                "de" => "Guten Tag",
                _ => "Hello",
            }
            .to_owned(),
        )
    }))
}

/// The control: the same handler without the negotiation.
#[kynos::get("/plain")]
async fn plain() -> kynos::extract::body::text::Text {
    kynos::extract::body::text::Text("Hello".to_owned())
}

fn negotiating_document() -> Document {
    Router::<()>::new()
        .mount(kynos::routes![greeting, plain])
        .openapi()
        .expect("a describable router")
}

/// `Accept-Language` is a parameter where `Accept` is not, because OpenAPI
/// ignores a definition for exactly three fields and this is not one of them.
#[test]
fn a_localized_operation_declares_the_field_it_reads() {
    let document = negotiating_document();
    let localized = operation(&document, "/greeting");

    let declared = localized.parameters.iter().find(|parameter| {
        matches!(parameter, kynos::openapi::RefOr::Item(item) if item.name == "Accept-Language")
    });
    let Some(kynos::openapi::RefOr::Item(parameter)) = declared else {
        panic!("the `Accept-Language` field is declared, and inline rather than as a `$ref`");
    };

    assert_eq!(parameter.location, kynos::openapi::ParameterIn::Header);
    assert_ne!(parameter.required, Some(true));

    let kynos::openapi::Schema::Object(schema) = parameter.schema().expect("a schema") else {
        panic!("described by a boolean schema");
    };

    // No enumeration: the value is a priority list, not a tag, so a set of
    // offered tags would be a claim no client could satisfy.
    assert!(
        schema.enumeration.is_none(),
        "the offer is enumerated on `Content-Language`, not here"
    );
    // And no pattern: an unreadable range is dropped rather than refused, so a
    // pattern would document a rejection the service never makes.
    assert!(schema.pattern.is_none(), "{:?}", schema.pattern);

    // The control declares no such parameter.
    assert!(operation(&document, "/plain").parameters.is_empty());
}

/// The offer is stated where it is true, on the field that carries one tag.
#[test]
fn a_localized_operation_enumerates_its_offer_on_the_language_it_answers_in() {
    let document = negotiating_document();
    let localized = operation(&document, "/greeting");

    let kynos::openapi::RefOr::Item(response) = localized
        .responses
        .responses
        .get("200")
        .expect("a described success")
    else {
        panic!("the 200 is described as a `$ref`");
    };
    let kynos::openapi::RefOr::Item(field) = response
        .headers
        .get("Content-Language")
        .expect("the language is declared on the success")
    else {
        panic!("the field is described as a `$ref`");
    };

    // Required, which is what makes serving a default instead of a 406 honest:
    // a client that cannot use the fallback can always see what it got.
    assert_eq!(field.required, Some(true));

    let (_, media) = field.content().expect("a content-described header");
    let kynos::openapi::Schema::Object(schema) = media.schema.clone().expect("a schema") else {
        panic!("described by a boolean schema");
    };
    let enumerated: Vec<String> = schema
        .enumeration
        .expect("the offer is enumerated")
        .into_iter()
        .map(|value| value.as_str().expect("a string tag").to_owned())
        .collect();

    assert_eq!(enumerated, ["en", "fr", "de"]);
}

/// The field reaches the statuses the body declares and no others.
#[test]
fn a_localized_operation_states_its_language_on_the_statuses_that_carry_it() {
    let document = negotiating_document();
    let localized = operation(&document, "/greeting");

    // No wildcard entry was minted. A `2XX` beside a declared `200` is a key no
    // reader of the 200 resolves, and a response the service cannot produce.
    let statuses: BTreeSet<&str> = localized
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(statuses, BTreeSet::from(["200", "404"]));

    assert_eq!(
        declared_headers(&localized, "200"),
        Some(vec!["Content-Language".to_owned()])
    );

    // The error arm carries none: a problem document is not localized by
    // `Localized`, and claiming otherwise would be a promise nothing keeps.
    assert_eq!(declared_headers(&localized, "404"), Some(Vec::new()));
}

/// Negotiating a language adds no status to an operation.
#[test]
fn a_localized_operation_declares_no_status_it_would_not_otherwise_send() {
    let document = negotiating_document();

    let greeting = operation(&document, "/greeting");
    let plain = operation(&document, "/plain");

    let negotiated: BTreeSet<&str> = greeting
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();
    let control: BTreeSet<&str> = plain
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();

    // The only difference is the error type the handler names, which is the
    // control's whole job here: no 406 and no 400 came from the negotiation.
    assert_eq!(
        negotiated.difference(&control).copied().collect::<Vec<_>>(),
        ["404"]
    );
}

/// `Vary` is merged onto the wire and never described.
#[test]
fn a_localized_operation_never_declares_the_field_it_varies_on() {
    let document = negotiating_document();
    let localized = operation(&document, "/greeting");

    for (status, response) in &localized.responses.responses {
        let kynos::openapi::RefOr::Item(response) = response else {
            panic!("{status} is described as a `$ref`");
        };
        assert!(
            !response
                .headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("vary")),
            "{status} describes `Vary`, which a client generator has no use for"
        );
    }
}
