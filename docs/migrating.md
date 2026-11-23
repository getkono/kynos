# Migrating

What each release makes unnecessary downstream. An upgrade fixes what stops
compiling, because the compiler points at it; the
[changelog](../crates/kynos/CHANGELOG.md) lists those breaks. Nothing points at
a workaround that still compiles, so it survives the upgrade beside the API
that replaced it. This guide lists those.

Each row reads: if you wrote the left column, write the middle one instead.
The [issue](https://github.com/getkono/kynos/issues) in the right column
carries the motivating example. Paths are
relative to `kynos`, and an item `kynos::prelude` exports is named bare.

This is a guide, not a contract: nothing here is normative.

## Keeping it current

A feature request's optional **Replaces** field names the downstream code the
feature makes unnecessary. When that issue ships, its answer becomes a row in
the section for the release that ships it.

## 0.6 to 0.7

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| `FromStr`, `Display` and `schema::ParamValue` written by hand for a newtype, each delegating to its field | `#[derive(kynos::ParamValue)]` on the newtype | #1378 |
| `#[derive(Schema, QueryParams)]`, or `Schema` beside another parameter derive only so the group's `#[schema(...)]` bounds compile | the parameter derive alone, which registers and checks `#[schema(...)]` itself | #1378 |
| `#[error(transparent)]` and `#[problem(transparent)]` on one `ApiError` variant | `#[error(transparent)]` alone; a `#[problem(status = ...)]` beside it opts out | #1378 |
| A variant copied from another `ApiError` enum, and a `match` or `let ... else { unreachable!() }` moving a value across | `#[problem(variant_of = Other::Variant)]` on the variant, and `Narrow::try_from(other)` | #1378 |
| `TcpListener::bind`, `tokio::spawn(server.listener(..).serve())`, a hand-formatted request head and a status-line parser, to test what only a listener answers | `test::TestClient::connect(&running)` over `server::BoundServer::spawn`'s handle, then `running.shutdown()` | #1389 |
| A `test::Coverage` newtype behind an `Arc`, whose only method calls `assert_declared_responses_covered(&my_document())` | `test::Coverage::for_document(my_document())`, a `.clone()` of it to each harness, and `assert_covered()` once | #1388 |
| A `middleware::rate_limit::decision::RateLimitPolicy` written by hand only so a test can move the time its windows count against | `middleware::rate_limit::quota::Quotas::new(key, store).with_clock(clock)`, where `clock` implements `middleware::rate_limit::clock::Clock` | #1392 |
| `http::Method::from_bytes(name.as_bytes()).unwrap()` before `test::TestClient::method(..)`, for a table of cases naming each method as text | `client.method("POST", path)`: `method` takes any `test::IntoMethod`, a `Method`, a `&Method` or a name | #1391 |
| `assert_eq!(resp.status().as_u16(), 401, "a revoked token is dead")`, which drops the body from the failure | `resp.assert_status_because(401, "a revoked token is dead")` | #1391 |
| An `assert_problem(resp, status, slug)` wrapper formatting `BASE + slug` | `test::TestClient::new(service).with_problem_base(BASE)` once, then `resp.assert_problem(403, "insufficient-scope")` | #1391 |
| `.peer(..)` on every request so a test reads one client whether built `test::TestClient::new` or `through_server` | nothing: both send from `127.0.0.1:0`; `with_peer` once where the address itself matters | #1391 |
| A `#[kynos::head]` twin whose body calls the same function as the `#[kynos::get]`, only so the document describes the HEAD | `#[kynos::get(.., head)]` on the GET alone, or `head = "..."` beside an explicit `operation_id`; a twin left beside it is refused, `HeadDescribedTwice` | #1394 |

## 0.5 to 0.6

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| A coverage record of your own, with its own template matcher, for responses no `test::TestClient` received | `test::Coverage::record` | #1152 |
| A loopback socket, a hand-written request head and a status-line parser, to test what a configured server's timeouts answer | `test::TestClient::through_server`, which sends each request over `server::Server::serve_io` | #1152 |
| The `uuid`, `time-chrono`, `time-jiff`, `decimal-rust` or `decimal-big` feature | `uuid1`, `time-chrono04`, `time-jiff02`, `decimal-rust1` or `decimal-big04`: each names the crate major its impls are for, so the next major arrives beside it | #1214 |
| `#[derive(prost::Message)]` alone on a `Protobuf<T>` message, or a `prost` pinned to the version Kynos compiled | `#[derive(prost::Message, ProtobufMessage)]`, or `type_attribute(".", "#[derive(kynos::ProtobufMessage)]")` in `prost-build`: `extract::body::protobuf::Message` is Kynos's, and the `prost` it calls is your own: any version whose `Message` has `decode(impl Buf)` over `bytes` 1 and `encode_to_vec`, named `::prost` or by the type's `#[prost(prost_path = "...")]` | #1215 |

## 0.4 to 0.5

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| Presenting, authenticating and authorizing a credential by hand, then patching the 403's challenge | `security::requirement::ScopeCheckedBy::check_scoped`, which authorizes the scheme's default scopes and the demanded ones in one call | #1000 |
| `extract::body::binary::Binary<M>` to carry a text body under a `text/*` media type | `extract::body::text_as::TextAs<M>`, which checks the body is UTF-8 | #987 |
| A hand-built hyper accept loop around your own transport, buffering each body before routing | `server::Server::serve_io`, which serves any `AsyncRead + AsyncWrite` connection, streams bodies and speaks HTTP/2 | #580 |

## 0.3 to 0.4

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| Patching `externalDocs` into the document after the router emits it | `Router::external_docs` | #472 |
| A mount prefix of your own joined into a `Location` with `format!` | the `extract::link::Link<E>` extractor: `Created::at(link.uri(params), body)` renders the target with every `Group` and `nest` prefix applied | #477 |
| The same literal in every `#[problem(base = "https://…")]` | a shared `const`: `#[problem(base = crate::PROBLEM_BASE)]` | #458 |
| `FromStr`, `Display` and `schema::ParamValue` written by hand for a field-less enum | `#[derive(kynos::ParamValue)]`, reading and writing the names serde does | #464 |
| A macro re-declaring shared failure variants in every `ApiError` enum | one variant, `#[problem(transparent)] Shared(#[from] SharedError)` | #459 |
| `WithHeaders<E, H>` with an `Option` header that only one failure sets | `#[problem(headers)]` on that failure's one header-group field | #460 |
| A random request-id source of your own | `middleware::request_id::Random`, given to `middleware::request_id::RequestId::source` | #476 |
| An interceptor sending `X-Content-Type-Options: nosniff` and framing denial | `middleware::security_headers::SecurityHeaders`, with `.deny_framing()` | #475 |
| A hand-built schema for a base64 string field | `#[schema(content_encoding = "base64", content_media_type = "…")]` on the field | #463 |
| Response coverage and template matching merged across several test clients by hand | one `test::Coverage`, shared through `test::TestClient::with_coverage(&coverage)` | #479 |
| A raw TCP harness to send a stalled, partial or oversized body | `test::TestRequest::body_stream` with `content_length` | #479 |
| Matching `server::error::ServerError::ShutdownTimeout` to tell a requested shutdown from a failure | `server::error::ServerError::is_requested_shutdown` | #478 |
