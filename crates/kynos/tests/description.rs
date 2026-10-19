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
//! The second section is the same question one level down: which *statuses*
//! within an operation a declaration reaches. A response field declared on a
//! status that gives it no meaning is the same silent error as an interceptor
//! declared on an operation it does not cover.
//!
//! The third is the question on its remaining axis: which of the four tag
//! scopes reaches the document at all, and whether each one that puts a name on
//! an operation also puts that tag's metadata in the document's own `tags`. A
//! scope that pays one half and not the other produces a document that still
//! validates against everything else here.
//!
//! The fourth is what a status publishes when two contributors reach it. An
//! extractor's rejection and the handler's error type can both name 400, and
//! only one entry can be filed under that key -- so what the document says
//! there is a choice, and a document that made the wrong one would still
//! validate against everything else here.
//!
//! The fifth is what `deny_unchecked_schemas` reaches. A derived body is
//! registered as a component, so an `Unchecked` field sits two levels below
//! the media type that names it -- and a setting that only read the media
//! type's own schema would pass a router it promises to refuse.
//!
//! The sixth is which version the document claims. It follows from what the
//! API uses and never from the `openapi32` feature, so a router that bumped it
//! on the feature would still produce a document that validates.
//!
//! The seventh is what a router declares about itself rather than its
//! operations: its servers, which router's `info` and `externalDocs` a nest
//! publishes, and which policies a nested router may not set.
//!
//! The eighth is what an interceptor's `Reads` answers with when it cannot be
//! decoded. The 400 is sent before the interceptor runs, so nothing in its
//! `Short` names it, and a document that left it out would still validate.

#![cfg(all(feature = "macros", feature = "json"))]

use std::collections::BTreeSet;

