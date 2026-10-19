//! What the escape hatches actually do at run time.
//!
//! One reason, and it is a gap rather than a nicety: `crates/kynos/tests/`
//! contained no behavioural coverage of `route_unchecked`, `layer_unchecked` or
//! `upgrade_unchecked`, and `src/unchecked.rs` has no `#[cfg(test)]` module.
//! Nothing asserted that a catch-all serves a request, that interceptors run
//! over one, or that `x-kynos-opaque-routes` reaches a router-produced
//! document. Static assets are about to rest on all three.

#![cfg(all(feature = "unchecked", feature = "macros", feature = "json"))]

use kynos::{
    Router,
    http::{Method, Request, Response, StatusCode, body::Body},
    middleware::{Continued, Interceptor, Next, request_id::RequestId},
    openapi::{Opaque, OpaqueReason, OpaqueRoute},
    response::status::NoContent,
    router::group::Group,
};

#[path = "support/mod.rs"]
mod support;

use support::{get, outcome};

/// Serves the path it was reached by, so a test can see what the router
/// captured.
async fn echo_path(request: Request) -> Response {
    let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(
        request.uri().path().to_owned(),
    )));
    *response.status_mut() = StatusCode::OK;
    response
}

/// A router serving one catch-all under `/static`.
fn with_catch_all() -> Router<()> {
    Router::<()>::new().route_unchecked([Method::GET], "/static/{*path}", echo_path)
}

// --- That it serves at all ------------------------------------------------

/// A catch-all reaches its handler, at any depth.
#[tokio::test]
async fn a_catch_all_serves_every_path_beneath_its_prefix() {
    let service = with_catch_all().build(()).expect("a buildable router");

    for path in [
        "/static/app.css",
        "/static/css/app.css",
        "/static/a/b/c.png",
    ] {
        let reply = get(&service, path).call().await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.text(), path);
    }
}

/// And nothing outside it.
#[tokio::test]
async fn a_catch_all_does_not_answer_outside_its_prefix() {
    let service = with_catch_all().build(()).expect("a buildable router");

    assert_eq!(
        get(&service, "/elsewhere").call().await.status,
        StatusCode::NOT_FOUND
    );
}

