# Kynos

> Status: Not for public use yet! Feel free to star. Also we have had enough AI-generated PRs in various repos but contributions with human-oversight would be welcomed.

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

Kynos only lets you build APIs it can fully describe. Every handler input describes itself as a Parameter or Request Body, every handler output describes itself as a Responses Object, and every interceptor declares what it contributes. Anything undescribable does not compile.

The emitted document is therefore not documentation that drifts from the code. It is a checked contract derived from the same types the server runs on.

## Features

- [ ] OpenAPI 3.1 document model, with 3.2 as an opt-in strict superset
- [ ] Structural validation: `operationId` uniqueness, path-template correspondence, parameter uniqueness, closed style/location table
- [ ] Typed extraction for path, query, header, cookie, JSON, form and multipart
- [ ] Extractor rejections documented automatically
- [ ] Type-level status codes and response headers
- [ ] RFC 9457 problem details for both framework and application errors
- [ ] Compile-time-resolved dependency injection
- [ ] `Interceptor` and `Observer` middleware with declared contributions
- [ ] Security schemes that cannot be enforced without being documented
- [ ] Server-Sent Events and streaming bodies, under `openapi32`
- [ ] In-process test client with contract-conformance assertions

Permanently out-of-scope

- Support for OpenAPI 3.0 and older: OpenAPI 3.0.x is vastly different from OpenAPI 3.1+ and has technically been superseded for many years.
- WebSockets: OpenAPI describes HTTP request/response semantics. A socket that stops being either belongs to AsyncAPI, and Kynos would rather point at it than pretend.
- Templating and HTML rendering: Kynos is a REST framework, not a web framework.

## Anti-patterns

Each of these is something another Rust framework offers and Kynos does not, and in every case the reason is the same: it would put a claim in the description that the running service does not honour. Where an escape hatch exists it is named.

**1. Arbitrary middleware.** A `tower::Layer` can change the status, rewrite the body, add headers or refuse the request, and its type says nothing about which. Wrapping an operation in one silently invalidates its description. Write an `Interceptor` and declare an `OperationContribution` instead — it is barely more work, and in exchange every covered operation documents the effect automatically. Escape hatch: `layer_unchecked`, behind `unchecked`.

**2. Raw request access.** There is no `Request`, `Body` or `HeaderMap` extractor. These are exactly the holes through which aide and utoipa emit documents with silent gaps. Declare what you read with `Headers<T>`; if a body genuinely is arbitrary, say `Unchecked<T>`.

**3. Wildcard and catch-all routes.** A path parameter value must not contain an unescaped `/`, so `/assets/{*path}` has no OpenAPI equivalent. Serving a directory tree and SPA fallback follow from this. Use a reverse proxy or CDN. Escape hatch: `route_unchecked`, behind `unchecked`.

**4. Runtime-chosen status codes.** `HttpResponse::build(code)` and a bare `StatusCode` return have no equivalent here. A status the description does not list is a status it is wrong about. Status is part of the return type; use `#[derive(Reply)]` when an operation has several.

**5. `Accept`, `Content-Type` or `Authorization` as header parameters.** The specification says a parameter definition for these *shall be ignored*, so declaring one is a claim no consumer will honour. `#[derive(Headers)]` rejects them at compile time and names the right tool: content negotiation for the first two, `#[derive(SecurityScheme)]` for the third.

**6. `serde_json::Value` bodies.** No `Schema` implementation, so `Json<Value>` does not compile. A payload that really is unconstrained must say so in the type — `Unchecked<Value>` — which is annotated in the document and reported by `validate`. Weakness is allowed; *silent* weakness is not. The same rule removes `usize` (maps to `int32` or `int64` depending on the build target), `SystemTime` (serde emits a seconds/nanos struct) and `Box<dyn Trait>`.

**7. Erased state maps.** `Extension<T>` and salvo's `Depot` turn a missing dependency into a runtime panic. `Inject<T>` makes it a compile error.

**8. Request-derived values as dependencies.** A `CurrentUser` read from an `Authorization` header is not application state; injecting it would make the requirement invisible in the description. It arrives through `Auth<S>`, so enforcing a credential and declaring it are one act.

**9. Header-based API versioning.** OpenAPI expresses paths. Put the version in the path.

**10. Per-route trailing-slash or case normalization.** One app-level policy, or none. Paths in a description are exact.

**11. Hand-writing or patching the emitted document.** `Router::openapi()` is the only supported path from code to description. If it cannot express something, that is a bug worth reporting rather than routing around.

## Feature flags

`openapi31` is the baseline and is enabled by default. `openapi32` is a strict superset — enabling it is purely additive for a program that uses no 3.2-only construct.

3.2-only fields are `#[cfg]`-gated rather than runtime-optional, so a 3.1-only build cannot construct a description it is unable to emit. That is also why `Sse`, `JsonLines` and `QueryString` require `openapi32`: OpenAPI 3.1 has no `itemSchema` and no `in: querystring`, so under 3.1 those payloads can only be described as opaque strings. Kynos would rather not compile than describe your stream inaccurately.

| Flag | Default | What it adds |
| --- | --- | --- |
| `openapi31` | yes | The OpenAPI 3.1 object model. Baseline. |
| `openapi32` | no | The 3.2 superset: `itemSchema`, `in: querystring`, `QUERY`, hierarchical tags, `$self`, device authorization |
| `macros` | yes | Route attributes and derives |
| `server` | yes | The `tokio`/`hyper` server |
| `http1`, `http2` | yes | Protocol versions |
| `json` | yes | Application JSON request and response codecs |
| `trace` | yes | Per-operation `tracing` spans. Facade only; the subscriber stays yours |
| `tls` | no | `rustls`, including client-certificate verification |
| `form`, `multipart`, `cookie` | no | Additional codecs |
| `compression` | no | Response compression |
| `yaml` | no | YAML document emission |
| `test-util` | no | In-process test client and contract-conformance assertions |
| `unchecked` | no | Escape hatches. Marks the emitted document non-authoritative |

Disabling `json` removes application JSON payload types and helpers, including
the OpenAPI 3.2 JSON stream responses. OpenAPI document serialization and the
framework's RFC 9457 problem responses remain JSON-based core behavior.

HTTP/3 is not implemented and there is currently no `http3` feature. QUIC and
HTTP/3 support are on the roadmap, with prioritization based on demonstrated
user demand.

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
We plan to release our benchmarks in this repo: <https://github.com/getkono/kynos-bench>
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