use kynos::{
    ApiError, PathParams, Router, Schema,
    error::{RouterError, rejection::RangeRejection},
    extract::{body::binary::Binary, params::path::Path},
    http::media::OctetStream,
    middleware::limits::{body_size::BodySize, timeout::Timeout},
    openapi::Document,
    response::{
        range::{
            Range, Ranged,
            served::{Conditions, Delivery, Served},
            source::InMemory,
        },
        status::NoContent,
    },
    router::{endpoint::builder::EndpointBuilder, group::Group},
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
        .mount(kynos::routes![alpha, beta.intercept(BodySize::new(1024))])
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
    assert!(pattern.starts_with("^[Bb][Yy][Tt][Ee][Ss]="), "{pattern}");

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

/// Serves a fixed object as a named download with a stated lifetime: every
/// field `Served`'s builder can add.
#[kynos::get("/objects/current")]
async fn object(conditions: Conditions) -> Delivery<OctetStream> {
    Served::<_, OctetStream>::new(InMemory::new(bytes::Bytes::from_static(b"0123456789")))
        .etag(kynos::http::etag::ETag::strong("o1"))
        .cache_control("max-age=60")
        .attachment("object.bin")
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
}

/// A delivery declares every field it can send on the statuses that carry it.
///
/// `Cache-Control` rides the 304 as well, which RFC 9110 section 15.4.5 makes
/// a MUST wherever the 200 would carry it; `Content-Disposition` does not,
/// since the same section asks a 304 to carry no other metadata. Both are sent
/// only when the builder asks for them, so neither is required.
#[test]
fn a_delivery_declares_the_fields_its_builder_adds() {
    let document = Router::<()>::new()
        .mount(kynos::routes![object])
        .openapi()
        .expect("a describable router");

    let delivery = operation(&document, "/objects/current");
    let declared = |status: &str| -> BTreeSet<String> {
        declared_headers(&delivery, status)
            .expect("a described status")
            .into_iter()
            .collect()
    };
    let names = |names: &[&str]| -> BTreeSet<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    };

    let representation = [
        "Accept-Ranges",
        "Cache-Control",
        "Content-Disposition",
        "ETag",
        "Last-Modified",
    ];
    assert_eq!(declared("200"), names(&representation));
    assert_eq!(
        declared("206"),
        names(&[&representation[..], &["Content-Range"]].concat())
    );
    assert_eq!(
        declared("304"),
        names(&["Cache-Control", "ETag", "Last-Modified"])
    );

    for status in ["200", "206", "304"] {
        let Some(kynos::openapi::RefOr::Item(response)) = delivery.responses.responses.get(status)
        else {
            panic!("an inline {status}");
        };
        for name in ["Cache-Control", "Content-Disposition"] {
            if let Some(header) = response.headers.get(name) {
                let kynos::openapi::RefOr::Item(header) = header else {
                    panic!("{name} on {status} is described as a `$ref`");
                };
                assert_ne!(header.required, Some(true), "{name} on {status}");
            }
        }
    }
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

impl kynos::response::language::offer::Languages for Supported {
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

/// What a catalogue nothing wrote down can still describe.
///
/// A record of today's behaviour rather than an endorsement of it, paired with
/// the gap [`nfr.md`](../../../docs/nfr.md) documents: `Languages::TAGS` is a
/// `const`, so a catalogue discovered at startup cannot enumerate its offer.
/// The escape is `WithHeaders<T, ContentLanguage>`, which still states the
/// field and still varies on the request — it simply describes the value as an
/// unconstrained string. Closing the gap would turn this assertion red, which
/// is the point of writing it down.
#[test]
fn a_catalogue_no_const_can_name_still_declares_the_field_without_its_offer() {
    let document = Router::<()>::new()
        .mount(kynos::routes![resolved])
        .openapi()
        .expect("a describable router");

    let resolved = operation(&document, "/resolved");
    let kynos::openapi::RefOr::Item(response) = resolved
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
        .expect("the field is declared")
    else {
        panic!("the field is described as a `$ref`");
    };

    let (_, media) = field.content().expect("a content-described header");
    let kynos::openapi::Schema::Object(schema) = media.schema.clone().expect("a schema") else {
        panic!("described by a boolean schema");
    };

    // The offer is absent, which is exactly the gap: nothing wrote it down.
    assert!(
        schema.enumeration.is_none(),
        "a runtime catalogue cannot state its offer, and this records that"
    );
}

/// A handler whose language came from somewhere no `const` can see.
#[kynos::get("/resolved")]
async fn resolved() -> kynos::response::headers::WithHeaders<
    kynos::extract::body::text::Text,
    kynos::response::language::headers::ContentLanguage,
> {
    let tag = kynos::response::language::tag::LanguageTag::parse("fr").expect("well-formed");

    kynos::response::headers::WithHeaders::new(
        kynos::extract::body::text::Text("Bonjour".to_owned()),
        kynos::response::language::headers::ContentLanguage::new(&tag),
    )
}

/// A header group naming the same field as the negotiation contributes one
/// parameter, not two spellings of one.
///
/// RFC 9110 section 5.1: field names are case-insensitive. OpenAPI requires a
/// parameter to be unique by name and location, so emitting both
/// `Accept-Language` and `accept-language` is a document no consumer can read
/// as one field -- and `is_ignored_header_parameter` already folds case for
/// exactly this reason.
#[test]
fn a_header_named_twice_in_different_cases_is_one_parameter() {
    let document = Router::<()>::new()
        .mount(kynos::routes![doubly_declared])
        .openapi()
        .expect("a describable router");

    let declared = operation(&document, "/doubly-declared");
    let named: Vec<String> = declared
        .parameters
        .iter()
        .filter_map(|parameter| match parameter {
            kynos::openapi::RefOr::Item(item) => Some(item.name.clone()),
            kynos::openapi::RefOr::Ref(_) => None,
        })
        .collect();

    assert_eq!(named.len(), 1, "{named:?}");
}

/// A group spelling the field the way a Rust identifier forces.
#[derive(kynos::HeaderParams)]
struct LowercaseNegotiation {
    /// The natural languages preferred in the response.
    #[header(rename = "accept-language")]
    accept_language: Option<String>,
}

#[kynos::get("/doubly-declared")]
async fn doubly_declared(
    _preferred: kynos::response::language::AcceptLanguage<Supported>,
    _declared: kynos::extract::params::header::Headers<LowercaseNegotiation>,
) -> kynos::extract::body::text::Text {
    kynos::extract::body::text::Text("Hello".to_owned())
}

// --- Scope in a status, for a field the handler never writes ----------------
//
// The same question from the other side. A response header contributed by a
// header *group* or by an *interceptor* has to reach the status a consumer
// resolves an observed response to. OpenAPI resolves an exact key before a
// wildcard, so a header filed under `2XX` beside a declared `200` is a header
// no reader of that 200 ever sees — and the `2XX` entry is then a response no
// service can produce.
//
// `tests/matrix.rs` is where that was found: its
// `assert_declared_responses_covered` reported nine unreachable `2XX` keys, one
// per operation, on the first run over the whole owned-layer matrix. It remains
// the guard, because only a live exchange can notice a declared response that
// never happened; these state the rule where a failure names it, over a fixture
// small enough to read.

/// A declared response header group, so a `WithHeaders` return has something
/// the description promises.
#[derive(kynos::HeaderParams)]
struct Paging {
    /// How many records the collection holds.
    #[header(rename = "X-Total-Count")]
    total: u64,
}

/// One operation answering 200, with a group on its response.
#[kynos::get("/listing")]
async fn listing() -> kynos::response::headers::WithHeaders<kynos::extract::body::text::Text, Paging>
{
    kynos::response::headers::WithHeaders::new(
        kynos::extract::body::text::Text("[]".to_owned()),
        Paging { total: 0 },
    )
}

/// A response header a group declares is one the conformance check requires, so
/// this pins that the group is described rather than merely sent.
#[test]
fn a_declared_response_header_reaches_the_description() {
    let document = Router::<()>::new()
        .mount(kynos::routes![listing, alpha])
        .openapi()
        .expect("a describable router");

    assert_eq!(
        declared_headers(&operation(&document, "/listing"), "200"),
        Some(vec!["X-Total-Count".to_owned()]),
        "a header group with `DESCRIBED = true` was sent and not declared"
    );

    // The control: an operation carrying no group declares the name on none of
    // its statuses, so the case above is about the group rather than about a
    // `describe` pass that names `X-Total-Count` everywhere.
    let control = operation(&document, "/alpha");
    for status in control.responses.responses.keys() {
        assert_eq!(
            declared_headers(&control, status),
            Some(Vec::new()),
            "{status}"
        );
    }
}

/// An interceptor's response header is declared where a consumer will look for
/// it.
///
/// `ErasedInterceptor::describe` files the header under `StatusPattern::Success`
/// — the `2XX` pattern. Spreading it over the statuses already declared is what
/// makes it reachable: a consumer resolving an observed 200 takes the exact key
/// first, per the precedence the specification gives, so a header left on a
/// `2XX` entry is one it never sees.
#[test]
fn an_interceptors_response_header_is_declared_where_a_consumer_resolves_it() {
    let document = Router::<()>::new()
        .intercept(kynos::middleware::request_id::RequestId::new())
        .mount(kynos::routes![listing])
        .openapi()
        .expect("a describable router");

    let listing = operation(&document, "/listing");
    let declared = declared_headers(&listing, "200").expect("a described 200");

    assert!(
        declared.contains(&"X-Request-Id".to_owned()),
        "the 200 a consumer resolves declares {declared:?}, and the header an \
         interceptor sets is filed under a key nothing resolves to"
    );

    // And the wildcard it was filed under is not left behind as a response
    // nothing can produce.
    assert!(
        !listing.responses.responses.contains_key("2XX"),
        "{:?}",
        listing.responses.responses.keys().collect::<Vec<_>>()
    );
}

/// Setting a cookie means declaring it, on the status that carries it.
#[cfg(feature = "cookie")]
#[test]
fn a_cookie_an_interceptor_sets_is_declared_where_a_consumer_resolves_it() {
    let document = Router::<()>::new()
        .mount(kynos::routes![listing])
        .intercept(kynos::middleware::cookies::SetCookies::new(vec![
            kynos::response::cookie::Cookie::new("locale", "en"),
        ]))
        .openapi()
        .expect("a describable router");

    let declared =
        declared_headers(&operation(&document, "/listing"), "200").expect("a described 200");

    assert!(declared.contains(&"Set-Cookie".to_owned()), "{declared:?}");
}

// --- Scope in the document's tags -------------------------------------------
//
// The same question again, on the axis the sections above do not reach. A tag
// is applied at four scopes -- `Router::tag`, `Group::tag`,
// `EndpointBuilder::tag`, and `tag = T` on the route attribute -- and
// `docs/routing.md` promises they "add rather than override". Each of them owes
// the document two things: the tag's *name* on every operation it covers, and
// the tag's *metadata* in the document's own `tags`. A name that arrives
// without its metadata is an operation filed under a heading no consumer can
// render, which the validator raises as `UndocumentedTag`; metadata that
// arrives without the name is a heading with nothing under it.
//
// Both halves are asserted for every scope, because a fix for one that forgets
// the other is a worse document than the silence it replaced.

/// A tag with metadata worth losing, so an assertion can tell a registered tag
/// from a bare name.
#[derive(kynos::Tag)]
#[tag(name = "users", description = "Managing user accounts")]
struct Users;

#[derive(kynos::Tag)]
#[tag(name = "ops", description = "Health and readiness")]
struct Ops;

#[derive(kynos::Tag)]
#[tag(name = "admin", description = "Restricted to staff")]
struct Admin;

/// Tagged on the attribute, which is the one scope no builder call supplies.
#[kynos::get("/tagged", tag = Users)]
async fn tagged() -> NoContent {
    NoContent
}

/// Tagged on the attribute, for the operation that sits under three scopes.
#[kynos::get("/suspension", tag = Admin)]
async fn suspension() -> NoContent {
    NoContent
}

/// A handler with no attribute, so only a builder can describe it.
async fn version() -> NoContent {
    NoContent
}

/// The `tags` of the one operation under `path`.
fn tags_on(document: &Document, path: &str) -> Vec<String> {
    operation(document, path).tags
}

/// The document's own metadata for `name`, if it declares any.
fn documented(document: &Document, name: &str) -> Option<kynos::openapi::Tag> {
    document.tags.iter().find(|tag| tag.name == name).cloned()
}

/// Every tag name an operation carries that the document never documents.
fn undocumented_tags(router: &Router<()>) -> Vec<String> {
    router
        .validate()
        .expect("a describable router")
        .into_iter()
        .filter_map(|violation| match violation.error {
            RouterError::Spec(kynos::openapi::SpecError::UndocumentedTag { name }) => Some(name),
            _ => None,
        })
        .collect()
}

/// The innermost scope reaches the operation it is written on.
///
/// The attribute is the only scope that is a fact about the operation rather
/// than about what encloses it, so nothing in the router can supply it and
/// nothing else in the suite would notice it going missing.
#[test]
fn a_route_attribute_tag_reaches_the_operation() {
    let document = Router::<()>::new()
        .mount(kynos::routes![tagged])
        .openapi()
        .expect("a describable router");

    assert_eq!(tags_on(&document, "/tagged"), ["users"]);
}

/// And it registers the metadata that makes the name mean something.
///
/// The name alone would group the operation under a heading the document never
/// describes. `#[derive(Tag)]` is what carries the description, so the scope
/// that names the type is the scope that owes the document its metadata.
#[test]
fn a_route_attribute_tag_documents_itself_in_the_document_tags() {
    let document = Router::<()>::new()
        .mount(kynos::routes![tagged])
        .openapi()
        .expect("a describable router");

    let users = documented(&document, "users").expect("`users` in the document `tags`");
    assert_eq!(users.description.as_deref(), Some("Managing user accounts"));
    assert_eq!(document.tags.len(), 1, "{:?}", document.tags);
}

/// The control against the half-fix.
///
/// Wiring the attribute's tag onto the operation without registering its
/// metadata turns a silent drop into a warning on every operation carrying one
/// — a strictly worse document than the one that dropped the tag. This is the
/// only assertion in the workspace that would notice, and it is green both
/// before the fix (nothing is tagged) and after (everything tagged is
/// documented), which is what makes it a control rather than a red test.
#[test]
fn a_route_attribute_tag_raises_no_undocumented_tag_violation() {
    let router = Router::<()>::new().mount(kynos::routes![tagged]);

    let undocumented = undocumented_tags(&router);
    assert!(undocumented.is_empty(), "{undocumented:?}");
}

/// The endpoint scope owes the same two things, and today pays only one.
///
/// `EndpointBuilder::tag` pushes a name and nothing else, so a router that
/// never repeats the tag itself emits an operation grouped under a heading the
/// document does not declare.
#[test]
fn an_endpoint_builder_tag_documents_itself_in_the_document_tags() {
    let endpoint = EndpointBuilder::new(
        kynos::openapi::Method::Get,
        kynos::openapi::PathTemplate::parse("/version").expect("valid path"),
        version,
    )
    .tag::<Ops>();

    let router = Router::<()>::new().mount(endpoint);
    let document = router.openapi().expect("a describable router");

    assert_eq!(tags_on(&document, "/version"), ["ops"]);
    let ops = documented(&document, "ops").expect("`ops` in the document `tags`");
    assert_eq!(ops.description.as_deref(), Some("Health and readiness"));

    let undocumented = undocumented_tags(&router);
    assert!(undocumented.is_empty(), "{undocumented:?}");
}

/// Three scopes over one operation: its own tag, then each enclosing scope
/// outermost first.
///
/// The order is the contract a generator reads, since most consumers bucket an
/// operation by `tags[0]`. An operation that tags itself therefore wins its own
/// primary bucket over a blanket `Router::tag` — which is the whole of the
/// guarantee, and is why the sibling case below exists to say what happens when
/// it does not.
#[test]
fn an_operation_lists_its_own_tag_first_then_each_enclosing_scope_outermost_first() {
    let document = Router::<()>::new()
        .tag::<Ops>()
        .group(
            Group::<()>::new("/admin")
                .tag::<Users>()
                .mount(kynos::routes![suspension]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(
        tags_on(&document, "/admin/suspension"),
        ["admin", "ops", "users"]
    );

    // Each scope's metadata, the router's and the group's as well as the
    // operation's own, and nothing beside them.
    for (name, description) in [
        ("admin", "Restricted to staff"),
        ("ops", "Health and readiness"),
        ("users", "Managing user accounts"),
    ] {
        let tag = documented(&document, name).unwrap_or_else(|| panic!("`{name}` documented"));
        assert_eq!(tag.description.as_deref(), Some(description), "{name}");
    }
    assert_eq!(document.tags.len(), 3, "{:?}", document.tags);
}

/// An operation with no tag of its own is filed under its *outermost*
/// enclosing scope.
///
/// The other half of the rule, and the half a reader is most likely to guess
/// wrong: `describe` walks `self.tags` before `mounted.tags`, so the router's
/// blanket tag precedes the group's more specific one. A consumer bucketing by
/// `tags[0]` files this operation under `ops` rather than under `users`.
///
/// Pinned rather than merely written down, because "the most specific claim
/// wins" is the intuitive rule and is not this one. Changing the order would
/// move the primary bucket of every operation that does not tag itself, which
/// is a document change no other assertion here would see.
#[test]
fn an_operation_with_no_tag_of_its_own_lists_its_enclosing_scopes_outermost_first() {
    let document = Router::<()>::new()
        .tag::<Ops>()
        .group(
            Group::<()>::new("/v2")
                .tag::<Users>()
                .mount(kynos::routes![alpha]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(tags_on(&document, "/v2/alpha"), ["ops", "users"]);
}

/// One name declared at two scopes is one entry, in both places it appears.
///
/// `tags` is a set spelled as an array, and the document's `tags` keeps one
/// copy of a description however many scopes repeat it. Green before and
/// after: the fix must not turn a tag repeated at an enclosing scope into a
/// duplicate.
#[test]
fn a_tag_named_at_two_scopes_appears_once() {
    let document = Router::<()>::new()
        .group(
            Group::<()>::new("/v1")
                .tag::<Users>()
                .mount(kynos::routes![tagged]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(tags_on(&document, "/v1/tagged"), ["users"]);
    assert_eq!(
        document
            .tags
            .iter()
            .filter(|tag| tag.name == "users")
            .count(),
        1,
        "{:?}",
        document.tags
    );
}

/// A tag named at two *enclosing* scopes survives in the outermost one's slot.
///
/// `a_tag_named_at_two_scopes_appears_once` above pairs the attribute with a
/// group, which is the one pair where "innermost" and "first scope to name it"
/// agree — so it cannot tell the rule from its opposite, and a stale claim that
/// the innermost position wins survived beside it. This pair can tell them
/// apart: the router names `Users` before the group does, and `add_tag` keeps
/// the first insertion, so `users` sits in the router's slot.
///
/// `Ops` between the two slots is what makes the difference observable. Without
/// it both readings emit `["users"]`; with it, keeping the group's instance
/// instead would emit `["ops", "users"]` and move the primary bucket.
#[test]
fn a_tag_named_at_two_enclosing_scopes_survives_at_the_outermost_of_them() {
    let document = Router::<()>::new()
        .tag::<Users>()
        .tag::<Ops>()
        .group(
            Group::<()>::new("/v3")
                .tag::<Users>()
                .mount(kynos::routes![alpha]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(tags_on(&document, "/v3/alpha"), ["users", "ops"]);
}

/// A second type claiming `users` with other metadata, which the document
/// cannot hold beside [`Users`]: operations name a tag by its name alone.
#[derive(kynos::Tag)]
#[tag(name = "users", description = "Account holders, as billing sees them")]
struct BillingUsers;

/// A second type claiming `users` with metadata identical to [`Users`]'s.
#[derive(kynos::Tag)]
#[tag(name = "users", description = "Managing user accounts")]
struct SameUsers;

/// Two types describing one name differently is a router `build` refuses.
///
/// Keeping either description would publish the other type's operations under
/// a heading their own type never wrote.
#[test]
fn two_tag_types_describing_one_name_differently_are_refused_by_build() {
    let router = Router::<()>::new().tag::<Users>().group(
        Group::<()>::new("/billing")
            .tag::<BillingUsers>()
            .mount(kynos::routes![alpha]),
    );

    let Err(kynos::Error::Invalid { violations }) = router.build(()) else {
        panic!("two descriptions of `users` must be refused");
    };
    assert!(
        violations.iter().any(|violation| matches!(
            &violation.error,
            RouterError::Spec(kynos::openapi::SpecError::DuplicateTag { name }) if name == "users"
        )),
        "{violations:?}"
    );
}

/// `validate` reports the same refusal `build` makes, so a test that checks a
/// router without serving it sees it too.
#[test]
fn two_tag_types_describing_one_name_differently_are_a_validation_error() {
    let violations = Router::<()>::new()
        .tag::<Users>()
        .tag::<BillingUsers>()
        .mount(kynos::routes![alpha])
        .validate()
        .expect("a describable router");

    assert!(
        violations.iter().any(|violation| {
            violation.severity == kynos::openapi::Severity::Error
                && matches!(
                    &violation.error,
                    RouterError::Spec(kynos::openapi::SpecError::DuplicateTag { name })
                        if name == "users"
                )
        }),
        "{violations:?}"
    );
}

/// Two types whose metadata is identical describe one tag, and the document
/// says so once: nothing is lost, so nothing is refused.
#[test]
fn two_tag_types_describing_one_name_identically_appear_once() {
    let document = Router::<()>::new()
        .tag::<Users>()
        .tag::<SameUsers>()
        .mount(kynos::routes![alpha])
        .openapi()
        .expect("a describable router");

    assert_eq!(tags_on(&document, "/alpha"), ["users"]);
    assert_eq!(document.tags.len(), 1, "{:?}", document.tags);
}

/// The control for all of the above: naming no tag declares none.
///
/// Without it every assertion here would pass against an implementation that
/// tagged every operation with everything it had ever seen.
#[test]
fn no_tag_is_declared_when_none_is_named() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .openapi()
        .expect("a describable router");

    assert!(tags_on(&document, "/alpha").is_empty());
    assert!(document.tags.is_empty(), "{:?}", document.tags);
}

// --- Scope in a status two contributors reach -------------------------------
//
// `Handler::describe` contributes each argument's rejection before the return
// type's responses, and a status keys exactly one response. Where an extractor
// and the handler's error type both name one, the entry filed there is what
// every client is told about a status that carries *both* — so it publishes the
// types both halves can put on the wire, rather than whichever half was
// contributed first.

/// What `/reviews/{id}` captures. The capture that may fail to deserialize is
/// what makes `PathRejection`'s 400 reach the operation.
#[derive(Schema, PathParams)]
struct ReviewPath {
    #[allow(dead_code)]
    id: u64,
}

/// A failure answering with the status the capture's rejection already claims.
#[derive(Debug, thiserror::Error, ApiError)]
enum ReviewError {
    #[error("the review says nothing")]
    #[problem(
        status = 400,
        type = "https://errors.example.com/empty-review",
        title = "Empty review"
    )]
    Empty,
}

/// One operation, two contributors to one status.
#[kynos::get("/reviews/{id}")]
async fn review(Path(path): Path<ReviewPath>) -> Result<NoContent, ReviewError> {
    let _ = path;
    Err(ReviewError::Empty)
}

/// The problem schema declared on `status`, as JSON.
fn declared_problem(operation: &kynos::openapi::Operation, status: &str) -> serde_json::Value {
    let kynos::openapi::RefOr::Item(response) = operation
        .responses
        .responses
        .get(status)
        .unwrap_or_else(|| panic!("{status} is described"))
    else {
        panic!("{status} is described as a `$ref`");
    };

    serde_json::to_value(response).expect("a response serializes")
}

/// Every type URI a branch of `schema` constrains a problem's `type` to.
fn published(schema: &serde_json::Value) -> Vec<String> {
    let branches = schema["oneOf"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![schema.clone()]);

    branches
        .iter()
        .filter_map(|branch| branch["allOf"][1]["properties"]["type"]["const"].as_str())
        .map(ToOwned::to_owned)
        .collect()
}

/// A status an extractor and a handler error type share publishes both.
///
/// The 400 a `Path<T>` capture produces carries `about:blank`; the 400 this
/// handler returns carries the type its declaration named. Both are observable
/// on this operation, so both are declared — publishing one of them makes the
/// document wrong about the other, whichever way round it is dropped.
#[test]
fn a_status_an_extractor_and_an_error_type_share_publishes_both_types() {
    let document = Router::<()>::new()
        .mount(kynos::routes![review])
        .openapi()
        .expect("a describable router");
    let schema = declared_problem(&operation(&document, "/reviews/{id}"), "400")
        ["content"]["application/problem+json"]["schema"]
        .clone();

    assert_eq!(
        published(&schema),
        vec![
            "about:blank".to_owned(),
            "https://errors.example.com/empty-review".to_owned(),
        ],
        "{schema}"
    );
}

/// And says what each of them means, rather than what the first one meant.
///
/// The description is prose and under no exactly-one rule, so it joins the two
/// the way a shared status already joins the summaries of the variants
/// answering with it.
#[test]
fn a_shared_status_describes_both_halves_of_what_it_publishes() {
    let document = Router::<()>::new()
        .mount(kynos::routes![review])
        .openapi()
        .expect("a describable router");
    let response = declared_problem(&operation(&document, "/reviews/{id}"), "400");

    assert_eq!(
        response["description"],
        serde_json::json!("Bad Request; Empty review"),
        "{response}"
    );
}

/// The field a turned-away client waits on.
#[derive(Debug, kynos::HeaderParams)]
struct RetryAfter {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

/// Why admission turned a request away: a 503 sending `Retry-After`, and a
/// 429 that does not.
#[derive(Debug, thiserror::Error, ApiError)]
#[allow(dead_code)]
enum AdmissionRefused {
    #[error("the service is draining")]
    #[problem(status = 503)]
    Draining(#[problem(headers)] RetryAfter),

    #[error("too many requests")]
    #[problem(status = 429)]
    Throttled,
}

/// An extractor whose rejection is contributed before the handler's error.
struct Admitted;

impl kynos::extract::describe::Describe for Admitted {
    fn describe(operation: &mut kynos::router::operation::OperationCx<'_>) {
        let _ = operation;
    }
}

impl<C: Sync> kynos::extract::FromRequestParts<C> for Admitted {
    type Rejection = AdmissionRefused;

    async fn from_request_parts(
        parts: &mut kynos::http::Parts,
        context: &C,
    ) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        Ok(Self)
    }
}

/// The handler's failures, the other way round: a 429 sending `Retry-After`,
/// and a 503 that does not.
#[derive(Debug, thiserror::Error, ApiError)]
#[allow(dead_code)]
enum UploadError {
    #[error("the upload quota is spent")]
    #[problem(status = 429)]
    Quota(#[problem(headers)] RetryAfter),

    #[error("storage is offline")]
    #[problem(status = 503)]
    Offline,
}

/// One operation, two contributors to each of two statuses.
#[kynos::get("/uploads")]
async fn upload(admitted: Admitted) -> Result<NoContent, UploadError> {
    let _ = admitted;
    Err(UploadError::Offline)
}

/// A status two contributors reach declares the headers either sends there,
/// required only where both send them.
///
/// The 429 the handler sends carries `Retry-After` although the rejection
/// contributed first does not, so the field is declared; the 503 the rejection
/// sends carries it and the handler's does not, so it is not required.
#[test]
fn a_shared_status_declares_the_headers_either_contributor_sends() {
    let document = Router::<()>::new()
        .mount(kynos::routes![upload])
        .openapi()
        .expect("a describable router");
    let operation = operation(&document, "/uploads");

    for status in ["429", "503"] {
        let response = declared_problem(&operation, status);
        let field = &response["headers"]["Retry-After"];
        assert!(field.is_object(), "{status}: {response}");
        assert_eq!(field.get("required"), None, "{status}: {response}");
    }
}

// --- Scope in what a 403 publishes ------------------------------------------
//
// `AuthRejection::forbidden_as` puts an application's own problem type on a 403
// at run time. `Scopes::FORBIDDEN_TYPE` is the half a document assembled from
// types can read: a scope set names the URI its refusal may carry, and the
// operation's declared 403 narrows to a choice between that URI and
// `about:blank` — never to the URI alone, because `AuthRejection::forbidden()`
// stays available to every authorizer.
//
// A scope set naming none, and an argument naming no scope set at all, declare
// the shared component. That is the widest thing declarable rather than a gap:
// an authorizer under either may still put any URI on the wire.

/// What a verified token yields.
struct Claims;

/// Refuses everything, because nothing here runs it: the assertions below read
/// the *document*, and an authenticator is unreachable while one is built —
/// which is the whole reason `FORBIDDEN_TYPE` is a const on the scope set.
struct Tokens;

impl<C: Sync, P: kynos::security::Scope>
    kynos::security::Authenticator<kynos::security::schemes::Bearer<Claims, P>, C> for Tokens
{
    async fn authenticate(
        &self,
        presented: kynos::security::carrier::BearerToken,
        context: &C,
    ) -> Result<Claims, kynos::error::rejection::AuthRejection> {
        let _ = (presented, context);
        Err(kynos::error::rejection::AuthRejection::unauthenticated())
    }

    async fn authorize(
        &self,
        credential: &Claims,
        scopes: &[P],
        context: &C,
    ) -> Result<(), kynos::error::rejection::AuthRejection> {
        let _ = (credential, scopes, context);
        Err(kynos::error::rejection::AuthRejection::forbidden_as(
            INSUFFICIENT_SCOPE,
        ))
    }
}

/// The application context the guarded operations below describe against.
struct Guarded;

impl<P: kynos::security::Scope>
    kynos::security::Authenticates<kynos::security::schemes::Bearer<Claims, P>> for Guarded
{
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &Tokens
    }
}

/// The refusal `Staff` names, and the one `Tokens` publishes.
const INSUFFICIENT_SCOPE: &str = "https://errors.example.com/insufficient-scope";

/// The scopes the operations below demand.
#[derive(Clone, Copy)]
enum Grant {
    Admin,
    ReadReports,
}

impl kynos::security::Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::ReadReports => "reports:read",
        }
    }
}

/// A bearer token whose operations may demand a `Grant`.
type Granted = kynos::security::schemes::Bearer<Claims, Grant>;

/// A scope set that names the refusal it can produce.
struct Staff;

impl kynos::security::auth::Scopes for Staff {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::Admin];
    const FORBIDDEN_TYPE: Option<&'static str> = Some(INSUFFICIENT_SCOPE);
}

/// A scope set that names none, which is every one written before the const
/// existed.
struct ReadReports;

impl kynos::security::auth::Scopes for ReadReports {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::ReadReports];
}

#[kynos::get("/admin/audit")]
async fn audit(caller: kynos::security::auth::Scoped<Granted, Staff>) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

#[kynos::get("/reports")]
async fn reports(caller: kynos::security::auth::Scoped<Granted, ReadReports>) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

#[kynos::get("/me")]
async fn me(
    caller: kynos::security::auth::Auth<kynos::security::schemes::Bearer<Claims>>,
) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

/// A failure of the handler's own, answering with the status the guard already
/// declares.
#[derive(Debug, thiserror::Error, ApiError)]
enum AuditError {
    #[error("that record is sealed")]
    #[problem(
        status = 403,
        type = "https://errors.example.com/record-sealed",
        title = "Record sealed"
    )]
    Sealed,
}

/// A guard beside the extractors it precedes.
#[kynos::get("/reviews/{id}/audit")]
async fn audited_review(
    caller: kynos::security::auth::Auth<kynos::security::schemes::Bearer<Claims>>,
    Path(path): Path<ReviewPath>,
) -> NoContent {
    let _ = (caller.into_inner(), path);
    NoContent
}

#[kynos::get("/admin/sealed")]
async fn sealed(
    caller: kynos::security::auth::Scoped<Granted, Staff>,
) -> Result<NoContent, AuditError> {
    let _ = caller.into_inner();
    Err(AuditError::Sealed)
}

/// The schema declared for `status` on `path`, as JSON.
fn declared_problem_schema(document: &Document, path: &str, status: &str) -> serde_json::Value {
    declared_problem(&operation(document, path), status)["content"]
        ["application/problem+json"]["schema"]
        .clone()
}

/// The document the guarded operations emit.
fn guarded_document() -> Document {
    Router::<Guarded>::new()
        .mount(kynos::routes![audit, reports, me, sealed, audited_review])
        .openapi()
        .expect("a describable router")
}

/// The guard's slot is described like any argument's: its whole `security`
/// list, once, beside what the extractors after it declare.
///
/// The guarded handler implementations are separate from the unguarded ones,
/// so one that forgot to describe its guard would serve a credential check the
/// document never mentions.
#[test]
fn a_guard_before_other_arguments_declares_its_security_once() {
    let operation = operation(&guarded_document(), "/reviews/{id}/audit");

    let demanded: Vec<Vec<String>> = operation
        .security
        .as_deref()
        .expect("the guard's requirement")
        .iter()
        .map(|requirement| requirement.0.keys().cloned().collect())
        .collect();
    assert_eq!(demanded, vec![vec!["Bearer".to_owned()]]);

    assert_eq!(operation.parameters.len(), 1, "{:?}", operation.parameters);
    for status in ["400", "401", "403"] {
        assert!(
            operation.responses.responses.contains_key(status),
            "{status} is missing"
        );
    }
}

/// A scope set naming a type declares that type *and* `about:blank`.
///
/// Both, and this is the whole soundness argument: an authorizer refusing a
/// `Scoped<S, Staff>` may answer `AuthRejection::forbidden_as(INSUFFICIENT_SCOPE)`
/// or the plain `AuthRejection::forbidden()`, and both bodies reach a client of
/// this operation. Narrowing to the named URI alone would declare less than the
/// operation sends, which is the one direction `emitted ⊇ observable` forbids —
/// and it is a failure no document validator would report, because a document
/// that under-declares still validates.
#[test]
fn a_scope_set_naming_a_forbidden_type_declares_it_beside_about_blank() {
    let schema = declared_problem_schema(&guarded_document(), "/admin/audit", "403");

    assert_eq!(
        published(&schema),
        vec!["about:blank".to_owned(), INSUFFICIENT_SCOPE.to_owned(),],
        "{schema}"
    );
}

/// A scope set naming none declares the shared component, as it always has.
///
/// Not `about:blank`, which would be the intuitive reading and is unsound: an
/// authorizer under a scope set that named no type may still reach
/// `forbidden_as` with a URI of its own, and only a schema every problem
/// document satisfies is true of that. The const is opt-in precisely because
/// naming one is the promise that makes the narrowing honest.
#[test]
fn a_scope_set_naming_no_forbidden_type_declares_what_it_always_did() {
    let schema = declared_problem_schema(&guarded_document(), "/reports", "403");

    assert_eq!(
        schema["$ref"],
        serde_json::json!("#/components/schemas/Problem"),
        "{schema}"
    );
    assert!(published(&schema).is_empty(), "{schema}");
}

/// `Auth<S>` names no scope set, so its 403 has nothing to narrow to.
///
/// The seam is on `Scopes` rather than on `SecurityScheme` deliberately: a URI
/// hung on the scheme would pin one name to every check made under it. `Auth`
/// is the argument that pays for that choice, and this pins what it pays.
#[test]
fn an_unscoped_guard_declares_the_shared_component_for_its_403() {
    let schema = declared_problem_schema(&guarded_document(), "/me", "403");

    assert_eq!(
        schema["$ref"],
        serde_json::json!("#/components/schemas/Problem"),
        "{schema}"
    );
}

/// The 401 beside it narrows, and is not touched by any of this.
///
/// `AuthRejection::unauthenticated` has no URI field and is not getting one, so
/// `about:blank` is a fact about every 401 a guard sends whatever its scope set
/// declares.
#[test]
fn a_named_forbidden_type_leaves_the_401_narrowed_to_about_blank() {
    let document = guarded_document();

    for path in ["/admin/audit", "/reports", "/me"] {
        let schema = declared_problem_schema(&document, path, "401");
        assert_eq!(published(&schema), vec!["about:blank".to_owned()], "{path}");
    }
}

/// The narrowed 403 unions with a handler error's 403 rather than losing to it.
///
/// This is what the union buys and the bare `$ref` cost. A guard declaring the
/// shared component *wins* the whole entry — it admits every problem document,
/// so the handler's `const` is discarded — while two narrowed sides flatten
/// into a choice of three, each a `const` no other branch matches. That last
/// clause is `oneOf`'s exactly-one rule, and it is why a `$ref` with no const
/// can never be a branch.
#[test]
fn a_named_forbidden_type_meets_a_handler_errors_403_as_a_choice_of_three() {
    let schema = declared_problem_schema(&guarded_document(), "/admin/sealed", "403");

    assert_eq!(
        published(&schema),
        vec![
            "about:blank".to_owned(),
            INSUFFICIENT_SCOPE.to_owned(),
            "https://errors.example.com/record-sealed".to_owned(),
        ],
        "{schema}"
    );

    // Three branches under `oneOf` rather than one flattened away, so a body
    // carrying any of the three matches exactly one of them.
    assert_eq!(
        schema["oneOf"].as_array().map(Vec::len),
        Some(3),
        "{schema}"
    );
}

// --- What `deny_unchecked_schemas` reaches ----------------------------------
//
// The setting promotes a warning the validator raises, so it refuses exactly
// what the validator sees. A derived body is registered as a component and
// named by `$ref`, which puts its `Unchecked` field under
// `components/schemas/<Name>/properties` rather than under the media type.

/// A supplier's feed: one field this service constrains, one it does not.
#[derive(Schema, serde::Deserialize)]
struct Feed {
    #[allow(dead_code)]
    supplier: String,
    #[allow(dead_code)]
    payload: kynos::schema::unchecked::Unchecked<serde_json::Value>,
}

#[kynos::post("/feeds")]
async fn ingest_feed(
    kynos::extract::body::json::Json(feed): kynos::extract::body::json::Json<Feed>,
) -> NoContent {
    let _ = feed;
    NoContent
}

/// The field's own warning is what the setting turns into a build error.
///
/// Without the setting the same router builds, so the refusal is the
/// setting's rather than something else wrong with the document.
#[test]
fn deny_unchecked_schemas_refuses_an_unchecked_field_of_a_registered_body() {
    use kynos::openapi::{Severity, SpecError};

    let router = || Router::<()>::new().mount(kynos::routes![ingest_feed]);

    let reported = router().validate().expect("a describable router");
    assert!(
        reported.iter().any(|violation| violation.error
            == RouterError::Spec(SpecError::UncheckedSchema)
            && violation.severity == Severity::Warning),
        "the field warns without the setting: {reported:?}"
    );
    router()
        .openapi()
        .expect("a warning alone does not refuse the router");

    let Err(kynos::Error::Invalid { violations }) = router().deny_unchecked_schemas().openapi()
    else {
        panic!("deny_unchecked_schemas built a router with an `Unchecked` field");
    };
    let unchecked: Vec<(&str, Severity)> = violations
        .iter()
        .filter(|violation| violation.error == RouterError::Spec(SpecError::UncheckedSchema))
        .map(|violation| (violation.location.as_str(), violation.severity))
        .collect();
    assert_eq!(
        unchecked,
        [(
            "#/components/schemas/Feed/properties/payload",
            Severity::Error
        )]
    );
}

// --- Which version the description claims ---------------------------------

/// The `openapi` field an assembled router's description carries.
fn claimed_version<C: 'static>(router: &Router<C>) -> String {
    router.openapi().expect("a describable router").openapi
}