/// matchit's catch-all does not match the prefix itself, or the prefix with a
/// trailing slash.
///
/// Recorded rather than fixed: it is the matcher's rule, and a service wanting
/// an index at the mount root registers one. Closing this would turn a test red
/// rather than nothing.
#[tokio::test]
async fn a_catch_all_matches_neither_its_own_prefix_nor_a_bare_trailing_slash() {
    let service = with_catch_all().build(()).expect("a buildable router");

    for path in ["/static", "/static/"] {
        assert_eq!(
            get(&service, path).call().await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

// --- What the handler is told ---------------------------------------------

/// The handler is told what the wildcard captured.
///
/// `route_unchecked` documents the route as "served from the same table as
/// every described one", and this is the half that was not: `install_unchecked`
/// recorded no variables, so `Dispatch`'s capture branch was unreachable and
/// `PathCaptures` was never inserted. A handler had to re-derive the file path
/// from `request.uri().path()` by hand — including the percent-decoding and the
/// `..` rejection `extract/params/path.rs` already implements and keeps private
/// behind `PathCaptures`.
///
/// That is a path-traversal footgun in the feature most likely to meet one.
#[tokio::test]
async fn a_catch_all_hands_its_handler_what_it_captured() {
    async fn echo_capture(request: Request) -> Response {
        let captured = kynos::unchecked::captured(&request, "path")
            .map_or_else(|| "absent".to_owned(), std::borrow::Cow::into_owned);

        Response::new(Body::from_bytes(bytes::Bytes::from(captured)))
    }

    let service = Router::<()>::new()
        .route_unchecked([Method::GET], "/static/{*path}", echo_capture)
        .build(())
        .expect("a buildable router");

    assert_eq!(
        get(&service, "/static/css/app.css").call().await.text(),
        "css/app.css"
    );
}

/// And percent-decodes it, because a capture is a value rather than URL syntax.
#[tokio::test]
async fn a_capture_is_decoded_the_way_a_described_one_is() {
    async fn echo_capture(request: Request) -> Response {
        let captured = kynos::unchecked::captured(&request, "path")
            .map_or_else(|| "absent".to_owned(), std::borrow::Cow::into_owned);

        Response::new(Body::from_bytes(bytes::Bytes::from(captured)))
    }

    let service = Router::<()>::new()
        .route_unchecked([Method::GET], "/static/{*path}", echo_capture)
        .build(())
        .expect("a buildable router");

    assert_eq!(
        get(&service, "/static/a%20b/c%2Ed").call().await.text(),
        "a b/c.d"
    );
}

// --- That the router's own machinery still runs ---------------------------

/// Router-scoped interceptors run over an unchecked route.
///
/// `route_unchecked`'s documentation says so; nothing checked it.
#[tokio::test]
async fn a_router_interceptor_covers_an_unchecked_route() {
    let service = with_catch_all()
        .intercept(RequestId::new())
        .build(())
        .expect("a buildable router");

    let reply = get(&service, "/static/app.css").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.field("x-request-id").is_some(),
        "the router's interceptor did not cover the unchecked route"
    );
}

/// The 401 `Refuse` answers every request with.
#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[error("no credentials")]
#[problem(status = 401)]
struct Unauthenticated;

/// An authentication stand-in that turns every request away.
struct Refuse;

impl Interceptor<()> for Refuse {
    type Reads = ();
    type Adds = ();
    type Short = Unauthenticated;

    async fn intercept(
        &self,
        _: Request,
        (): (),
        (): &(),
        _: Next<'_, ()>,
    ) -> Result<Continued<()>, Unauthenticated> {
        Err(Unauthenticated)
    }
}

/// A nested router's interceptor covers that router's unchecked routes, as it
/// covers its operations, and runs inside the root's own.
///
/// `install_unchecked` used to give every unchecked route the root's
/// interceptors alone, so an authentication interceptor on a nested router
/// left that router's catch-all open.
#[tokio::test]
async fn a_nested_routers_interceptor_covers_its_unchecked_route() {
    let service = Router::<()>::new()
        .nest(
            "/admin",
            Router::<()>::new().intercept(Refuse).route_unchecked(
                [Method::GET],
                "/files/{*path}",
                echo_path,
            ),
        )
        .intercept(RequestId::new())
        .build(())
        .expect("a buildable router");

    let reply = get(&service, "/admin/files/x").call().await;

    assert_eq!(
        reply.status,
        StatusCode::UNAUTHORIZED,
        "the nested router's interceptor did not cover its unchecked route"
    );
    assert!(
        reply.field("x-request-id").is_some(),
        "the root's interceptor did not wrap the nested one"
    );
}

/// A nested router's panic recovery covers that router's unchecked routes.
#[test]
fn a_nested_routers_panic_recovery_covers_its_unchecked_route() {
    async fn explode(_: Request) -> Response {
        panic!("the handler failed");
    }

    let service = Router::<()>::new()
        .nest(
            "/admin",
            Router::<()>::new().catch_panics().route_unchecked(
                [Method::GET],
                "/files/{*path}",
                explode,
            ),
        )
        .build(())
        .expect("a buildable router");

    assert_eq!(
        outcome(&service, "/admin/files/x"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR),
        "a panic in the nested router's unchecked route unwound"
    );
}

/// A nested router holding only unchecked routes still has its interceptors
/// checked for a configuration they cannot honour, now that they cover those
/// routes. Built, this pair echoes any `Origin` with credentials allowed.
#[test]
fn a_nested_routers_unhonourable_cors_over_unchecked_routes_refuses_to_build() {
    let refused = Router::<()>::new()
        .nest(
            "/admin",
            Router::<()>::new()
                .intercept(
                    kynos::middleware::cors::Cors::new()
                        .allow_any_origin()
                        .allow_credentials(),
                )
                .route_unchecked([Method::GET], "/files/{*path}", echo_path),
        )
        .build(());

    assert!(
        matches!(
            refused,
            Err(kynos::Error::Middleware(
                kynos::middleware::MiddlewareError::CredentialedWildcardOrigin
            ))
        ),
        "a nested wildcard origin with credentials over an unchecked route was not refused as one"
    );
}

/// A directory served from a nested router runs that router's interceptors,
/// since `assets_directory` registers its route the way `route_unchecked` does.
#[cfg(feature = "assets-fs")]
#[tokio::test]
async fn a_nested_routers_interceptor_covers_its_served_directory() {
    let service = Router::<()>::new()
        .nest(
            "/admin",
            Router::<()>::new().intercept(Refuse).assets_directory(
                "/files",
                kynos::router::assets::fs::Directory::new("tests/assets"),
            ),
        )
        .build(())
        .expect("a buildable router");

    assert_eq!(
        get(&service, "/admin/files/index.html").call().await.status,
        StatusCode::UNAUTHORIZED,
        "the nested router's interceptor did not cover its served directory"
    );
}

// --- What the document says -----------------------------------------------

/// The route is recorded at the document root, and gets no `paths` key.
#[test]
fn an_unchecked_route_is_recorded_where_a_generator_cannot_act_on_it() {
    let document = with_catch_all().openapi().expect("a describable router");

    assert!(
        document.paths.items.is_empty(),
        "a catch-all took a `paths` key, which is a claim about a path it does not honour"
    );

    let recorded = OpaqueRoute::all(&document).expect("a readable record");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].pattern, "/static/{*path}");
    assert_eq!(recorded[0].prefix.as_deref(), Some("/static"));
    assert_eq!(recorded[0].methods, ["GET"]);
    assert_eq!(recorded[0].reason, OpaqueReason::UntypedRoute);
    assert_eq!(recorded[0].note, None);

    assert!(!document.is_authoritative());
}

