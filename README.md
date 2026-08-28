# Kynos

> Status: Not for public use yet! Feel free to star. Also we have had enough AI-generated PRs in various repos but contributions with human-oversight would be welcomed.

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

Kynos only lets you build APIs it can fully describe. Every handler input describes itself as a Parameter or Request Body, every handler output describes itself as a Responses Object, and every interceptor declares what it contributes. Anything undescribable does not compile.

The emitted document is therefore not documentation that drifts from the code. It is a checked contract derived from the same types the server runs on.

## Features

- [x] OpenAPI 3.1 document model, with 3.2 as an opt-in strict superset
- [x] Structural validation: `operationId` uniqueness, path-template correspondence, parameter uniqueness, closed style/location table
- [x] Typed extraction for path, query, header, cookie, JSON, form and multipart
- [x] Extractor rejections documented automatically
- [x] Type-level status codes and response headers
- [x] RFC 9457 problem details for both framework and application errors
- [x] Compile-time-resolved dependency injection
- [x] `Interceptor` and `Observer` middleware with declared contributions
- [x] Security schemes that cannot be enforced without being documented
- [x] Server-Sent Events and streaming bodies, under `openapi32`
- [x] In-process test client with contract-conformance assertions

Permanently out-of-scope

- Support for OpenAPI 3.0 and older: OpenAPI 3.0.x is vastly different from OpenAPI 3.1+ and has technically been superseded for many years.
- WebSockets: OpenAPI describes HTTP request/response semantics. A socket that stops being either belongs to AsyncAPI, and Kynos would rather point at it than pretend.
- Templating and HTML rendering: Kynos is a REST framework, not a web framework.
- Runtime abstraction: Kynos is tokio-only. There is no executor generic and no feature flag selecting another runtime, because no trait spans readiness-based and completion-based I/O without paying for a copy. See [`docs/architecture.md`](docs/architecture.md#runtime-policy).

## Anti-patterns

Each of these is something another Rust framework offers and Kynos does not, and in every case the reason is the same: it would put a claim in the description that the running service does not honour. Where an escape hatch exists it is named.

**1. Arbitrary middleware.** A `tower::Layer` can change the status, rewrite the body, add headers or refuse the request, and its type says nothing about which. Wrapping an operation in one silently invalidates its description. Write an `Interceptor` instead, whose signature *is* the declaration: the responses it can answer with, the headers it adds and the headers it reads are three associated types, so it cannot say one thing and do another. Every covered operation documents the effect automatically. Escape hatch: `layer_unchecked`, behind `unchecked`.

**2. Raw request access.** There is no `Request`, `Body` or `HeaderMap` extractor. These are exactly the holes through which aide and utoipa emit documents with silent gaps. Declare what you read with `Headers<T>`; if a body genuinely is arbitrary, say `Unchecked<T>`.

**3. Wildcard and catch-all routes.** A path parameter value must not contain an unescaped `/`, so `/assets/{*path}` has no OpenAPI equivalent. SPA fallback follows from this. Use a reverse proxy or CDN. Escape hatch: `route_unchecked`, behind `unchecked`. *Static files are the case worth separating*: a set whose membership is fixed is enumerable, so `assets!` compiles it into the binary and every file becomes a literal `paths` key with nothing waived. Only a directory that anything may add to needs the hatch — `assets_directory`, which records itself at the document root where no generator can act on it.

**4. Runtime-chosen status codes.** `HttpResponse::build(code)` and a bare `StatusCode` return have no equivalent here. A status the description does not list is a status it is wrong about. Status is part of the return type; use `#[derive(Reply)]` when an operation has several.

**5. `Accept`, `Content-Type` or `Authorization` as header parameters.** The specification says a parameter definition for these *shall be ignored*, so declaring one is a claim no consumer will honour. `#[derive(HeaderParams)]` rejects them at compile time and names the right tool: content negotiation for the first two, `#[derive(SecurityScheme)]` for the third.

**6. `serde_json::Value` bodies.** No `Schema` implementation, so `Json<Value>` does not compile. A payload that really is unconstrained must say so in the type — `Unchecked<Value>` — which is annotated in the document and reported by `validate`. Weakness is allowed; *silent* weakness is not. The same rule removes `usize` (maps to `int32` or `int64` depending on the build target), `SystemTime` (serde emits a seconds/nanos struct) and `Box<dyn Trait>`.

**7. Erased state maps.** `Extension<T>` and salvo's `Depot` turn a missing dependency into a runtime panic. `Inject<T>` makes it a compile error.

**8. Request-derived values as dependencies.** A `CurrentUser` read from an `Authorization` header is not application state; injecting it would make the requirement invisible in the description. It arrives through `Auth<S>`, so enforcing a credential and declaring it are one act.

**9. Header-based API versioning.** OpenAPI expresses paths. Put the version in the path. This is the one item here with no mechanical enforcement — a version header declared with `#[derive(HeaderParams)]` compiles — so it is advice rather than a rule the compiler keeps.

**10. Per-route trailing-slash or case normalization.** One app-level policy, or none. Paths in a description are exact.

**11. Hand-writing or patching the emitted document.** `Router::openapi()` is the only supported path from code to description. If it cannot express something, that is a bug worth reporting rather than routing around.

## Feature flags

`openapi31` is the baseline and is enabled by default. `openapi32` is a strict superset — enabling it is additive for a program that uses no 3.2-only construct. Because Cargo unifies features across a dependency graph, any crate in the build can turn it on for every crate, so the model types it extends are `#[non_exhaustive]`: matching one takes a wildcard arm in either build. The one thing it does not survive is a struct literal naming every field of a model type, which is why the model's own examples end theirs with `..Default::default()`.

3.2-only fields are `#[cfg]`-gated rather than runtime-optional, so a 3.1-only build cannot construct a description it is unable to emit. That is also why `Sse`, `JsonLines` and `QueryString` require `openapi32`: OpenAPI 3.1 has no `itemSchema` and no `in: querystring`, so under 3.1 those payloads can only be described as opaque strings. Kynos would rather not compile than describe your stream inaccurately.

| Flag | Default | What it adds |
| --- | --- | --- |
| `openapi31` | yes | The OpenAPI 3.1 object model. Baseline. |
| `openapi32` | no | The 3.2 superset: `itemSchema`, `in: querystring`, `QUERY`, hierarchical tags, `$self`, device authorization |
| `macros` | yes | Route attributes and derives |
| `server` | yes | The `tokio`/`hyper` server. tokio is the only supported runtime |
| `http1`, `http2` | yes | Protocol versions |
| `json` | yes | Application JSON request and response codecs |
| `trace` | yes | Two `tracing` events per request, keyed by operation. Facade only; the subscriber stays yours |
| `tls` | no | `rustls`, including client-certificate verification |
| `form`, `multipart`, `protobuf` | no | Additional request and response codecs |
| `cookie` | no | Cookie parameters, response cookies and the `SetCookies` interceptor. Kynos owns the RFC 6265 this needs, so it pulls in no dependency |
| `uuid` | no | `Uuid` as `format: uuid` |
| `time-chrono`, `time-jiff` | no | Dates and times from one backend or the other, both mapping onto shapes `time` defines once |
| `decimal-rust`, `decimal-big` | no | Decimals, written as JSON strings so the precision they exist for survives |
| `compression` | no | Response compression, and decompressing a request body the client compressed |
| `yaml` | no | YAML document emission |
| `test-util` | no | In-process test client and contract-conformance assertions |
| `assets` | no | Compile a directory into the binary as one described operation per file |
| `cache` | no | A shared response cache over a store you supply, and the conditional-request half |
| `docs` | no | `Router::docs`: a Scalar or Redoc reference and this API's own description, mounted as two described operations. Ships the wiring and the pages, no UI and no dependency |
| `assets-fs` | no | Serve a directory from disk. Implies `unchecked`: its membership is not fixed, so no path template is true of it |
| `unchecked` | no | Escape hatches. What they reach is recorded and flagged rather than dropped, and the document is stamped non-authoritative |
| `full` | no | Every feature above except `unchecked` and `assets-fs`, which implies it. A convenience for testing the whole surface, not a recommended default |

`time` and `decimal` are umbrellas rather than flags to enable: each defines the shape both of its backends map onto, so `date-time-local` is settled in one place and cannot change with a flag. Enabling one on its own is a `compile_error!` naming the backends, because an umbrella with no backend describes nothing.

Disabling `json` removes application JSON payload types and helpers, including
the OpenAPI 3.2 JSON stream responses. OpenAPI document serialization and the
framework's RFC 9457 problem responses remain JSON-based core behavior.

HTTP/3 is not implemented and there is currently no `http3` feature. QUIC and
HTTP/3 support are on the roadmap, with prioritization based on demonstrated
user demand.

## What this release freezes

The core is what every operation passes through, so it freezes with the release: a change there is a breaking change. Everything else is additive — it composes onto the core, and the core names none of it. That is what makes each row separable, and it is why the rows furthest out settle last: a part that only ever sits at the edge of a stack is the one you will have exercised least, so it gets the most time to be argued with before it is fixed.

`frozen` commits to the surface. `settling` means the shape is right and the details may still move. `open` means expect the surface to move — use it, and say where it is wrong.

| Part | Flag | Freezes |
| --- | --- | --- |
| Document model and validation | `openapi31`, `openapi32` | frozen |
| Schema, and the scalar formats | `uuid`, `time-*`, `decimal-*` | frozen |
| Route attributes and derives | `macros` | frozen |
| Handlers, extraction, responses | `json` | frozen |
| Routing, groups, path templates | — | frozen |
| Errors and RFC 9457 problems | — | frozen |
| Dependency injection | — | frozen |
| Security schemes | — | frozen |
| `Interceptor`, `Observer`, and the contribution check | — | frozen |
| Server, graceful shutdown, TLS | `server`, `http1`, `http2`, `tls` | frozen |
| Codecs beyond JSON | `form`, `multipart`, `protobuf` | settling |
| Cookies, request and response | `cookie` | settling |
| CORS | — | settling |
| Limits: body size, timeout, concurrency | — | settling |
| Correlation identifiers | — | settling |
| Cross-site request forgery | — | open |
| Panic policy | — | settling |
| Request tracing | `trace` | settling |
| YAML emission | `yaml` | settling |
| Server-Sent Events and streaming bodies | `openapi32` | settling |
| Rate limiting | — | open |
| Compression, and request decompression | `compression` | open |
| Response cache and conditional requests | `cache` | open |
| Static assets, embedded and from disk | `assets`, `assets-fs` | open |
| In-process test client | `test-util` | open |
| Escape hatches | `unchecked` | open |

Note what the middle column does *not* say: most of what settles last is not behind a flag at all. Rate limiting, CORS and the limits ship with the default build, so `settling` and `open` are statements about the API rather than about what you are compiling.

The `open` rows are young surfaces rather than filed defects. Five once carried one apiece and all five are closed: compression now mints a validator per content coding and serves a ranged response, the test client reaches methods, cookies, bodies and ranged responses, and the corpus a downstream generator is built against is published. Rate limiting is still not behind a flag of its own, and that is now a decision rather than a gap — [`docs/middleware.md`](docs/middleware.md) records why a flag whose off-state removes no dependency and no cost is a build configuration to get wrong.

The cache and rate-limit rows ship a *seam* rather than a store. Prescribing one would mean prescribing a dependency, which is the line this project draws elsewhere too; both name a working implementation in [`crates/kynos/examples/`](crates/kynos/examples/).

This table is about the API. Whether a guarantee is *enforced* is a different question with a different answer, kept in [`docs/nfr.md`](docs/nfr.md) — a row can be `frozen` here and still owe a test there.

## Development

- Prerequisites: rustup, [mise](https://mise.jdx.dev/)

- Install dependencies:
```bash
mise install
mise exec -- hk install --mise
mise run check
```

*See <mise.toml> for scripts.*

## FAQ

**Is kynos monolithic?**
No, it is minimal but strict and opinionated where correctness is in question.

**Is kynos really faster than the competition?**
A better way to frame it is we are as fast if not faster in most ways (open an issue if not) and spend effort on optimizations possible due to our API strictness since day-one.
The concrete claim: there is no JSON Schema interpreter on the hot path. Constraints are declared once on the type, and the emitted document and the request parser are two projections of that one declaration.
We plan to release our benchmarks in this repo: <https://github.com/getkono/kynos-bench>, which also defines the measurement methodology — what is measured, how, and why each number is worth tracking.
<!-- TODO: establish this claim. -->

**Why code-first and not contract-first?**
There may be more than one answer but most consumers already expect contracts for REST APIs (usually in the form of OpenAPI specs). Building and extending code that define OpenAPI specs has less friction and enables performance optimizations not possible with an OpenAPI spec that constrains code.

**Why can't I just use a tower layer?**
You can, behind the `unchecked` feature, and the emitted document will say it is no longer authoritative. The default answer is an `Interceptor`, which is strictly more useful: it declares what it does to the exchange, and that declaration propagates into every operation it covers. With tower, that mapping is maintained by hand and drifts.

**How is kynos related to spargen?**
[spargen](https://github.com/getkono/spargen) is a related project by the same people but completely separate internals. kynos is for the server and spargen is for the client.

## MSRV

We conservatively bump as new language features come about.

Rust 1.85

## License

MIT — see [LICENSE](LICENSE) for details.