/// `Router::openapi` picks the lowest version that expresses the API, and
/// never the cargo feature: under `openapi32` this router still says 3.1.
#[test]
fn a_router_using_no_3_2_construct_is_described_as_3_1() {
    let router = Router::<()>::new().mount(kynos::routes![alpha, beta]);

    assert_eq!(claimed_version(&router), "3.1.2");
}

/// A cookie group stating no style, so nothing in it is 3.2-only.
#[cfg(feature = "cookie")]
#[derive(kynos::CookieParams)]
struct Unstyled {
    #[allow(dead_code)]
    theme: String,
}

#[cfg(feature = "cookie")]
#[kynos::get("/preference")]
async fn preference(_: kynos::extract::params::cookie::Cookies<Unstyled>) -> NoContent {
    NoContent
}

/// A derived cookie is no 3.2 construct of its own accord: the `openapi32`
/// feature, wherever in the graph it is enabled, leaves this router at 3.1.
#[cfg(feature = "cookie")]
#[test]
fn a_derived_cookie_leaves_the_description_at_3_1() {
    let router = Router::<()>::new().mount(kynos::routes![preference]);

    assert_eq!(claimed_version(&router), "3.1.2");
}

/// A cookie group asking for the `cookie` style, which 3.1 does not have.
#[cfg(all(feature = "cookie", feature = "openapi32"))]
#[derive(kynos::CookieParams)]
struct Styled {
    #[allow(dead_code)]
    #[cookie(style = "cookie")]
    greeting: String,
}