/// An upgrade route is recorded the same way, under the reason and the note
/// `upgrade_unchecked` documents, served on `GET` alone.
#[test]
fn an_upgrade_route_is_recorded_as_a_protocol_upgrade() {
    let document = Router::<()>::new()
        .upgrade_unchecked("/rooms/{room}/socket", echo_path)
        .openapi()
        .expect("a describable router");

    assert!(document.paths.items.is_empty());
    let recorded = OpaqueRoute::all(&document).expect("a readable record");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].pattern, "/rooms/{room}/socket");
    assert_eq!(recorded[0].prefix.as_deref(), Some("/rooms"));
    assert_eq!(recorded[0].methods, ["GET"]);
    assert_eq!(recorded[0].reason, OpaqueReason::ProtocolUpgrade);
    assert_eq!(
        recorded[0].note.as_deref(),
        Some("the connection leaves HTTP, which no version of the specification models")
    );
    assert!(!document.is_authoritative());
}

// --- The gate a CI job asserts on ----------------------------------------

/// A router that waives nothing reports no reason.
#[test]
fn a_router_that_waives_nothing_reports_nothing() {
    let router = Router::<()>::new();

    assert!(!router.has_unchecked());
    assert!(router.unchecked_reasons().is_empty());
}

/// A gate can name the one waiver a service takes deliberately.
///
/// `has_unchecked` is the whole-document answer and cannot be anything else,
/// so a service serving a file tree would have to delete its CI assertion for
/// *everything* — which is how a check meant to catch an accidental
/// `layer_unchecked` stops catching one.
#[test]
fn a_gate_can_tolerate_one_waiver_and_still_catch_the_rest() {
    let assets = with_catch_all();

    assert!(assets.has_unchecked());
    assert_eq!(
        assets.unchecked_reasons(),
        [kynos::openapi::OpaqueReason::UntypedRoute]
    );

    // A second, different waiver is visible beside the first rather than
    // hidden behind it.
    let both = with_catch_all().upgrade_unchecked("/ws", echo_path);
    assert_eq!(
        both.unchecked_reasons(),
        [
            kynos::openapi::OpaqueReason::UntypedRoute,
            kynos::openapi::OpaqueReason::ProtocolUpgrade,
        ]
    );
}

