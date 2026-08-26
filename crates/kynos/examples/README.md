# Examples

Every example runs with:

```text
cargo run -p kynos --example <name>
```

The **Features** column is what to append as `--features …` when an example
needs more than the defaults. Each file's own header explains why it needs them.

## Start here

| Example | Shows | Features |
| --- | --- | --- |
| [`hello.rs`](hello.rs) | The whole path from an `async fn` to a served, described operation | — |

## Describing types

| Example | Shows | Features |
| --- | --- | --- |
| [`schema.rs`](schema.rs) | `#[derive(Schema)]`: constraints, serde interop, map keys, and the escape hatch | — |
| [`scalars.rs`](scalars.rs) | Which types outside `std` map to which JSON Schema `format` | `uuid,time-chrono,time-jiff,decimal-rust,decimal-big` |

## Requests

| Example | Shows | Features |
| --- | --- | --- |
| [`parameters.rs`](parameters.rs) | Path, query, header and cookie groups, plus a hand-written extractor | `cookie` |
| [`payloads.rs`](payloads.rs) | Every request body codec, and the three shapes binary content takes | `form,multipart` |
| [`protobuf.rs`](protobuf.rs) | Protocol Buffers as a request and a response body | `protobuf` |

## Responses

| Example | Shows | Features |
| --- | --- | --- |
| [`responses.rs`](responses.rs) | Status in the return type: `Created`, `Accepted`, `Redirect<CODE>`, `Reply`, `WithHeaders`, typed URIs | — |
| [`negotiation.rs`](negotiation.rs) | Choosing a representation from the client's `Accept` header | — |
| [`errors.rs`](errors.rs) | `#[derive(ApiError)]`, RFC 9457 problem documents, and every other way a status reaches an operation | — |
| [`sse.rs`](sse.rs) | Server-Sent Events: discriminated event types, resumption, reconnection advice, keep-alive | `openapi32` |
| [`streaming.rs`](streaming.rs) | JSON Lines, JSON text sequences, byte streams, and the whole-query-string parameter | `openapi32` |

## Structure

| Example | Shows | Features |
| --- | --- | --- |
| [`composition.rs`](composition.rs) | `nest` versus `merge` versus `Group`, the four tag scopes, and the fallback policies | — |
| [`state.rs`](state.rs) | Compile-time dependency injection: `#[derive(Provider)]` and `Inject<T>` | — |
| [`document.rs`](document.rs) | Document metadata, validation, version refusal, and serving the description as a route | `openapi32,yaml` |

## Middleware

| Example | Shows | Features |
| --- | --- | --- |
| [`middleware.rs`](middleware.rs) | Writing an interceptor, and the ones Kynos ships | `compression` |
| [`decompression.rs`](decompression.rs) | Accepting a compressed request body: the direction `Accept-Encoding` says nothing about, and why `BodySize` cannot guard it | `compression` |
| [`cache.rs`](cache.rs) | A shared response cache over a store Kynos does not ship: what may be stored, what may not, and why `Conditional` goes outside | `cache` |
| [`rate_limit.rs`](rate_limit.rs) | Named quotas over a counter store Kynos does not ship: a sliding window, several windows at once, and both header spellings | — |
| [`cors.rs`](cors.rs) | The two exchanges a browser makes, the preflight nothing declares, and the configuration that is refused | — |
| [`tracing.rs`](tracing.rs) | Spans reaching a real subscriber, and why an observer declares nothing | — |
| [`opentelemetry.rs`](opentelemetry.rs) | Distributed tracing that leaves the process: a W3C Trace Context group that is also the propagation carrier, and why this is an example rather than middleware | `trace` |
| [`print_request_response.rs`](print_request_response.rs) | An interceptor buffering both bodies, and what reading them costs the description | — |

## Security

| Example | Shows | Features |
| --- | --- | --- |
| [`security_schemes.rs`](security_schemes.rs) | Every kind of security scheme as a type, from `Auth<S>` to the emitted requirement | — |
| [`jwt.rs`](jwt.rs) | A JWT verifier the framework does not ship: claims as a typed credential, `kid` rotation, scopes, optional auth, and a token endpoint | `json` |
| [`tls.rs`](tls.rs) | Serving over TLS: client certificates, SNI, and the HTTP/1 and HTTP/2 configs ALPN chooses between | `tls` |

## Serving

| Example | Shows | Features |
| --- | --- | --- |
| [`graceful_shutdown.rs`](graceful_shutdown.rs) | Every shutdown trigger, the drain deadline, and reading the bound addresses | — |
| [`auto_reload.rs`](auto_reload.rs) | Inheriting a listening socket so a rebuild does not drop it | — |

## Testing and escape hatches

| Example | Shows | Features |
| --- | --- | --- |
| [`testing.rs`](testing.rs) | Driving the service in-process, and proving the description is truthful | `test-util` |
| [`assets.rs`](assets.rs) | Both ways to serve files: an embedded set that is fully described, and a directory that is recorded instead | `assets-fs` |
| [`unchecked.rs`](unchecked.rs) | The escape hatches, and what using one costs the document | `unchecked` |

## Elsewhere

[`crates/kynos-openapi/examples/standalone.rs`](../../kynos-openapi/examples/standalone.rs)
builds and validates a description with no server anywhere in sight. Run it with
`cargo run -p kynos-openapi --example standalone --features openapi32,yaml`.