#[cfg(all(feature = "cookie", feature = "openapi32"))]
#[kynos::get("/styled")]
async fn styled(_: kynos::extract::params::cookie::Cookies<Styled>) -> NoContent {
    NoContent
}

/// Asking for `style: cookie` is using a 3.2 construct, and raises the
/// description as one does.
#[cfg(all(feature = "cookie", feature = "openapi32"))]
#[test]
fn a_cookie_asking_for_its_style_raises_the_description_to_3_2() {
    let router = Router::<()>::new().mount(kynos::routes![preference, styled]);

    assert_eq!(claimed_version(&router), "3.2.0");
}

/// The one violation a router refused to describe reports.
fn sole_refusal<C: 'static>(router: &Router<C>) -> RouterError {
    let Err(kynos::Error::Invalid { violations }) = router.openapi() else {
        panic!("a router carrying an unrecognised field was described");
    };
    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert_eq!(violation.location, "#");
    violation.error.clone()
}

/// A router's own metadata holding `key` beside its extensions.
fn info_with(key: &str) -> kynos::openapi::Info {
    let mut info = kynos::openapi::Info::new("API", "1.0.0");
    info.extensions.insert(key, true);
    info
}

/// A key without the `x-` prefix is no field either version expresses, so
/// describing the router refuses it in every build rather than claiming
/// whichever version would carry it.
#[test]
fn an_unrecognised_field_refuses_the_description() {
    let router = Router::<()>::new()
        .info(info_with("foo"))
        .mount(kynos::routes![alpha]);

    assert_eq!(
        sole_refusal(&router),
        RouterError::Spec(kynos::openapi::SpecError::RequiresV3_2 {
            blockers: vec!["#/info/foo".to_owned()],
        })
    );
}