/// Two routes waived for one reason report it once.
#[test]
fn one_reason_is_reported_once_however_many_routes_took_it() {
    let router = Router::<()>::new()
        .route_unchecked([Method::GET], "/static/{*path}", echo_path)
        .route_unchecked([Method::GET], "/media/{*path}", echo_path);

    assert_eq!(
        router.unchecked_reasons(),
        [kynos::openapi::OpaqueReason::UntypedRoute]
    );
}

// --- What a layer leaves on an operation -----------------------------------

/// Outside every unchecked layer.
#[kynos::get("/outside")]
async fn outside() -> NoContent {
    NoContent
}

/// Inside the group's unchecked layer.
#[kynos::get("/inside")]
async fn inside() -> NoContent {
    NoContent
}

/// The reasons the operation at `path` is marked opaque for, empty when it is
/// not marked.
fn opaque_reasons(router: &Router<()>, path: &str) -> Vec<OpaqueReason> {
    let document = router.openapi().expect("a describable router");
    let item = document.paths.items.get(path).expect("a described path");
    let (_, operation) = item.operations().next().expect("one operation");

    Opaque::of(operation)
        .expect("a readable marker")
        .map(|marker| marker.reasons)
        .unwrap_or_default()
}

/// A router with `outside` mounted directly and `inside` in a group, with an
/// unchecked layer on the group alone when `on_group`, and on the router alone
/// when `on_router`.
fn layered(on_router: bool, on_group: bool) -> Router<()> {
    let mut group = Group::new("/").mount(kynos::routes![inside]);
    if on_group {
        group = group.layer_unchecked(tower::layer::util::Identity::new());
    }
    let mut router = Router::<()>::new()
        .mount(kynos::routes![outside])
        .group(group);
    if on_router {
        router = router.layer_unchecked(tower::layer::util::Identity::new());
    }
    router
}

/// A group's layer marks the group's operations without any layer on the
/// router, and leaves the router's own operations unmarked.
#[test]
fn a_group_layer_alone_marks_exactly_the_groups_operations() {
    assert_eq!(
        opaque_reasons(&layered(false, true), "/inside"),
        [OpaqueReason::UntypedLayer]
    );
    assert_eq!(opaque_reasons(&layered(false, true), "/outside"), []);
}

/// A router's layer marks every operation beneath it without any layer on the
/// group, including the group's.
#[test]
fn a_router_layer_alone_marks_every_operation() {
    for path in ["/inside", "/outside"] {
        assert_eq!(
            opaque_reasons(&layered(true, false), path),
            [OpaqueReason::UntypedLayer],
            "{path}"
        );
    }
}

/// A `tower` layer that stamps every response it wraps, so a test sees where
/// it ran.
#[derive(Clone, Copy, Debug)]
struct Stamp;

impl<S> tower::Layer<S> for Stamp {
    type Service = Stamped<S>;

    fn layer(&self, inner: S) -> Stamped<S> {
        Stamped(inner)
    }
}

/// What [`Stamp`] wraps a service in.
#[derive(Clone, Debug)]
struct Stamped<S>(S);

impl<S> tower::Service<Request> for Stamped<S>
where
    S: tower::Service<Request, Response = Response, Error = std::convert::Infallible>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(context)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let answered = self.0.call(request);
        Box::pin(async move {
            let mut response = answered.await?;
            response
                .headers_mut()
                .insert("x-stamped", kynos::http::HeaderValue::from_static("yes"));
            Ok(response)
        })
    }
}

/// [`layered`]'s shape with [`Stamp`] in place of the identity layer, built.
fn stamped(on_router: bool, on_group: bool) -> kynos::router::service::Service<()> {
    let mut group = Group::new("/").mount(kynos::routes![inside]);
    if on_group {
        group = group.layer_unchecked(Stamp);
    }
    let mut router = Router::<()>::new()
        .mount(kynos::routes![outside])
        .group(group);
    if on_router {
        router = router.layer_unchecked(Stamp);
    }
    router.build(()).expect("a buildable router")
}

