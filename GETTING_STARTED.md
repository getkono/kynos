# Getting started

The [README](README.md) has the install lines — Kynos, plus tokio for
`#[tokio::main]` and serde for its derives, neither of which Kynos re-exports —
and a quickstart that compiles against exactly those. This page is
the next hour: where each common task is shown, and the three things people
coming from other frameworks ask first. [`docs/`](docs/README.md) is design
law for contributors, not a guide; the [API reference](https://docs.rs/kynos)
and the examples are. Its two exceptions are written for applications:
[production](docs/production.md), on what a deployment needs, and
[migrating](docs/migrating.md), on upgrading.

Every example runs with `cargo run -p kynos --example <name>`, plus the
features its [index](crates/kynos/examples/README.md) lists.

## By task

| I want to | Example |
| --- | --- |
| Serve one described operation | [`hello`](crates/kynos/examples/hello.rs) |
| Read path, query, header or cookie parameters | [`parameters`](crates/kynos/examples/parameters.rs) |
| Accept a JSON, form or multipart body | [`payloads`](crates/kynos/examples/payloads.rs) |
| Return a status other than 200, or one of several | [`responses`](crates/kynos/examples/responses.rs) |
| Return errors as RFC 9457 problems | [`errors`](crates/kynos/examples/errors.rs) |
| Give handlers a database pool or other shared state | [`state`](crates/kynos/examples/state.rs) |
| Split routes across modules, prefixes and tags | [`composition`](crates/kynos/examples/composition.rs) |
| Require a credential, and read who the caller is | [`security_schemes`](crates/kynos/examples/security_schemes.rs), [`jwt`](crates/kynos/examples/jwt.rs) |
| Write middleware | [`middleware`](crates/kynos/examples/middleware.rs) |
| Add CORS, rate limiting or caching | [`cors`](crates/kynos/examples/cors.rs), [`rate_limit`](crates/kynos/examples/rate_limit.rs), [`cache`](crates/kynos/examples/cache.rs) |
| Log and trace requests | [`tracing`](crates/kynos/examples/tracing.rs), [`opentelemetry`](crates/kynos/examples/opentelemetry.rs) |
| Serve the OpenAPI document, or a docs UI | [`document`](crates/kynos/examples/document.rs), [`docs_ui`](crates/kynos/examples/docs_ui.rs) |
| Stream a response or send Server-Sent Events | [`streaming`](crates/kynos/examples/streaming.rs), [`sse`](crates/kynos/examples/sse.rs) |
| Serve static files | [`assets`](crates/kynos/examples/assets.rs) |
| Serve over TLS, and shut down gracefully | [`tls`](crates/kynos/examples/tls.rs), [`graceful_shutdown`](crates/kynos/examples/graceful_shutdown.rs) |
| Test the service, and prove its description true | [`testing`](crates/kynos/examples/testing.rs) |

## Before writing it yourself

What Kynos already does that an application is tempted to write. If you are
about to write the left column, the item in the right one does it, and its API
documentation says how. Paths are relative to `kynos`, and an item the prelude
exports is named bare. [Migrating](docs/migrating.md) lists the same features
by the release that shipped each, for code written before it.

| If you are about to write | Reach for |
| --- | --- |
| An `authorization` header formatting a bearer token in a test | `test::TestRequest::bearer` |
| The same header or cookie on every request a test sends | `test::TestClient::with_default_header` or `with_default_cookie` |
| `get`/`post` wrappers prepending a mount prefix and a token minted mid-test | `test::TestClient::at(prefix)` or `view()`, then `bearer(&token)`: a `test::view::ClientView` whose requests the client still records |
| A helper checking a problem response's status, type URI and members | `test::TestResponse::assert_problem`, with the base set once by `test::TestClient::with_problem_base`, then `assert_problem_extension`, or `test::TestProblem::extension` to read one |
| A derived `ApiError`'s type URI typed out again in a test or an interceptor marker | `error::problem::ProblemType::TYPE_URI`, which a struct naming a `type` or `base` implements, or an enum's `type_uri(&self)` |
| `assert!(resp.header(name).is_none())`, or `is_some()` | `test::TestResponse::assert_no_header`, or `assert_has_header` |
| A substring search of a comma-separated field such as `Vary` or `Access-Control-Allow-Headers` | `test::TestResponse::assert_header_lists` |
| `starts_with` on a `Content-Type` to skip its parameters | `test::TestResponse::assert_content_type` |
| A `serde_json::Value` read only to index one member of a response body | `test::TestResponse::json_at`, with an RFC 6901 pointer; behind `json` |
| A parser for a `Set-Cookie` field's attributes | `test::TestResponse::set_cookie`, `assert_cookie_set` and `assert_cookie_cleared`; behind `cookie` |
| A `multipart/form-data` test body framed by hand | `test::TestRequest::multipart`; behind `multipart` |
| A loopback socket to test what a configured server's timeouts answer | `test::TestClient::through_server` |
| A raw socket sending a body that stalls, arrives short or runs long | `test::TestRequest::stalled_body`, or `body_stream` with `content_length` |
| A raw socket sending a request head that never finishes, to test the header-read timeout | `test::TestRequest::send_stalled_head` |
| A oneshot, a `JoinHandle` and an address struct around `serve()` | `server::BoundServer::spawn`, and `test::TestClient::connect` to its `server::Running` |
| Response coverage merged across test clients, or held to one area of the document | `test::Coverage::for_document`, given to each client by `test::TestClient::with_coverage`, then `assert_covered`, `assert_covered_where` and `assert_conformance` |
| The router built a second time in a test, for the document a client serves | `test::TestClient::openapi` |
| A test comparing a committed `openapi.json` with the router's document, and regenerating it behind an environment variable | `test::document::assert_file_matches`, over `openapi::Document::to_canonical_json` |
| A declared response no test can produce, such as a panic's 500, deleted from the document before asserting coverage | `test::Exemptions`, given to `test::Coverage::assert_declared_responses_covered_except` or `assert_covered_except` |
| A 403 deleted from the document before asserting coverage | `#[security(forbids = false)]` on a scheme whose credential grants nothing beyond itself |
| A scope enum's `security::Scope::name` matching on each variant to restate its serde name | `#[derive(kynos::Scope)]` |
| A credential carrying a store failure into every handler | `#[security(unavailable)]` on the scheme, and `error::rejection::AuthRejection::unavailable` from its authenticator, for a 503; for the application's own 500, `type Error` on the `security::Authenticator`, and `?` inside `authenticate` |
| A `code` field and a constructor on every failure | `#[problem(extension(code = CONST))]` |
| A `type` literal restating a `const` base, so two failures share one type URI | `#[problem(slug = "...")]` under that `base` |
| Failure variants copied from one `ApiError` enum into another | `#[problem(transparent, from)]`, `#[problem(subset_of = Wider)]` or `#[problem(variant_of = Wider::Variant)]` |
| A `Location` joined from a mount prefix with `format!` | `extract::link::Link<E>`, rendered into `Created::at` |
| A function returning `Router<App, impl PanicPolicy, impl Sized + use<>, impl Sized + use<>>` | `-> router::sealed::Sealed<App>`, from `Router::seal` |
| Several `routes!` lists, or `routes![op].0.intercept(..)` | one `routes![..]`, with `.intercept(..)` after the member it scopes to |
| `#[tracing::instrument]` spans that carry no request id | `middleware::trace::Trace::span` |
| An interceptor that only counts requests in flight | `middleware::Observer`'s `on_request`, then `on_response` or `on_abandoned` |
| A literal schema bound kept equal to a `const` | `#[schema(maximum = CONST)]`, or any other numeric or count bound |
| `FromStr`, `Display` and `schema::ParamValue` by hand for an enum or a newtype | `#[derive(kynos::ParamValue)]` |
| A hand `impl Schema` describing a type `FromStr` and `Display` carry as one string, such as an identifier wrapping a structured value | `#[derive(Schema, ParamValue)]` and `#[schema(string, ..)]` on the type |
| A query parameter split on `,` by hand | a `Vec<T>` field in `#[derive(QueryParams)]`, with `#[param(explode = false)]` for one comma-separated pair |
| An `Option` query parameter and `.unwrap_or(DEFAULT)` in the handler | `#[param(default = DEFAULT)]` on the plain field, which the description states |
| The same paging fields copied into every query group | one `QueryParams` group, and `#[param(flatten)]` on a field holding it in each |
| A one-field `PathParams` group for a route of one variable | `Path<T>` over the `ParamValue` itself, such as `Path<Uuid>` |
| A rate limiter keyed on the authenticated account, checked in each handler | `middleware::rate_limit::key::ByPrincipal`, behind `guard_first` |
| A 429 of your own for a limit keyed on a body field | `RateLimit::spend` on a `middleware::rate_limit::key::ByHandler` limiter, returning its `RateLimited` through a transparent variant |
| An enum's wire names restated in an `as_str` or an `ALL` list | `#[derive(kynos::ParamValue)]`, then `schema::UnitEnum::as_str` (not `const`) and `T::VARIANTS` with the trait imported, or `s.parse()` |

## Getting data from middleware to a handler

There is no channel for it, deliberately. A handler's arguments are its
declared inputs, and the document is derived from them; a value an interceptor
slipped into the request would be an input nobody described. That is why there
is no `Extension<T>` ([anti-pattern 7](README.md#anti-patterns)). Ask for the
data where it is used instead:

- **Who the caller is.** Not middleware at all. Declare a scheme with
  `#[derive(SecurityScheme)]`, verify it with an `Authenticator` your context
  provides, and take `Auth<S>` in the handler: it is the verified credential, as
  your own type, and the operation's security requirement in one. `MaybeAuth<S>`
  makes it optional. [`jwt`](crates/kynos/examples/jwt.rs) turns a bearer token
  into typed claims; [`security_schemes`](crates/kynos/examples/security_schemes.rs)
  shows every scheme kind.
- **A request header an interceptor also reads.** Take `Headers<T>` in the
  handler over the same `#[derive(HeaderParams)]` group. Both declare it, and
  the document lists it once.
- **A service such as a pool or a client.** `Inject<T>` from the context, which
  fails to compile when the context does not provide it.
  [`state`](crates/kynos/examples/state.rs).
- **Anything else derived from the request.** A hand-written
  `FromRequestParts` extractor that describes what it reads.
  [`parameters`](crates/kynos/examples/parameters.rs) writes one.

## What a tower layer costs

The cost that matters is to the description. A `tower::Layer` can answer with any status and set
any header without its type saying so, so Kynos cannot document what it does.
Mounting one takes the `unchecked` feature and `Router::layer_unchecked`, and
then:

- every operation beneath it is marked `x-kynos-opaque`, and the document is
  stamped `x-kynos-document-not-authoritative`;
- `Router::has_unchecked` turns true, which is the line a CI check should
  assert against;
- the interceptor conflict check cannot see inside it.

Write an [`Interceptor`](crates/kynos/examples/middleware.rs) instead: its
`Short`, `Adds` and `Reads` types declare the responses, response headers and
request headers it contributes, and every covered operation documents them.
A proxy in front of the server is outside the document entirely; converting the
built service into a tower `Service` with `into_tower_unchecked` marks every
operation, since nothing knows what will wrap it. [`unchecked`](crates/kynos/examples/unchecked.rs)
shows all of it; [`docs/middleware.md`](docs/middleware.md#tower-interop) has
the reasoning.

## Coming from axum

The shapes are close; what moves is that each one must describe itself.

| axum | Kynos |
| --- | --- |
| `Router::new().route("/users/{id}", get(h))` | `#[kynos::get("/users/{id}")]` on `h`, then `Router::<App>::new().mount(kynos::routes![h])` |
| `Path<u64>`, `Path<(u64, u64)>`, `Query<T>` | `Path<u64>` on a route of one variable; `Path<T>` over `#[derive(PathParams)]` for several, `Query<T>` over `#[derive(QueryParams)]`; the path's variables are checked against the template at compile time |
| `HeaderMap`, `TypedHeader<T>` | `Headers<T>` over `#[derive(HeaderParams)]` |
| `Json<serde_json::Value>` | a `#[derive(Schema)]` type, or `Unchecked<Value>` where it really is arbitrary |
| `State<T>`, `Extension<T>` | `Inject<T>` from a `#[derive(Provider)]` context, passed to `Router::build` |
| `impl IntoResponse`, `(StatusCode, T)` | a status in the return type: `Created<T>`, `NoContent`, or a `#[derive(Reply)]` enum |
| an error implementing `IntoResponse` | `#[derive(ApiError)]`, answered as an RFC 9457 problem |
| a hand-written `From` into each error enum for a shared failure | a `#[problem(transparent)]` variant holding the shared `ApiError`, with `#[from]` |
| `middleware::from_fn`, `tower::Layer` | an `Interceptor` via `Router::intercept`; see above |
| `Router::nest`, `merge` | `nest` and `merge`, plus `Group`: one resource's operations under one prefix, tag and interceptor stack |
| `/{*path}` wildcards, `fallback` SPA | none; `assets!` for a fixed file set, a reverse proxy otherwise |
| `axum::serve(listener, app)` | `Server::new(router.build(context)?).bind(addr).serve().await` |

Each refusal in that table is argued in the README's
[anti-patterns](README.md#anti-patterns), and an escape hatch is named where one
exists.