/// A stream that ends at once: describing one is all that is asked of it.
#[cfg(feature = "openapi32")]
struct NoEvents;

#[cfg(feature = "openapi32")]
impl futures_core::Stream for NoEvents {
    type Item = Result<kynos::response::stream::sse::Event<u8>, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(None)
    }
}

/// A `text/event-stream` response, whose `itemSchema` 3.1 has no field for.
#[cfg(feature = "openapi32")]
#[kynos::get("/events")]
async fn events() -> kynos::response::stream::sse::Sse<NoEvents> {
    kynos::response::stream::sse::Sse::new(NoEvents)
}

/// The `QUERY` method, which 3.1's Path Item has no field for.
#[cfg(feature = "openapi32")]
#[kynos::query("/search")]
async fn search() -> NoContent {
    NoContent
}

/// An `in: querystring` parameter, a location 3.1 does not have.
#[cfg(feature = "openapi32")]
#[kynos::get("/filtered")]
async fn filtered(
    _: kynos::extract::params::querystring::QueryString<String, kynos::http::media::Json>,
) -> NoContent {
    NoContent
}

/// Each 3.2-only construct raises the description on its own: the router
/// beside it uses none, and says 3.1.
#[cfg(feature = "openapi32")]
#[test]
fn an_sse_operation_raises_the_description_to_3_2() {
    let router = Router::<()>::new().mount(kynos::routes![alpha, events]);

    assert_eq!(claimed_version(&router), "3.2.0");
}