/// A layer runs on exactly the operations it flags: a group's on the group's
/// operation and not its sibling, a router's on both — and neither on a
/// request no route matched, which never reaches a layer.
#[tokio::test]
async fn an_unchecked_layer_runs_on_exactly_the_operations_it_flags() {
    let cases = [
        ("the group's layer", stamped(false, true), [true, false]),
        ("the router's layer", stamped(true, false), [true, true]),
    ];

    for (case, service, [on_inside, on_outside]) in cases {
        for (path, expected) in [("/inside", on_inside), ("/outside", on_outside)] {
            let reply = get(&service, path).call().await;

            assert_eq!(reply.status, StatusCode::NO_CONTENT, "{case}, {path}");
            assert_eq!(
                reply.field("x-stamped").as_deref(),
                expected.then_some("yes"),
                "{case}, {path}"
            );
        }

        let unmatched = get(&service, "/nowhere").call().await;
        assert_eq!(unmatched.status, StatusCode::NOT_FOUND, "{case}");
        assert_eq!(unmatched.field("x-stamped"), None, "{case}");
    }
}

// --- What the match table refuses ------------------------------------------

/// The error-level lines `validate` reports, and the ones `build` refuses
/// with, which have to be the same.
fn refusals(router: impl Fn() -> Router<()>) -> Vec<String> {
    let reported: Vec<String> = router()
        .validate()
        .expect("a describable router")
        .iter()
        .filter(|violation| violation.severity == kynos::openapi::Severity::Error)
        .map(ToString::to_string)
        .collect();

    let refused = match router().build(()) {
        Err(kynos::Error::Invalid { violations }) => violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("built"),
    };

    assert_eq!(reported, refused);
    reported
}

/// A catch-all and a bare variable in the same position leave the match table
/// no rule for which one `/static/app.css` reaches.
#[test]
fn an_unchecked_route_the_matcher_cannot_order_fails_every_entry_point() {
    assert_eq!(
        refusals(|| with_catch_all().route_unchecked([Method::GET], "/static/{name}", echo_path)),
        [
            "error at #: `/static/{name}` conflicts with `/static/{*path}` in the router's match \
          table"
        ]
    );

    // The control: the same two shapes under different literals.
    assert!(
        with_catch_all()
            .route_unchecked([Method::GET], "/media/{name}", echo_path)
            .build(())
            .is_ok()
    );
}

/// A catch-all anywhere but the end is not a pattern the matcher reads.
#[test]
fn an_unchecked_pattern_the_matcher_cannot_read_fails_every_entry_point() {
    assert_eq!(
        refusals(|| Router::<()>::new().route_unchecked(
            [Method::GET],
            "/static/{*path}/raw",
            echo_path
        )),
        ["error at #: `/static/{*path}/raw` is not a pattern the router's match table accepts"]
    );
}

// --- What an unchecked route may not repeat -------------------------------

/// The described operation an unchecked route collides with.
#[kynos::get("/users")]
async fn list_users() -> NoContent {
    NoContent
}

/// An unchecked route repeating a described method and path would never be
/// served, since dispatch takes the first operation for a method, yet the
/// document would list it as opaque.
#[test]
fn an_unchecked_route_repeating_a_described_operation_fails_every_entry_point() {
    assert_eq!(
        refusals(|| Router::<()>::new()
            .mount(kynos::routes![list_users])
            .route_unchecked([Method::GET], "/users", echo_path)),
        [
            "error at #/paths/~1users/get: operation `GET /users` is described more than once; a \
             path item holds one operation per method"
        ]
    );
}

/// Two unchecked routes repeating one method and pattern leave the second
/// unreachable just the same.
#[test]
fn an_unchecked_route_repeating_another_fails_every_entry_point() {
    assert_eq!(
        refusals(|| with_catch_all().route_unchecked(
            [Method::POST, Method::GET],
            "/static/{*path}",
            echo_path
        )),
        [
            "error at #: operation `GET /static/{*path}` is described more than once; a path item \
             holds one operation per method"
        ]
    );
}

