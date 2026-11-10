//! What each interceptor writes onto a response, against what it declared.
//!
//! One reason: `Adds` and `NAMES` are the whole of an interceptor's promise
//! about response headers, and the compiler checks that two interceptors do not
//! *collide* on a name — never that either sets the names it claimed, and never
//! that it sets nothing else. Both halves are asserted here, over a live
//! service, because both are how a response and its description come apart.
//!
//! This does not assert `describe` directly. `middleware/erased.rs` restates
//! the associated types and asserting it back would compare the mechanism to
//! itself; only the conformance matrix can find that wrong.

#![cfg(all(feature = "macros", feature = "json"))]

use std::collections::BTreeSet;

use kynos::{
    Router,
    extract::{
        body::text::Text,
        params::header::{EncodeHeaders, HeaderParams, Headers, UnencodableHeader},
    },
    http::{HeaderName, HeaderValue, Method, StatusCode},
    middleware::{
        catch_panic::Propagate,
        cors::Cors,
        limits::body_size::BodySize,
        request_id::{CorrelationHeaders, RequestId, RequestIdSource, XRequestId},
    },
    openapi::Method as OperationMethod,
    router::group::{Group, MethodSet, Methods, SafeMethods, UnsafeMethods},
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, post};

/// The response headers a bare service sends, so a later comparison can name
/// what an interceptor *added* rather than what was there anyway.
async fn baseline() -> BTreeSet<String> {
    fields(&get(&support::service(), "/users/1").call().await)
}

fn fields(reply: &support::Reply) -> BTreeSet<String> {
    reply
        .headers
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// Every name in `NAMES`, lowercased the way a `HeaderMap` key is.
fn declared<H: HeaderParams>() -> BTreeSet<String> {
    H::NAMES
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect()
}

/// `RequestId` sets exactly the one name its header group declares.
#[tokio::test]
async fn a_request_id_sets_the_name_its_group_declares_and_no_other() {
    let service = support::router()
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1").call().await;
    assert_eq!(reply.status, StatusCode::OK);

    let added: BTreeSet<String> = fields(&reply)
        .difference(&baseline().await)
        .cloned()
        .collect();

    assert_eq!(added, declared::<XRequestId>());
    assert!(
        reply
            .field("x-request-id")
            .is_some_and(|value| !value.is_empty()),
        "the declared name was set to nothing"
    );
}

/// The pass control: without the interceptor the name is absent, so the case
/// above is about the interceptor rather than about the fixture.
#[tokio::test]
async fn a_service_without_a_request_id_sets_no_such_name() {
    let reply = get(&support::service(), "/users/1").call().await;

    assert!(reply.field("x-request-id").is_none());
}

/// A client-supplied id is honoured only where it was trusted.
///
/// Both directions, because trusting by default would let a client choose its
/// own correlation id and collide with another's on purpose.
#[tokio::test]
async fn a_client_supplied_id_is_used_only_when_it_was_trusted() {
    let trusting = support::router()
        .intercept(RequestId::new().trust_client(true))
        .build(App::new())
        .expect("a describable router");

    let trusted = get(&trusting, "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    assert_eq!(
        trusted.field("x-request-id").as_deref(),
        Some("from-the-client")
    );

    let ignored = get(&support::service(), "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    assert!(ignored.field("x-request-id").is_none());

    let untrusting = support::router()
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let replaced = get(&untrusting, "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    // Replaced, not dropped: the first identifier the default counter mints.
    assert_eq!(
        replaced.field("x-request-id").as_deref(),
        Some("0"),
        "an untrusted client id was echoed back, or dropped rather than replaced"
    );
}

/// Answers with the identifier the handler was given.
#[kynos::get("/correlated")]
async fn correlated(Headers(XRequestId(id)): Headers<XRequestId>) -> Text {
    Text(id.to_str().expect("a printable identifier").to_owned())
}

/// The identifier is set on the request as well as the response, so a
/// handler logs the value the client is told.
#[tokio::test]
async fn a_handler_sees_the_request_id_the_response_carries() {
    let service = Router::<App>::new()
        .mount(kynos::routes![correlated])
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/correlated").call().await;
    assert_eq!(reply.status, StatusCode::OK);

    let carried = reply
        .field("x-request-id")
        .expect("the response carries an identifier");
    assert_eq!(reply.text(), carried);
}

/// The default source never hands one identifier to two requests, which is
/// the whole of what correlating by it needs.
#[tokio::test]
async fn two_requests_are_given_two_identifiers() {
    let service = support::router()
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let first = get(&service, "/users/1").call().await.field("x-request-id");
    let second = get(&service, "/users/1").call().await.field("x-request-id");

    assert!(
        first.is_some() && second.is_some(),
        "an identifier was not set"
    );
    assert_ne!(first, second);
}

/// A source that always answers with one identifier.
struct Fixed;

impl RequestIdSource for Fixed {
    fn next_id(&self) -> HeaderValue {
        HeaderValue::from_static("from-the-source")
    }
}

/// A replaced source decides the identifier both ends see, rather than
/// sitting beside the default one.
#[tokio::test]
async fn a_replaced_source_supplies_the_identifier() {
    let service = Router::<App>::new()
        .mount(kynos::routes![correlated])
        .intercept(RequestId::new().source(Fixed))
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/correlated").call().await;

    assert_eq!(
        reply.field("x-request-id").as_deref(),
        Some("from-the-source")
    );
    assert_eq!(reply.text(), "from-the-source");
}

/// A group carrying one identifier under two names.
struct Correlation(HeaderValue);

impl HeaderParams for Correlation {
    const NAMES: &'static [&'static str] = &["x-request-id", "x-correlation-id"];
}

impl EncodeHeaders for Correlation {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(vec![
            (HeaderName::from_static("x-request-id"), self.0.clone()),
            (HeaderName::from_static("x-correlation-id"), self.0.clone()),
        ])
    }
}

impl CorrelationHeaders for Correlation {
    fn from_id(id: HeaderValue) -> Self {
        Self(id)
    }
}

/// Every name the group declares carries one identifier, once — whether it was
/// generated or read from a trusting client, where the first declared name the
/// client sent is the one used.
#[tokio::test]
async fn a_group_naming_two_headers_sets_both_to_one_identifier() {
    let generating = support::router()
        .intercept(RequestId::new().header::<Correlation>())
        .build(App::new())
        .expect("a describable router");

    let generated = get(&generating, "/users/1").call().await;
    let id = generated.fields("x-request-id");
    assert_eq!(id.len(), 1, "one identifier under the first name: {id:?}");
    assert_eq!(generated.fields("x-correlation-id"), id);

    let trusting = support::router()
        .intercept(RequestId::new().header::<Correlation>().trust_client(true))
        .build(App::new())
        .expect("a describable router");

    let trusted = get(&trusting, "/users/1")
        .header("x-correlation-id", "second-name")
        .header("x-request-id", "first-name")
        .call()
        .await;
    assert_eq!(trusted.fields("x-request-id"), ["first-name"]);
    assert_eq!(trusted.fields("x-correlation-id"), ["first-name"]);
}

/// A correlation group that refuses every identifier it is given.
struct Unencodable;

impl HeaderParams for Unencodable {
    const NAMES: &'static [&'static str] = &["x-trace-id"];
}

impl EncodeHeaders for Unencodable {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Err(UnencodableHeader::new(HeaderName::from_static(
            "x-trace-id",
        )))
    }
}