#[cfg(feature = "openapi32")]
#[test]
fn a_query_operation_raises_the_description_to_3_2() {
    let router = Router::<()>::new().mount(kynos::routes![alpha, search]);

    assert_eq!(claimed_version(&router), "3.2.0");
}

#[cfg(feature = "openapi32")]
#[test]
fn a_querystring_parameter_raises_the_description_to_3_2() {
    let router = Router::<()>::new().mount(kynos::routes![alpha, filtered]);

    assert_eq!(claimed_version(&router), "3.2.0");
}

/// A 3.2 construct beside an unrecognised field does not carry the description
/// to 3.2, which cannot express the field either: the refusal names the field
/// alone, since the construct is not what stands in the way.
#[cfg(feature = "openapi32")]
#[test]
fn an_unrecognised_field_refuses_a_description_using_3_2() {
    let router = Router::<()>::new()
        .info(info_with("foo"))
        .mount(kynos::routes![alpha, search]);

    assert_eq!(
        sole_refusal(&router),
        RouterError::Spec(kynos::openapi::SpecError::RequiresV3_2 {
            blockers: vec!["#/info/foo".to_owned()],
        })
    );
}

/// `openapi_as` targets and never downgrades: 3.1 asked of an API using
/// 3.2-only constructs is one refusal naming where each is, not a document
/// with the operations missing.
#[cfg(feature = "openapi32")]
#[test]
fn openapi_as_3_1_refuses_a_3_2_construct_and_names_it() {
    use kynos::openapi::{SpecError, SpecVersion};

    let router = Router::<()>::new().mount(kynos::routes![alpha, events, search, filtered]);

    let Err(kynos::Error::Invalid { violations }) = router.openapi_as(SpecVersion::V3_1) else {
        panic!("a 3.2-only construct was emitted as 3.1");
    };
    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert_eq!(
        violation.error,
        RouterError::Spec(SpecError::RequiresV3_2 {
            blockers: vec![
                "#/paths/~1events/get/responses/200/content/text~1event-stream/itemSchema"
                    .to_owned(),
                "#/paths/~1search/query".to_owned(),
                "#/paths/~1filtered/get/parameters/0".to_owned(),
            ],
        })
    );
    assert_eq!(
        violation.to_string(),
        "error at #: cannot emit as OpenAPI 3.1: 3 3.2-only or unrecognised field(s) in use: \
         #/paths/~1events/get/responses/200/content/text~1event-stream/itemSchema, \
         #/paths/~1search/query, #/paths/~1filtered/get/parameters/0"
    );
}