/// The control: another method on a described path is served, and the path's
/// `Allow` lists each method once (RFC 9110 §10.2.1). `DELETE` is served
/// elsewhere, so `/users` answers it 405 rather than 501.
#[tokio::test]
async fn an_unchecked_method_beside_a_described_one_is_served() {
    let service = Router::<()>::new()
        .mount(kynos::routes![list_users])
        .route_unchecked([Method::POST], "/users", echo_path)
        .route_unchecked([Method::DELETE], "/elsewhere", echo_path)
        .build(())
        .expect("a buildable router");

    assert_eq!(
        support::post(&service, "/users").call().await.status,
        StatusCode::OK
    );
    assert_eq!(
        support::send(&service, Method::DELETE, "/users")
            .call()
            .await
            .field("allow")
            .as_deref(),
        Some("GET, HEAD, POST")
    );
}

// --- What an upgrade does on the wire ---------------------------------------

/// Answers `101 Switching Protocols`, then echoes every byte the client sends
/// on the connection it took over.
#[cfg(all(feature = "server", feature = "http1"))]
async fn echo_upgrade(mut request: Request) -> Response {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let upgrade = hyper::upgrade::on(&mut request);
    // Spawned, since the upgrade resolves only once the 101 is written. A
    // failed upgrade drops the connection, which the client sees.
    tokio::spawn(async move {
        let Ok(upgraded) = upgrade.await else {
            return;
        };
        let mut io = hyper_util::rt::TokioIo::new(upgraded);
        let mut buffer = [0_u8; 64];
        while let Ok(read @ 1..) = io.read(&mut buffer).await {
            if io.write_all(&buffer[..read]).await.is_err() {
                return;
            }
        }
    });

    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    headers.insert(
        kynos::http::header::CONNECTION,
        kynos::http::HeaderValue::from_static("upgrade"),
    );
    headers.insert(
        kynos::http::header::UPGRADE,
        kynos::http::HeaderValue::from_static("echo"),
    );
    response
}

/// An `upgrade_unchecked` handler gets the connection after its 101.
///
/// Over a real socket, because handing the connection over is the driver's: a
/// driver serving without upgrades writes the 101, then fails the upgrade and
/// closes the connection before a byte of the new protocol moves.
#[cfg(all(feature = "server", feature = "http1"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upgraded_connection_carries_the_new_protocol_after_the_101() {
    use std::{net::Ipv4Addr, time::Duration};

    use kynos::server::{Server, shutdown::Shutdown};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// How long the client waits for the server before the case fails.
    const BOUND: Duration = Duration::from_secs(5);

    let service = Router::<()>::new()
        .upgrade_unchecked("/echo", echo_upgrade)
        .build(())
        .expect("a buildable router");
    let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
    let bound = Server::new(service)
        .bind((Ipv4Addr::LOCALHOST, 0))
        .graceful_shutdown(Shutdown::on(async move {
            let _ = stopped.await;
        }))
        .prepare()
        .await
        .expect("a loopback listener binds");
    let address = bound.local_addrs()[0];
    let serving = tokio::spawn(bound.serve());

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("server accepts");
    stream
        .write_all(
            b"GET /echo HTTP/1.1\r\nHost: localhost\r\nConnection: upgrade\r\nUpgrade: echo\r\n\r\n",
        )
        .await
        .expect("the request head writes");

    // Byte by byte, so nothing past the head is consumed.
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let byte = tokio::time::timeout(BOUND, stream.read_u8())
            .await
            .expect("the response head arrives")
            .expect("the response head reads");
        head.push(byte);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 101"),
        "not a 101: {}",
        String::from_utf8_lossy(&head)
    );

    stream
        .write_all(b"ping")
        .await
        .expect("the upgraded stream writes");
    let mut echoed = [0_u8; 4];
    tokio::time::timeout(BOUND, stream.read_exact(&mut echoed))
        .await
        .expect("the echo arrives")
        .expect("the upgraded stream echoes rather than closes");
    assert_eq!(&echoed, b"ping");

    drop(stream);
    let _ = shutdown.send(());
    serving
        .await
        .expect("server task joins")
        .expect("server exits cleanly");
}
