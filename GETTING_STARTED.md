# Getting started

The [README](README.md) has the install line and a quickstart. This page is
the next hour: where each common task is shown, and the three things people
coming from other frameworks ask first. [`docs/`](docs/README.md) is design
law for contributors, not a guide; the [API reference](https://docs.rs/kynos)
and the examples are.

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
| `Path<(u64,)>`, `Query<T>` | `Path<T>` over `#[derive(PathParams)]`, `Query<T>` over `#[derive(QueryParams)]`; the path's fields are checked against the template at compile time |
| `HeaderMap`, `TypedHeader<T>` | `Headers<T>` over `#[derive(HeaderParams)]` |
| `Json<serde_json::Value>` | a `#[derive(Schema)]` type, or `Unchecked<Value>` where it really is arbitrary |
| `State<T>`, `Extension<T>` | `Inject<T>` from a `#[derive(Provider)]` context, passed to `Router::build` |
| `impl IntoResponse`, `(StatusCode, T)` | a status in the return type: `Created<T>`, `NoContent`, or a `#[derive(Reply)]` enum |
| an error implementing `IntoResponse` | `#[derive(ApiError)]`, answered as an RFC 9457 problem |
| `middleware::from_fn`, `tower::Layer` | an `Interceptor` via `Router::intercept`; see above |
| `Router::nest`, `merge` | `nest` and `merge`, plus `Group`: one resource's operations under one prefix, tag and interceptor stack |
| `/{*path}` wildcards, `fallback` SPA | none; `assets!` for a fixed file set, a reverse proxy otherwise |
| `axum::serve(listener, app)` | `Server::new(router.build(context)?).bind(addr).serve().await` |

Each refusal in that table is argued in the README's
[anti-patterns](README.md#anti-patterns), and an escape hatch is named where one
exists.