// --- What `server` and a nested router's `info` reach ---------------------

/// A declared server is published as declared, and a nested router's servers
/// follow the outer router's own.
#[test]
fn a_declared_server_reaches_the_documents_servers() {
    use kynos::openapi::Server;

    let document = Router::<()>::new()
        .server(Server::new("https://api.example.com"))
        .mount(kynos::routes![alpha])
        .nest(
            "/v2",
            Router::<()>::new()
                .server(Server::new("https://v2.example.com"))
                .mount(kynos::routes![beta]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(
        document.servers,
        [
            Server::new("https://api.example.com"),
            Server::new("https://v2.example.com"),
        ]
    );
}

/// `info` is the outer router's when it declares one, the nested router's
/// when only that one does, and the placeholder when neither does.
#[test]
fn a_nested_routers_info_is_used_only_when_the_outer_declares_none() {
    use kynos::openapi::Info;

    let inner = || {
        Router::<()>::new()
            .info(Info::new("Inner", "2.0.0"))
            .mount(kynos::routes![beta])
    };
    let info = |router: Router<()>| router.openapi().expect("a describable router").info;

    assert_eq!(
        info(
            Router::<()>::new()
                .info(Info::new("Outer", "1.0.0"))
                .nest("/v2", inner())
        ),
        Info::new("Outer", "1.0.0")
    );
    assert_eq!(
        info(Router::<()>::new().nest("/v2", inner())),
        Info::new("Inner", "2.0.0")
    );
    assert_eq!(
        info(Router::<()>::new().mount(kynos::routes![alpha])),
        Info::new("API", "0.0.0")
    );
}

/// `externalDocs` follows `info`'s rule: the outer router's when it declares
/// one, the first absorbed router's when only that one does, and absent when
/// none does.
#[test]
fn a_nested_routers_external_docs_are_used_only_when_the_outer_declares_none() {
    use kynos::openapi::ExternalDocumentation;

    let docs = |url: &str| ExternalDocumentation::new(url);
    let inner = |url: &str| {
        Router::<()>::new()
            .external_docs(docs(url))
            .mount(kynos::routes![beta])
    };
    let external_docs = |router: Router<()>| {
        router
            .openapi()
            .expect("a describable router")
            .external_docs
    };

    assert_eq!(
        external_docs(
            Router::<()>::new()
                .external_docs(docs("https://outer.example.com"))
                .nest("/v2", inner("https://inner.example.com"))
        ),
        Some(docs("https://outer.example.com"))
    );
    assert_eq!(
        external_docs(
            Router::<()>::new()
                .merge(inner("https://first.example.com"))
                .merge(
                    Router::<()>::new()
                        .external_docs(docs("https://second.example.com"))
                        .mount(kynos::routes![alpha])
                )
        ),
        Some(docs("https://first.example.com"))
    );
    assert_eq!(
        external_docs(Router::<()>::new().mount(kynos::routes![alpha])),
        None
    );
}

/// The four application-wide policies are the root router's alone, so an
/// absorbed router that sets one is refused rather than silently overridden,
/// through `nest` and `merge` alike. Setting one to its default counts too:
/// the root's own value would replace it just the same.
#[test]
fn a_policy_set_on_a_nested_or_merged_router_is_a_violation() {
    use kynos::{
        http::forwarded::{ProxyHeader, TrustedProxies},
        openapi::{Severity, Violation},
        router::policy::{FallbackPolicy, TrailingSlashPolicy},
    };

    type Set = fn(Router<()>) -> Router<()>;
    let policies: [(&str, Set); 4] = [
        ("not_found", |router| {
            router.not_found(FallbackPolicy::Empty)
        }),
        ("method_not_allowed", |router| {
            router.method_not_allowed(FallbackPolicy::Problem)
        }),
        ("trailing_slashes", |router| {
            router.trailing_slashes(TrailingSlashPolicy::Redirect)
        }),
        ("trusted_proxies", |router| {
            router.trusted_proxies(TrustedProxies::hops(ProxyHeader::XForwarded, 1))
        }),
    ];
    let refused = |policy| Violation {
        location: "#".to_owned(),
        severity: Severity::Error,
        error: RouterError::NestedRouterPolicy { policy },
    };
    let violations = |router: Router<()>| router.validate().expect("a describable router");

    for (policy, set) in policies {
        let inner = || set(Router::<()>::new().mount(kynos::routes![beta]));

        assert_eq!(
            violations(Router::<()>::new().nest("/v2", inner())),
            [refused(policy)],
            "`{policy}` through `nest`"
        );
        assert_eq!(
            violations(Router::<()>::new().merge(inner())),
            [refused(policy)],
            "`{policy}` through `merge`"
        );
        assert_eq!(
            violations(set(
                Router::<()>::new().nest("/v2", Router::<()>::new().mount(kynos::routes![beta]))
            )),
            [],
            "`{policy}` on the root router"
        );
    }
}

// --- What an interceptor's `Reads` answers with -----------------------------
//
// A handler's head extractor files its rejection's responses beside its
// parameters. An interceptor's `Reads` is the same decode with the same
// `HeaderRejection`, run before the interceptor's body, so it owes the document
// the same 400 -- and only where the decode can fail at all, since a 400 no
// request can produce is a promise `tests/matrix.rs` refuses.

/// A header every covered request must send.
#[derive(kynos::HeaderParams)]
struct TenantHeader {
    /// Not an `Option`, so its absence is a rejection.
    #[header(rename = "X-Tenant")]
    tenant: String,
}

/// Reads [`TenantHeader`] and answers with nothing of its own.
struct Tenant;

impl kynos::middleware::Interceptor<()> for Tenant {
    type Reads = TenantHeader;
    type Adds = ();
    type Short = std::convert::Infallible;

    async fn intercept(
        &self,
        request: kynos::http::Request,
        reads: TenantHeader,
        context: &(),
        next: kynos::middleware::Next<'_, ()>,
    ) -> Result<kynos::middleware::Continued<()>, std::convert::Infallible> {
        let _ = (context, reads.tenant);
        Ok(next.run(request).await)
    }
}

/// The 400 reaches the operations the interceptor covers, and no other.
#[test]
fn an_interceptor_whose_reads_can_fail_declares_its_rejection() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .group(
            Group::<()>::new("/tenanted")
                .intercept(Tenant)
                .mount(kynos::routes![beta]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "400"), paths(&["/tenanted/beta"]));
}

/// A `Reads` whose decode never fails declares no 400: `()` reads nothing, and
/// `Preconditions` drops a condition it cannot parse rather than refusing it.
#[test]
fn an_interceptor_whose_reads_cannot_fail_declares_no_rejection() {
    let router = Router::<()>::new().intercept(kynos::middleware::request_id::RequestId::new());

    #[cfg(feature = "cache")]
    let router = router.intercept(kynos::middleware::conditional::Conditional::new());

    let document = router
        .mount(kynos::routes![alpha])
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "400"), BTreeSet::new());
}