impl CorrelationHeaders for Unencodable {
    fn from_id(id: HeaderValue) -> Self {
        let _ = id;
        Self
    }
}

/// How often [`uncorrelated`] ran.
static UNCORRELATED_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts its calls.
#[kynos::get("/uncorrelated")]
async fn uncorrelated() -> Text {
    UNCORRELATED_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Text("ran".to_owned())
}

/// A group that cannot carry the identifier answers 500 without running the
/// handler, which would otherwise act on a request its response then refuses.
#[tokio::test]
async fn a_correlation_group_that_refuses_to_encode_answers_500_unhandled() {
    let service = Router::<App>::new()
        .mount(kynos::routes![uncorrelated])
        .intercept(RequestId::new().header::<Unencodable>())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/uncorrelated").call().await;

    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.field("x-trace-id"), None);
    assert_eq!(
        UNCORRELATED_CALLS.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

/// `Cors` adds its own names to a permitted cross-origin response and nothing
/// beyond them.
#[tokio::test]
async fn cors_adds_only_the_names_it_declares() {
    let service = support::router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1")
        .header("origin", "https://app.example.com")
        .call()
        .await;

    let added: BTreeSet<String> = fields(&reply)
        .difference(&baseline().await)
        .cloned()
        .collect();

    assert_eq!(
        added,
        ["access-control-allow-origin", "vary"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );
}

/// Two interceptors that both contribute a `Vary` field union rather than
/// clobbering, which is the one response header whose contributors compose.
#[cfg(feature = "compression")]
#[tokio::test]
async fn two_interceptors_contributing_vary_both_appear_in_it() {
    use kynos::middleware::compression::Compression;

    let service = support::router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .intercept(Compression::new())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1")
        .header("origin", "https://app.example.com")
        .header("accept-encoding", "gzip")
        .call()
        .await;

    let vary = reply.fields("vary").join(", ");
    let names: BTreeSet<String> = vary
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();

    assert_eq!(
        names,
        ["accept-encoding", "origin"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        "{vary}"
    );
}

/// `SecurityHeaders` adds exactly the names its group declares, in each of
/// its four shapes: the fixed three, `X-Frame-Options` once framing is
/// denied, and `Strict-Transport-Security` on a response a trusted hop says
/// went out over HTTPS.
#[tokio::test]
async fn security_headers_add_only_the_names_they_declare() {
    use std::time::Duration;

    use kynos::{
        http::forwarded::{ProxyHeader, TrustedProxies},
        middleware::security_headers::{SecurityFields, SecurityHeaders, StrictTransportSecurity},
    };

    let hsts = || StrictTransportSecurity::max_age(Duration::from_secs(600));
    let behind_proxy =
        || support::router().trusted_proxies(TrustedProxies::hops(ProxyHeader::Forwarded, 1));
    let built = "a describable router";
    let cases = [
        (
            behind_proxy()
                .intercept(SecurityHeaders::new())
                .build(App::new())
                .expect(built),
            declared::<SecurityFields<false, false>>(),
        ),
        (
            behind_proxy()
                .intercept(SecurityHeaders::new().deny_framing())
                .build(App::new())
                .expect(built),
            declared::<SecurityFields<false, true>>(),
        ),
        (
            behind_proxy()
                .intercept(SecurityHeaders::new().strict_transport_security(hsts()))
                .build(App::new())
                .expect(built),
            declared::<SecurityFields<true, false>>(),
        ),
        (
            behind_proxy()
                .intercept(
                    SecurityHeaders::new()
                        .strict_transport_security(hsts())
                        .deny_framing(),
                )
                .build(App::new())
                .expect(built),
            declared::<SecurityFields<true, true>>(),
        ),
    ];
    let baseline = baseline().await;

    for (service, names) in cases {
        let reply = get(&service, "/users/1")
            .header("forwarded", "for=203.0.113.7;proto=https")
            .call()
            .await;
        assert_eq!(reply.status, StatusCode::OK);

        let added: BTreeSet<String> = fields(&reply).difference(&baseline).cloned().collect();
        assert_eq!(added, names);
    }
}

// --- An interceptor scoped to methods ---------------------------------------

/// The four operations in one group, behind a `RequestId` and a `BodySize`
/// covering only `methods`.
fn scoped<M: MethodSet + Copy>(methods: M) -> Router<App, Propagate, (), impl Sized> {
    Router::<App>::new().group(
        Group::new("/")
            .intercept_when(methods, RequestId::new())
            .intercept_when(methods, BodySize::new(1 << 10))
            .mount(kynos::routes![
                support::get_user,
                support::list_users,
                support::create_user,
                support::delete_user
            ]),
    )
}

/// A scoped interceptor runs on the operations whose method it names, and an
/// operation it does not name is served as if it were not mounted.
#[tokio::test]
async fn a_method_scoped_interceptor_runs_only_on_the_methods_it_names() {
    let service = scoped(UnsafeMethods)
        .build(App::new())
        .expect("a describable router");

    let read = get(&service, "/users/1").call().await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.field("x-request-id"), None);

    let removed = support::send(&service, Method::DELETE, "/users/1")
        .call()
        .await;
    assert!(removed.field("x-request-id").is_some());

    // Over the scoped `BodySize`, so it is the interceptor that answers.
    let created = post(&service, "/users")
        .header("content-type", "application/json")
        .body(vec![b' '; 2 << 10])
        .call()
        .await;
    assert_eq!(created.status, StatusCode::PAYLOAD_TOO_LARGE);
}

/// A `HEAD` the group does not declare is answered by the `GET` operation, so a
/// scope naming `GET` covers it and one naming only `HEAD` does not.
#[tokio::test]
async fn an_undeclared_head_is_covered_exactly_when_get_is() {
    #[derive(Clone, Copy)]
    struct Get;
    impl MethodSet for Get {
        const METHODS: Methods = Methods::only(&[OperationMethod::Get]);
    }

    #[derive(Clone, Copy)]
    struct Head;
    impl MethodSet for Head {
        const METHODS: Methods = Methods::only(&[OperationMethod::Head]);
    }

    for (service, covered) in [
        (scoped(Get).build(App::new()), true),
        (scoped(Head).build(App::new()), false),
    ] {
        let service = service.expect("a describable router");

        let reply = support::send(&service, Method::HEAD, "/users/1")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.field("x-request-id").is_some(), covered);
    }
}

/// Two interceptors adding the same header compose when scoped to disjoint
/// methods, and each operation runs and declares exactly the one covering it.
#[tokio::test]
async fn interceptors_scoped_to_disjoint_methods_never_meet_on_an_operation() {
    let router = Router::<App>::new().group(
        Group::new("/")
            .intercept_when(SafeMethods, RequestId::new())
            .intercept_when(UnsafeMethods, RequestId::new())
            .mount(kynos::routes![support::get_user, support::create_user]),
    );
    let document = serde_json::to_value(router.openapi().expect("a describable router"))
        .expect("a serializable document");
    let service = router.build(App::new()).expect("a describable router");

    for (path, method) in [("/users/{id}", "get"), ("/users", "post")] {
        let declared = document["paths"][path][method]["responses"]
            .as_object()
            .expect("a responses object")
            .values()
            .any(|response| response["headers"].get("X-Request-Id").is_some());
        assert!(declared, "{method} {path}");
    }

    let read = get(&service, "/users/1").call().await;
    let created = post(&service, "/users")
        .header("content-type", "application/json")
        .body(r#"{"id":2,"name":"a"}"#)
        .call()
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    for reply in [read, created] {
        assert_eq!(reply.fields("x-request-id").len(), 1);
    }
}

/// What a scoped interceptor declares lands on the operations it covers and on
/// no other: the `413` its `Short` answers, and the header it `Adds`.
#[test]
fn a_method_scoped_interceptor_is_declared_only_on_the_methods_it_names() {
    let document = serde_json::to_value(
        scoped(UnsafeMethods)
            .openapi()
            .expect("a describable router"),
    )
    .expect("a serializable document");

    for (path, method, covered) in [
        ("/users/{id}", "get", false),
        ("/users", "get", false),
        ("/users", "post", true),
        ("/users/{id}", "delete", true),
    ] {
        let responses = &document["paths"][path][method]["responses"];
        let declares_413 = responses.get("413").is_some();
        let declares_header = responses
            .as_object()
            .expect("a responses object")
            .values()
            .any(|response| response["headers"].get("X-Request-Id").is_some());

        assert_eq!(declares_413, covered, "{method} {path}: the 413");
        assert_eq!(declares_header, covered, "{method} {path}: X-Request-Id");
    }
}

// --- The two closed sets --------------------------------------------------

/// Every interceptor Kynos ships, named against the set this suite accounts
/// for.
///
/// Witnessing a set someone chose says nothing about whether the set is the
/// whole set. The declared side is therefore read off disk — every `.rs` file
/// under `src/middleware/`, walked rather than transcribed — so an interceptor
/// added in a module no list mentions still fails the build.
///
/// Naming the types rather than counting them is what makes the failure
/// readable: a count says two numbers differ, a set says which interceptor
/// nothing accounts for. It is also what lets two branches each add one and
/// merge, since alphabetical insertion puts them on different lines.
///
/// Neither side is `#[cfg]`-gated any more. Both are source text, and a file
/// exists on disk whether or not the feature that compiles it is on, so this
/// now holds at baseline features as well as under `--all-features`.
#[test]
fn every_interceptor_kynos_ships_is_accounted_for() {
    /// Sorted. `Trace` is an `Observer` rather than an `Interceptor` — it
    /// declares nothing, so it is not in this set and is named below instead.
    const WITNESSED: &[&str] = &[
        "BodySize",
        "BodyTimeout",
        "Cache",
        "Compression",
        "Concurrency",
        "Conditional",
        "Cors",
        "Csrf",
        "CsrfWhen",
        "Decompression",
        "RateLimit",
        "RequestId",
        "SecurityHeaders",
        "SetCookies",
        "Timeout",
        "TrustedHosts",
        // A stack marker with no value, so it writes nothing; the
        // method-scoped cases above read what its `N` declares.
        "When",
    ];

    let declared = implementors_of("> Interceptor<C> for ");

    assert_eq!(
        declared,
        WITNESSED
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`middleware/` implements `Interceptor` for a different set than this suite accounts \
         for; an interceptor added without a case is one whose declaration nothing reads"
    );
}

/// Every observer Kynos ships, named the same way.
///
/// A separate set because an observer declares *nothing*: it cannot add a
/// header or short-circuit, which is exactly why `Trace` needs no
/// header-versus-declaration case and does need to be accounted for somewhere.
///
/// The list this replaced opened ten files and `compression.rs` was not among
/// them, so an `Observer` implemented there would have been counted by nothing.
/// Walking the directory is what closes that.
#[test]
fn every_observer_kynos_ships_is_accounted_for() {
    /// Sorted.
    const WITNESSED: &[&str] = &["Trace"];

    let declared = implementors_of("> Observer<C> for ");

    assert_eq!(
        declared,
        WITNESSED
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`middleware/` implements `Observer` for a different set than this suite accounts for"
    );
}

/// The type names `middleware/` implements `marker` for, read off disk.
///
/// `marker` carries the `> ` that closes the impl's generic list, which is what
/// keeps `Interceptor<C>` from also matching `ErasedInterceptor<C>`.
fn implementors_of(marker: &str) -> BTreeSet<String> {
    let mut sources = Vec::new();
    collect_sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/middleware")),
        &mut sources,
    );
    assert!(
        !sources.is_empty(),
        "no sources found under `src/middleware/`"
    );

    implementors_in(marker, &sources)
}

/// The scan [`implementors_of`] runs, over source it is handed.
///
/// Whitespace is collapsed first: rustfmt wraps a long impl head before its
/// `for`, and a marker matched against the raw text misses that implementor —
/// which is how `SecurityHeaders` went uncounted.
fn implementors_in(marker: &str, sources: &[String]) -> BTreeSet<String> {
    sources
        .iter()
        .map(|source| source.split_whitespace().collect::<Vec<_>>().join(" "))
        .flat_map(|source| {
            source
                .match_indices(marker)
                .map(|(at, _)| {
                    source[at + marker.len()..]
                        .chars()
                        .take_while(|character| character.is_alphanumeric() || *character == '_')
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The scan finds an implementor whose head rustfmt wrapped before `for`, as
/// it does one written on one line, and still skips the erased trait.
#[test]
fn the_interceptor_scan_reads_a_wrapped_impl_head() {
    let sources = [
        "impl<C: Sync + 'static> Interceptor<C> for OneLine {".to_owned(),
        "impl<C, const A: bool, const B: bool> Interceptor<C>\n    for Wrapped<A, B>\nwhere"
            .to_owned(),
        "impl<C> ErasedInterceptor<C> for Erased {".to_owned(),
    ];

    assert_eq!(
        implementors_in("> Interceptor<C> for ", &sources),
        ["OneLine", "Wrapped"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );
}

/// Every `.rs` file under `directory`, read, except a sibling `tests.rs`.
///
/// Test modules are excluded because a fixture implementing `Interceptor` is
/// not something Kynos ships, and the transcribed list this replaced named no
/// `tests.rs` either.
fn collect_sources(directory: &std::path::Path, into: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read `{}`: {error}", directory.display()))
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_sources(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            into.push(
                std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read `{}`: {error}", path.display())),
            );
        }
    }
}

// --- The short circuits, and the body each owes its description -----------

/// Sorted. `Infallible` is in the set and has no case in the sweep below: it is
/// uninhabited, so there is no value to drive and no status to declare.
const SHORT_CIRCUITS: &[&str] = &[
    "AtCapacity",
    "BodyRefused",
    "BodySizeExceeded",
    "CrossSite",
    "Infallible",
    "NotAcceptable",
    "NotModified",
    "RateLimited",
    "RateLimitedFields",
    "TimedOut",
    "Undecodable",
    "UntrustedHost",
];

/// The members of `SHORT_CIRCUITS` that have no value to drive.
///
/// Membership is for a type no value of which can exist -- `Infallible` is
/// uninhabited, so there is nothing to hand `case` and nothing to put on a
/// wire. It is *not* for a type that is merely awkward to construct: every name
/// here is one the sweep stops asserting anything about. Widening it is how the
/// sweep would be silenced rather than satisfied, so a type that can be built
/// gets built, even where building it takes a fixture.
const UNCONSTRUCTIBLE: &[&str] = &["Infallible"];

/// Each excluded name beside the `STATUSES` of the type it stands for.
///
/// `UNCONSTRUCTIBLE` is `&[&str]`, and a string cannot be asked what it
/// declares -- which is why the exclusion went unchecked. This is the join
/// between the two, written once so that the check below reaches a type rather
/// than a name.
const UNCONSTRUCTIBLE_STATUSES: &[(&str, &[u16])] = &[(
    "Infallible",
    <std::convert::Infallible as kynos::response::ShortCircuit>::STATUSES,
)];

/// Every exclusion is earned rather than asserted.
///
/// Two halves, and neither alone is the check. The set equality is what makes
/// a name added to `UNCONSTRUCTIBLE` without an entry here fail, so the
/// exclusion cannot be widened by editing one list; the emptiness is what the
/// entry proves, since a type declaring a status the sweep never drives is a
/// response nothing in the suite ever reads.
#[test]
fn every_unconstructible_short_circuit_declares_no_status() {
    assert_eq!(
        UNCONSTRUCTIBLE_STATUSES
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>(),
        UNCONSTRUCTIBLE.iter().copied().collect::<BTreeSet<_>>(),
        "a name is excluded from the sweep with nothing here to prove it earned"
    );

    for (name, statuses) in UNCONSTRUCTIBLE_STATUSES {
        assert!(
            statuses.is_empty(),
            "`{name}` is excluded from the sweep and declares {statuses:?}, which no test reads"
        );
    }
}

/// Every short circuit Kynos ships, named against the set the sweep drives.
///
/// The same argument as the two sets above, one trait further out: without it a
/// short circuit beyond the eleven listed is a type the sweep silently does not
/// reach. The walk is over the whole crate rather than `src/middleware/`,
/// because `Infallible`'s implementation is in `src/response/mod.rs`.
#[test]
fn every_short_circuit_kynos_ships_is_accounted_for() {
    let declared = impls_of("ShortCircuit");

    assert_eq!(
        declared,
        SHORT_CIRCUITS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`src/` implements `ShortCircuit` for a different set than the sweep accounts for; a \
         short circuit added without a case is one whose description nothing reads"
    );
}

/// The type names `crates/kynos/src` implements `trait_name` for, read off disk.
///
/// Line-oriented rather than the `> Marker for ` substring `implementors_of`
/// uses, for two reasons that marker cannot cover: `ShortCircuit` takes no
/// generic argument to close the match on, and `middleware/limits/timeout.rs`
/// writes `impl ShortCircuit for TookTooLong` inside a doc example, which a bare
/// substring would count as a shipped implementation. Two conditions rather
/// than one: the line's code *begins* with `impl`, which excludes the `/// # `
/// that doc example carries, and the trait name begins a path segment, which
/// admits `impl crate::response::ShortCircuit for NotAcceptable` and refuses a
/// `MyShortCircuit` that merely ends in the name.
///
/// # What it requires of an implementation
///
/// `impl`, the trait name and `for` must fall on one line. Every implementation
/// in the tree does, and `ShortCircuit` takes no generic argument to push one
/// over the width.
///
/// # The two shapes it cannot see
///
/// Both go quiet rather than red, and the sweep derives what it must drive
/// from this set, so a miss here is a miss there too.
///
/// The first is a head rustfmt wrapped after a generic list, which puts
/// `impl<T: Bound>` and `ShortCircuit for Foo<T>` on separate lines with
/// neither carrying both. The prefix was `impl ` with a space until now, which
/// made this wider than its own description: `impl<const CODE: u16>` has no
/// space in that position either, so *every* generic head was invisible,
/// wrapped or not, and `response/status.rs` already writes that shape for
/// `Redirect<CODE>`. Dropping the space leaves only the genuinely wrapped head
/// missed, and an implementation added later keeps its head on one line or
/// this learns to join continuations first.
///
/// The second is a derive, and it is not closable here. `ApiError` emits
/// `impl ... ShortCircuit for` from `kynos-macros`, so the text exists in the
/// macro crate and the implementation exists in whichever crate wrote the
/// type. A walk over `crates/kynos/src` sees neither. That bounds what this
/// set claims rather than leaving a gap in it: the claim is over the short
/// circuits Kynos *ships*, and a derived one belongs to the application. What
/// the sweep would say about it is said instead by the conformance matrix,
/// which holds an application's own short circuit on a live exchange.
fn impls_of(trait_name: &str) -> BTreeSet<String> {
    let mut sources = Vec::new();
    collect_sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
        &mut sources,
    );
    assert!(!sources.is_empty(), "no sources found under `src/`");

    names_implementing(trait_name, &sources)
}

/// The scan itself, over source it is handed rather than source it reads.
///
/// Separate from [`impls_of`] so that the two conditions above can be held
/// against text: what the walk finds on disk is whatever the tree happens to
/// contain today, so a condition can only be *asserted* over source chosen to
/// exercise it.
fn names_implementing(trait_name: &str, sources: &[String]) -> BTreeSet<String> {
    let marker = format!("{trait_name} for ");

    sources
        .iter()
        .flat_map(|source| source.lines())
        .filter_map(|line| {
            let code = line.trim_start();
            if !code.starts_with("impl") {
                return None;
            }
            let at = code.match_indices(&marker).map(|(at, _)| at).find(|at| {
                code[..*at]
                    .chars()
                    .next_back()
                    .is_none_or(|character| !character.is_alphanumeric() && character != '_')
            })?;
            Some(
                code[at + marker.len()..]
                    .chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect::<String>(),
            )
        })
        .collect()
}

/// Each condition the scan applies, held against a line written to break it.
///
/// The set the sweep drives is derived from what this finds, so a condition
/// that stops holding is a short circuit that stops being swept -- silently,
/// because the walk would simply return a smaller set and every assertion over
/// it would still pass. What the tree contains cannot say this: it holds no
/// `MyShortCircuit`, and dropping either condition leaves the walk's own result
/// unchanged today.
///
/// One line per claim the documentation above makes. The generic head is the
/// shape `impl` without a trailing space exists for; the qualified path is the
/// spelling `middleware/compression/mod.rs` uses; `MyShortCircuit` is a name
/// ending in the trait's that implements something else; and the doc-example
/// line
/// is the one `middleware/limits/timeout.rs` really carries.
#[test]
fn the_scan_reads_a_generic_head_and_anchors_the_trait_name() {
    let sources = [
        "impl<const CODE: u16> ShortCircuit for Redirected<CODE> {".to_owned(),
        "impl ShortCircuit for Plain {".to_owned(),
        "    impl crate::response::ShortCircuit for Qualified {".to_owned(),
        "impl MyShortCircuit for NotOurs {".to_owned(),
        "/// # impl ShortCircuit for InADocExample {".to_owned(),
    ];

    assert_eq!(
        names_implementing("ShortCircuit", &sources),
        ["Plain", "Qualified", "Redirected"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );
}

/// One short circuit's wire response, beside the description it declared.
struct Case {
    name: &'static str,
    /// The statuses the implementation claims, so the sweep can assert it drove
    /// every one of them rather than whichever variant it happened to name.
    claimed: &'static [u16],
    /// The status this value answered with. Read from the wire rather than from
    /// `STATUSES`, because a `STATUSES` list may hold several and one value
    /// answers with exactly one — `Undecodable` declares three and needs three
    /// variants driven to reach them.
    status: u16,
    media_type: Option<String>,
    body: bytes::Bytes,
    declared: kynos::openapi::Responses,
    /// What `IntoProblem` yields for the same value, for a short circuit that
    /// implements it; `None` for one that carries no problem.
    held: Option<Held>,
}

/// A refusal's `IntoProblem` half: what a `#[problem(transparent)]` variant
/// holding it would read.
struct Held {
    statuses: Vec<u16>,
    problem: serde_json::Value,
}

/// Drives one short circuit and records both halves of its promise.
async fn case<S: kynos::response::ShortCircuit>(
    registry: &mut kynos::schema::registry::Registry,
    value: S,
) -> Case {
    use http_body_util::BodyExt;

    // `responses` and `into_response` resolve through `ShortCircuit`'s
    // supertraits, so neither trait is imported here.
    let declared = S::responses(registry);
    let (parts, body) = value.into_response().into_parts();

    let media_type = parts
        .headers
        .get(kynos::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let (media_type, _) = value.split_once(';').unwrap_or((value, ""));
            media_type.trim().to_ascii_lowercase()
        });

    let body = body.collect().await.expect("a readable body").to_bytes();

    Case {
        // The generic argument goes first, then the path. `type_name` of a
        // generic short circuit is `..::RateLimited<..::Throttled>`, whose last
        // `::` segment is `Throttled>` -- a name that appears in no source
        // file, so the sweep would report a short circuit nobody wrote. The
        // source-text half already stops at `<`, and this is the same rule on
        // the other side of the comparison.
        name: std::any::type_name::<S>()
            .split('<')
            .next()
            .expect("a type name is not empty")
            .rsplit("::")
            .next()
            .expect("a type name has a last segment"),
        claimed: S::STATUSES,
        status: parts.status.as_u16(),
        media_type,
        body,
        declared,
        held: None,
    }
}

/// Drives one refusal as [`case`] does, and records what `IntoProblem` yields
/// for a copy of the same value beside it.
async fn refusal<S>(registry: &mut kynos::schema::registry::Registry, value: S) -> Case
where
    S: kynos::response::ShortCircuit + kynos::error::problem::IntoProblem + Clone,
{
    let held = Held {
        statuses: S::statuses().iter().map(StatusCode::as_u16).collect(),
        problem: serde_json::to_value(value.clone().into_problem()).expect("a problem serializes"),
    };

    Case {
        held: Some(held),
        ..case(registry, value).await
    }
}

/// One value per short circuit this build compiled, driven against one registry.
///
/// Values rather than types, because a description is a claim about what the
/// wire carries and only a value produces one. `Undecodable` appears three
/// times: it declares three statuses and one value answers with one of them.
async fn every_case() -> Vec<Case> {
    use std::time::Duration;

    use kynos::middleware::{
        csrf::CrossSite,
        limits::{
            body_size::{BodyRefused, BodySizeExceeded, BodyTooSlow},
            concurrency::AtCapacity,
            timeout::TimedOut,
        },
        rate_limit::refusal::{RateLimited, RateLimitedFields},
        trusted_hosts::UntrustedHost,
    };

    let registry = &mut kynos::schema::registry::Registry::new();
    let mut cases = Vec::new();

    cases.push(refusal(registry, BodySizeExceeded::<()>::new(64)).await);
    cases.push(
        refusal(
            registry,
            BodyRefused::<(), ()>::TooLarge(BodySizeExceeded::new(64)),
        )
        .await,
    );
    cases.push(
        refusal(
            registry,
            BodyRefused::<(), ()>::TooSlow(BodyTooSlow::new(Duration::from_secs(1))),
        )
        .await,
    );
    cases.push(refusal(registry, TimedOut::<()>::new(Duration::from_secs(1))).await);
    cases.push(
        refusal(
            registry,
            AtCapacity::<()>::new(Some(Duration::from_secs(1))),
        )
        .await,
    );
    cases.push(refusal(registry, CrossSite::<()>::new()).await);
    cases.push(refusal(registry, UntrustedHost::<()>::new()).await);
    cases.push(refusal(registry, RateLimited::<()>::new(Duration::from_secs(1), 10)).await);
    cases.push(
        refusal(
            registry,
            RateLimitedFields::<()>::new(Duration::from_secs(1), Vec::new(), Vec::new()),
        )
        .await,
    );

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{compression::NotAcceptable, decompression::Undecodable};

        cases.push(refusal(registry, NotAcceptable::<()>::new()).await);
        cases.push(refusal(registry, Undecodable::<(), (), ()>::unsupported_coding()).await);
        cases.push(refusal(registry, Undecodable::<(), (), ()>::malformed()).await);
        cases.push(refusal(registry, Undecodable::<(), (), ()>::too_large(64)).await);
    }

    #[cfg(feature = "cache")]
    {
        use kynos::{http::HeaderMap, middleware::conditional::NotModified};

        cases.push(case(registry, NotModified::from_headers(&HeaderMap::new())).await);
    }

    cases
}

/// The names the sweep must drive, derived from `SHORT_CIRCUITS` rather than
/// transcribed a second time — so an eleventh implementation fails both tests.
///
/// A `cfg` per element rather than a second list, so the set stays derived at
/// every feature combination.
fn expected_names() -> BTreeSet<&'static str> {
    /// Whichever members this build did not compile.
    const ABSENT: &[&str] = &[
        #[cfg(not(feature = "compression"))]
        "NotAcceptable",
        #[cfg(not(feature = "compression"))]
        "Undecodable",
        #[cfg(not(feature = "cache"))]
        "NotModified",
    ];

    SHORT_CIRCUITS
        .iter()
        .copied()
        .filter(|name| !UNCONSTRUCTIBLE.contains(name) && !ABSENT.contains(name))
        .collect()
}

/// Every short circuit's *described* response declares the body its *wire*
/// response sends.
///
/// The defect issue #104 reported, asserted over the whole set rather than at
/// the site that was noticed: eight of the ten implementations put a problem
/// document on the wire under a response declaring no content, and
/// `assert_conformance` read an undeclared content as "nothing to check".
///
/// Both directions. `NotModified` sends no body and must go on declaring none,
/// so this is not "every short circuit declares content" — it is "the
/// declaration and the exchange agree".
#[tokio::test]
async fn every_short_circuit_declares_the_content_it_sends() {
    let cases = every_case().await;

    let driven: BTreeSet<&str> = cases.iter().map(|case| case.name).collect();
    assert_eq!(
        driven,
        expected_names(),
        "a short circuit the sweep does not drive"
    );

    // Every status an implementation claims has a value behind it, so a variant
    // added to a multi-status short circuit cannot go undriven.
    for name in &driven {
        let claimed: BTreeSet<u16> = cases
            .iter()
            .find(|case| &case.name == name)
            .expect("a driven case")
            .claimed
            .iter()
            .copied()
            .collect();
        let reached: BTreeSet<u16> = cases
            .iter()
            .filter(|case| &case.name == name)
            .map(|case| case.status)
            .collect();
        assert_eq!(
            reached, claimed,
            "`{name}` declares statuses the sweep does not drive a value to"
        );
    }

    for case in &cases {
        let key = kynos::openapi::StatusPattern::Code(case.status).to_string();
        let Some(kynos::openapi::RefOr::Item(declared)) = case.declared.responses.get(&key) else {
            panic!(
                "`{}` answers {} and its description declares no such response",
                case.name, case.status
            );
        };

        if let Some(media_type) = case.media_type.as_deref() {
            assert!(
                declared.content.contains_key(media_type),
                "`{}`'s {} sends a `{media_type}` body the description does not declare",
                case.name,
                case.status
            );
        } else {
            assert!(
                case.body.is_empty(),
                "`{}`'s {} sends {} bytes with no `Content-Type`",
                case.name,
                case.status,
                case.body.len()
            );
            assert!(
                declared.content.is_empty(),
                "`{}`'s {} declares content it does not send",
                case.name,
                case.status
            );
        }
    }
}

/// The members of `SHORT_CIRCUITS` that carry no problem, and so implement no
/// `IntoProblem`: `Infallible` sends nothing, and `NotModified`'s 304 has no
/// content (RFC 9110 section 15.4.5).
const PROBLEMLESS: &[&str] = &["Infallible", "NotModified"];

/// Every short circuit Kynos ships that sends a problem implements
/// `IntoProblem`, so a `#[problem(transparent)]` variant can hold any of them.
///
/// Read off disk, as the set above is, so a refusal added without the
/// implementation fails here at every feature set rather than only where the
/// sweep below compiles it. `IntoProblem` is implemented across the crate, by
/// every extractor rejection too, so the claim is over the intersection.
#[test]
fn every_short_circuit_sending_a_problem_implements_into_problem() {
    let short_circuits = impls_of("ShortCircuit");
    let problems = impls_of("IntoProblem");

    assert_eq!(
        short_circuits
            .intersection(&problems)
            .cloned()
            .collect::<BTreeSet<_>>(),
        SHORT_CIRCUITS
            .iter()
            .filter(|name| !PROBLEMLESS.contains(name))
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "a short circuit sends a problem a `#[problem(transparent)]` variant cannot hold, or \
         one that carries none implements `IntoProblem`"
    );
}

/// Every refusal's `IntoProblem` is the problem its `into_response` sends, and
/// claims the statuses its `ShortCircuit` does.
///
/// A transparent variant reads the first and answers through the second, so
/// the two parting company is a variant whose `into_problem` describes a
/// different failure than the one it put on the wire. The comparison is of the
/// whole document, so a header-only field such as `Retry-After` stays off it.
#[tokio::test]
async fn every_refusal_yields_the_problem_it_sends() {
    let cases = every_case().await;

    let held: BTreeSet<&str> = cases
        .iter()
        .filter(|case| case.held.is_some())
        .map(|case| case.name)
        .collect();
    assert_eq!(
        held,
        expected_names()
            .into_iter()
            .filter(|name| !PROBLEMLESS.contains(name))
            .collect(),
        "a refusal the sweep drives without reading its `IntoProblem`"
    );

    for case in &cases {
        let Some(held) = &case.held else { continue };

        assert_eq!(
            held.statuses, case.claimed,
            "`{}`'s `IntoProblem::statuses` and `ShortCircuit::STATUSES` disagree",
            case.name
        );

        let sent: serde_json::Value =
            serde_json::from_slice(&case.body).expect("a refusal sends a JSON problem");
        assert_eq!(
            sent, held.problem,
            "`{}`'s {} sends a problem other than the one `into_problem` yields",
            case.name, case.status
        );
    }
}

// --- What a marker must not cost the type that names it -------------------
//
// The four implementations every parameterised refusal writes out by hand, and
// the auto traits its `PhantomData<fn() -> T>` protects. Held here rather than
// per module because it is one rule over a set: a `#[derive]` anywhere in that
// set would bound the marker and take the implementation away from every
// application whose marker is only a name.

/// A marker that is deliberately neither `Send` nor `Sync`.
///
/// A raw pointer is the cheapest way to be neither. It is still `'static`, so
/// it satisfies `ProblemType` and the only thing under test is whether the
/// refusal's auto traits followed it.
struct Unsendable(std::marker::PhantomData<*const ()>);

impl kynos::error::problem::ProblemType for Unsendable {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/unsendable");
}

/// A marker deriving nothing at all, which is what an application writes.
struct Bare;

impl kynos::error::problem::ProblemType for Bare {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/bare");
}

/// Witnesses that a refusal crosses a task boundary whatever names its type.
fn assert_send_sync<T: Send + Sync>() {}

/// Witnesses the four implementations a refusal writes out.
fn assert_refusal_traits<T: Clone + std::fmt::Debug + Eq>() {}

/// Witnesses the two an interceptor writes out.
fn assert_clone_and_debug<T: Clone + std::fmt::Debug>() {}

/// A refusal is `Send` and `Sync` whatever marker names its problem type.
///
/// The field is `PhantomData<fn() -> T>` rather than `PhantomData<T>` for this
/// reason and no other: a `PhantomData<T>` inherits `T`'s auto traits, and a
/// refusal that is not `Send` cannot be returned from an interceptor at all —
/// a bound failure at every mount site, from a marker the application thought
/// was only a name.
#[test]
fn a_refusal_is_send_and_sync_whatever_marker_names_it() {
    use kynos::middleware::{
        csrf::CrossSite,
        limits::{
            body_size::{BodyRefused, BodySizeExceeded, BodyTooSlow},
            concurrency::AtCapacity,
            timeout::TimedOut,
        },
        trusted_hosts::UntrustedHost,
    };

    use kynos::middleware::rate_limit::refusal::{RateLimited, RateLimitedFields};

    assert_send_sync::<UntrustedHost<Unsendable>>();
    assert_send_sync::<BodySizeExceeded<Unsendable>>();
    assert_send_sync::<BodyTooSlow<Unsendable>>();
    assert_send_sync::<BodyRefused<Unsendable, Unsendable>>();
    assert_send_sync::<TimedOut<Unsendable>>();
    assert_send_sync::<AtCapacity<Unsendable>>();
    assert_send_sync::<CrossSite<Unsendable>>();
    assert_send_sync::<RateLimited<Unsendable>>();
    assert_send_sync::<RateLimitedFields<Unsendable>>();

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{compression::NotAcceptable, decompression::Undecodable};

        assert_send_sync::<NotAcceptable<Unsendable>>();
        assert_send_sync::<Undecodable<Unsendable, Unsendable, Unsendable>>();
    }
}

/// A policy stand-in: only `RateLimit`'s own bounds are under test, so this
/// need not be a `RateLimitPolicy`.
#[derive(Clone, Debug)]
struct Policy;

/// The refusals `naming_a_problem_type_costs_the_marker_no_derives` and
/// `a_refusal_is_send_and_sync_whatever_marker_names_it` witness. Sorted.
const MARKED_REFUSALS: &[&str] = &[
    "AtCapacity",
    "BodyRefused",
    "BodySizeExceeded",
    "CrossSite",
    "NotAcceptable",
    "RateLimited",
    "RateLimitedFields",
    "TimedOut",
    "Undecodable",
    "UntrustedHost",
];

/// The interceptors `naming_a_problem_type_costs_the_marker_no_derives`
/// witnesses. Sorted.
const MARKED_INTERCEPTORS: &[&str] = &[
    "BodySize",
    "Compression",
    "Concurrency",
    "Csrf",
    "Decompression",
    "RateLimit",
    "Timeout",
    "TrustedHosts",
];

/// Every type that names a problem type, read off disk and named against the
/// sets the two witnesses above cover — so a refusal or an interceptor gaining
/// a marker fails here until something holds what the marker costs it.
///
/// A refusal is a `ShortCircuit` whose implementation is bounded on
/// `ProblemType`; an interceptor is what a `problem_type` builder returns.
#[test]
fn every_type_naming_a_problem_type_is_witnessed() {
    let mut sources = Vec::new();
    collect_sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/middleware")),
        &mut sources,
    );
    // Comments dropped, so a doc example's `impl` is not read as one, and
    // whitespace collapsed, so a wrapped impl head reads as one line.
    let sources: Vec<String> = sources
        .iter()
        .map(|source| {
            source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .flat_map(str::split_whitespace)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let named = |names: &[&str]| -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    };
    let identifier = |text: &str| -> String {
        text.chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect()
    };

    // An impl head runs from `impl<` to its body's `{`, where clause included.
    let refusals: BTreeSet<String> = sources
        .iter()
        .flat_map(|source| {
            source
                .match_indices("impl<")
                .filter_map(|(at, _)| {
                    let head = &source[at..at + source[at..].find('{')?];
                    let trait_at = head.find("ShortCircuit for ")?;
                    head.contains("ProblemType")
                        .then(|| identifier(&head[trait_at + "ShortCircuit for ".len()..]))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let interceptors: BTreeSet<String> = sources
        .iter()
        .flat_map(|source| {
            source
                .match_indices("problem_type<")
                .filter_map(|(at, _)| {
                    let rest = &source[at..];
                    let arrow = rest.find("(self) -> ")?;
                    Some(identifier(&rest[arrow + "(self) -> ".len()..]))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    assert_eq!(refusals, named(MARKED_REFUSALS));
    assert_eq!(interceptors, named(MARKED_INTERCEPTORS));
}

/// A refusal and its interceptor keep their implementations whatever names the
/// problem type.
///
/// `Bare` derives nothing, so these calls are the whole test: a `#[derive]` on
/// any of these types would bound the marker and refuse them.
#[test]
fn naming_a_problem_type_costs_the_marker_no_derives() {
    use kynos::middleware::{
        csrf::{CrossSite, Csrf},
        limits::{
            body_size::{BodyRefused, BodySize, BodySizeExceeded, BodyTooSlow},
            concurrency::{AtCapacity, Concurrency},
            timeout::TimedOut,
        },
    };

    use kynos::middleware::{
        limits::timeout::Timeout,
        rate_limit::{
            Legacy, RateLimit, Structured,
            refusal::{RateLimited, RateLimitedFields},
        },
        trusted_hosts::{TrustedHosts, UntrustedHost},
    };

    assert_refusal_traits::<UntrustedHost<Bare>>();
    assert_clone_and_debug::<TrustedHosts<Bare>>();
    assert_refusal_traits::<BodySizeExceeded<Bare>>();
    assert_refusal_traits::<BodyTooSlow<Bare>>();
    assert_refusal_traits::<BodyRefused<Bare, Bare>>();
    assert_refusal_traits::<TimedOut<Bare>>();
    assert_refusal_traits::<AtCapacity<Bare>>();
    assert_refusal_traits::<CrossSite<Bare>>();
    assert_refusal_traits::<RateLimited<Bare>>();
    assert_refusal_traits::<RateLimitedFields<Bare>>();

    assert_clone_and_debug::<BodySize<Bare>>();
    assert_clone_and_debug::<BodySize<Bare, BodyTooSlow<Bare>>>();
    assert_clone_and_debug::<Concurrency<Bare>>();
    assert_clone_and_debug::<Csrf<Bare>>();
    assert_clone_and_debug::<Timeout<TimedOut<Bare>>>();
    assert_clone_and_debug::<RateLimit<Policy, Legacy, Bare>>();
    assert_clone_and_debug::<RateLimit<Policy, Structured, Bare>>();

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{
            compression::{Compression, NotAcceptable},
            decompression::{Decompression, Undecodable},
        };

        assert_refusal_traits::<NotAcceptable<Bare>>();
        assert_refusal_traits::<Undecodable<Bare, Bare, Bare>>();

        assert_clone_and_debug::<Compression<Bare>>();
        assert_clone_and_debug::<Decompression<Bare, Bare, Bare>>();
    }
}

// --- What the written-out implementations say ------------------------------

/// Witnesses the `Copy` a marked type writes out beside its `Clone`.
fn assert_copy<T: Copy>() {}

/// Each refusal a marker names prints, compares and clones exactly the fields
/// it carries, and nothing of the marker.
///
/// The witnesses above prove the implementations exist; this proves what they
/// do, since nothing regenerates a written-out `Debug` or `eq` when a field
/// moves. One inequality per field, for the reason
/// `two_refusals_differing_in_one_field_are_unequal` gives in the rate
/// limiter's own suite, which already holds its two refusals to this.
#[test]
fn a_marked_refusal_prints_compares_and_clones_its_fields() {
    use std::time::Duration;

    use kynos::middleware::{
        csrf::CrossSite,
        limits::{body_size::BodySizeExceeded, concurrency::AtCapacity, timeout::TimedOut},
        trusted_hosts::UntrustedHost,
    };

    assert_copy::<CrossSite<Bare>>();
    assert_copy::<UntrustedHost<Bare>>();
    assert_copy::<TimedOut<Bare>>();
    assert_copy::<BodySizeExceeded<Bare>>();
    assert_copy::<AtCapacity<Bare>>();

    // A field-free refusal prints its name alone, and every one equals every
    // other: there is nothing to tell two apart by.
    let cross_site = CrossSite::<Bare>::new();
    assert_eq!(format!("{cross_site:?}"), "CrossSite");
    assert_eq!(cross_site.clone(), CrossSite::<Bare>::default());

    let untrusted_host = UntrustedHost::<Bare>::new();
    assert_eq!(format!("{untrusted_host:?}"), "UntrustedHost");
    assert_eq!(untrusted_host.clone(), UntrustedHost::<Bare>::default());

    let timed_out = TimedOut::<Bare>::new(Duration::from_secs(5));
    assert_eq!(format!("{timed_out:?}"), "TimedOut { after: 5s }");
    assert_eq!(timed_out.clone(), timed_out);
    assert_ne!(
        timed_out,
        TimedOut::new(Duration::from_secs(6)),
        "after is not compared"
    );

    let too_large = BodySizeExceeded::<Bare>::new(16);
    assert_eq!(format!("{too_large:?}"), "BodySizeExceeded { limit: 16 }");
    assert_eq!(too_large.clone(), too_large);
    assert_ne!(
        too_large,
        BodySizeExceeded::new(17),
        "limit is not compared"
    );

    let at_capacity = AtCapacity::<Bare>::new(Some(Duration::from_secs(2)));
    assert_eq!(
        format!("{at_capacity:?}"),
        "AtCapacity { retry_after: Some(2s) }"
    );
    assert_eq!(at_capacity.clone(), at_capacity);
    assert_ne!(
        at_capacity,
        AtCapacity::new(None),
        "retry_after is not compared"
    );

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{compression::NotAcceptable, decompression::Undecodable};

        assert_copy::<NotAcceptable<Bare>>();
        assert_copy::<Undecodable<Bare, Bare, Bare>>();

        let not_acceptable = NotAcceptable::<Bare>::new();
        assert_eq!(format!("{not_acceptable:?}"), "NotAcceptable");
        assert_eq!(not_acceptable.clone(), NotAcceptable::<Bare>::default());

        let undecodable = Undecodable::<Bare, Bare, Bare>::too_large(10);
        assert_eq!(
            format!("{undecodable:?}"),
            "Undecodable { reason: TooLarge { limit: 10 } }"
        );
        assert_eq!(
            format!("{:?}", Undecodable::<Bare, Bare, Bare>::malformed()),
            "Undecodable { reason: Malformed }"
        );
        assert_eq!(undecodable.clone(), undecodable);
        assert_ne!(
            undecodable,
            Undecodable::too_large(11),
            "reason is not compared"
        );
        assert_ne!(
            Undecodable::<Bare, Bare, Bare>::unsupported_coding(),
            Undecodable::malformed(),
            "reason is not compared"
        );
    }
}

/// The same for the two refusals a body deadline answers with, kept apart
/// only so neither function outgrows the length lint.
#[test]
fn a_body_deadline_refusal_prints_compares_and_clones_its_fields() {
    use std::time::Duration;

    use kynos::middleware::limits::body_size::{BodyRefused, BodySizeExceeded, BodyTooSlow};

    assert_copy::<BodyTooSlow<Bare>>();
    assert_copy::<BodyRefused<Bare, Bare>>();

    let too_slow = BodyTooSlow::<Bare>::new(Duration::from_secs(5));
    assert_eq!(
        format!("{too_slow:?}"),
        "BodyTooSlow { after: 5s, closes: false }"
    );
    assert_eq!(too_slow.clone(), too_slow);
    assert_ne!(
        too_slow,
        BodyTooSlow::new(Duration::from_secs(6)),
        "after is not compared"
    );

    let too_large = BodySizeExceeded::<Bare>::new(16);
    let refused = BodyRefused::<Bare, Bare>::TooSlow(too_slow);
    assert_eq!(
        format!("{refused:?}"),
        "TooSlow(BodyTooSlow { after: 5s, closes: false })"
    );
    assert_eq!(refused.clone(), refused);
    assert_ne!(
        refused,
        BodyRefused::TooLarge(too_large),
        "the variant is not compared"
    );
    assert_ne!(
        BodyRefused::<Bare, Bare>::TooLarge(too_large),
        BodyRefused::TooLarge(BodySizeExceeded::new(17)),
        "a variant's refusal is not compared"
    );
}

/// Each interceptor a marker names prints every setting it holds, and a clone
/// prints the same.
///
/// None compares, so a clone is held to its original through what it prints.
/// `Timeout` alone ends non-exhaustively: its marker is the response it
/// answers with, which is a type and no setting.
///
/// `TrustedHosts` is held apart below, only so this does not outgrow the
/// length lint.
#[test]
fn a_marked_interceptor_prints_and_clones_its_settings() {
    use std::{num::NonZeroUsize, time::Duration};

    use kynos::middleware::{
        csrf::Csrf,
        limits::{
            body_size::{BodySize, BodyTooSlow},
            concurrency::Concurrency,
            timeout::Timeout,
        },
    };

    /// What `interceptor` prints, asserting its clone prints the same.
    fn printed<T: Clone + std::fmt::Debug>(interceptor: &T) -> String {
        let printed = format!("{interceptor:?}");
        assert_eq!(format!("{:?}", interceptor.clone()), printed);
        printed
    }

    assert_copy::<BodySize<Bare>>();
    assert_copy::<BodySize<Bare, BodyTooSlow<Bare>>>();
    assert_copy::<Timeout<kynos::middleware::limits::timeout::TimedOut<Bare>>>();

    assert_eq!(
        printed(
            &Csrf::new()
                .trusting_origin("https://a.example")
                .problem_type::<Bare>()
        ),
        r#"Csrf { trusted: ["https://a.example"] }"#
    );
    assert_eq!(printed(&Csrf::<Bare>::default()), "Csrf { trusted: [] }");
    assert_eq!(
        printed(&Timeout::new(Duration::from_secs(5)).problem_type::<Bare>()),
        "Timeout { limit: 5s, .. }"
    );
    assert_eq!(
        printed(&BodySize::new(4096).problem_type::<Bare>()),
        "BodySize { limit: 4096 }"
    );
    assert_eq!(
        printed(
            &BodySize::new(4096)
                .deadline(Duration::from_secs(30))
                .deadline_problem_type::<Bare>()
                .problem_type::<Bare>()
        ),
        "BodySize { limit: 4096, deadline: 30s }"
    );

    // The slots are tokio's semaphore, whose own `Debug` is not this crate's
    // to pin; the settings either side of it are.
    let concurrency = printed(
        &Concurrency::new(NonZeroUsize::new(3).expect("three is not zero"))
            .queue_for(Duration::from_millis(250))
            .retry_after(Duration::from_secs(1))
            .problem_type::<Bare>(),
    );
    assert!(
        concurrency.starts_with("Concurrency { limit: 3, slots: "),
        "{concurrency}"
    );
    assert!(
        concurrency.ends_with(", queue_for: 250ms, retry_after: Some(1s) }"),
        "{concurrency}"
    );

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{
            compression::{Compression, streaming::LatencyMode},
            decompression::Decompression,
        };

        assert_copy::<Compression<Bare>>();
        assert_copy::<Decompression<Bare, Bare, Bare>>();

        let compression = printed(
            &Compression::new()
                .min_size(512)
                .latency_mode(LatencyMode::Throughput)
                .problem_type::<Bare>(),
        );
        assert!(
            compression.starts_with("Compression { min_size: 512, levels: Levels { "),
            "{compression}"
        );
        assert!(
            compression.ends_with(" }, latency: Throughput }"),
            "{compression}"
        );

        assert_eq!(
            printed(
                &Decompression::new(1024)
                    .max_ratio(10)
                    .too_large_problem_type::<Bare>()
            ),
            "Decompression { limit: 1024, max_ratio: Some(10) }"
        );
        assert_eq!(
            printed(&Decompression::new(1024)),
            "Decompression { limit: 1024, max_ratio: None }"
        );
    }
}

/// `TrustedHosts` prints each host as it is compared, and whether loopback is
/// admitted; a clone prints the same.
#[test]
fn trusted_hosts_prints_and_clones_its_settings() {
    use kynos::middleware::trusted_hosts::TrustedHosts;

    let hosts = TrustedHosts::new()
        .allow("API.example.com")
        .allow("::1")
        .allow_loopback()
        .problem_type::<Bare>();
    let printed = format!("{hosts:?}");

    assert_eq!(format!("{:?}", hosts.clone()), printed);
    assert_eq!(
        printed,
        r#"TrustedHosts { hosts: ["api.example.com", "[::1]"], loopback: true }"#
    );
    assert_eq!(
        format!("{:?}", TrustedHosts::<Bare>::default()),
        "TrustedHosts { hosts: [], loopback: false }"
    );
}

/// `Compression::new()` and `Compression::default()` are one configuration,
/// as is naming a problem type on the first and defaulting the second.
///
/// The two are written separately, and `Compression` does not compare, so
/// what each prints — every setting it holds — is what is held equal.
#[cfg(feature = "compression")]
#[test]
fn a_new_compression_is_the_default_one() {
    use kynos::middleware::compression::Compression;

    assert_eq!(
        format!("{:?}", Compression::new()),
        format!("{:?}", Compression::<()>::default())
    );
    assert_eq!(
        format!("{:?}", Compression::new().problem_type::<Bare>()),
        format!("{:?}", Compression::<Bare>::default())
    );
}
