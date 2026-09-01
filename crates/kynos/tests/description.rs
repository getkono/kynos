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
            kynos::openapi::SpecError::UndocumentedTag { name } => Some(name),
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

    for name in ["admin", "ops", "users"] {
        assert!(documented(&document, name).is_some(), "{name}");
    }
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
/// `tags` is a set spelled as an array, and the document's `tags` keeps the
/// first claim on a name. Green before and after: the fix must not turn a tag
/// repeated at an enclosing scope into a duplicate.
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
