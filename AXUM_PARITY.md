# Axum family parity

This ledger compares Kynos with the latest published axum family audited for
the API-skeleton milestone:

- [`axum` 0.8.9](https://docs.rs/axum/0.8.9/axum/)
- [`axum-extra` 0.12.6](https://docs.rs/axum-extra/0.12.6/axum_extra/)
- [`axum-macros` 0.5.1](https://docs.rs/axum-macros/0.5.1/axum_macros/)

The published Cargo packages and their rustdoc surface are the source of
truth. “Equivalent” means the use case is present, not that Kynos copies the
same spelling. “Superset” means Kynos additionally carries the information
needed to keep the OpenAPI contract authoritative. “Excluded” is intentional,
not backlog. `openapi32` and other gates are stated where relevant.

## Axum 0.8.9

| Axum primitive | Kynos equivalent | Status and boundary |
| --- | --- | --- |
| `body::Bytes`, `Body`, `BodyDataStream`, `HttpBody` | `Binary<M>`, `BinaryStream<S, M>`, internal `http::Body` | Superset for handler-visible data because the media type is declared. Raw bodies and body traits are not handler extractors. Streaming responses require `openapi32`. |
| `Json<T>`, `Form<T>` | `Json<T>`, `Form<T>` | Superset: request and response schemas, media types, and rejections are coupled. `json` / `form`. |
| `Extension<T>`, `extract::State<T>`, `FromRef` | `Inject<T>`, `FromContext`, `Provides<T>`, `#[derive(Provider)]` | Superset: application dependencies are compile-time associations. Erased extension maps are excluded. |
| `extract::FromRequest`, `FromRequestParts`, optional extractor traits, `RequestExt`, `RequestPartsExt` | `FromRequest`, `FromRequestParts`, `Describe`, `RequestContent` | Superset: request-derived inputs must also describe themselves. `Option<T>` is implemented only for request bodies; optional path/header extraction would make required OpenAPI parameters ambiguous. |
| `extract::Request`, whole `HeaderMap` | `Headers<T>`, typed body extractors | Excluded from authoritative handlers. `Unchecked<T>` is the explicit weak-schema escape hatch; unchecked routes are feature-gated separately. |
| `DefaultBodyLimit` | `limits::BodySize` | Superset: the 413 response is contributed automatically. |
| `Path<T>`, `RawPathParams` | `Path<T>`, `PathParams` | Superset for declared captures, including compile-time template/name matching and typed URI encoding. Raw capture iteration is excluded. |
| `Query<T>`, `RawQuery` | `Query<T>`; `QueryString<T, M>` under `openapi32` | Superset: named parameters have schemas; whole query strings use OpenAPI 3.2 `in: querystring`. Undescribed raw strings are excluded. |
| `RawForm` | `Form<T>` | Equivalent typed use case. Raw form bytes use `Binary<M>` only when an application deliberately declares that media contract. |
| `ConnectInfo<T>`, `Connected<T>`, `MockConnectInfo<T>` | `ConnectInfo`, server listener integration, test utilities | Equivalent for supported TCP peer addresses. Custom transport-derived connection metadata is outside the sealed server transport contract. |
| `MatchedPath` | `MatchedPath` | Equivalent. Both expose the stable route template rather than the concrete URI. |
| `NestedPath`, `OriginalUri` | none | Excluded: original and nested runtime URIs are not operation inputs and encourage routing decisions outside the declared path contract. |
| `Multipart`, dynamic `Field`, `MultipartError` | `MultipartForm<T>`, `FilePart` | Superset for REST contracts: fields and per-part encodings are typed in both directions. Dynamic field iteration is excluded. `multipart`. |
| WebSocket family (`WebSocketUpgrade`, `WebSocket`, `Message`, `CloseFrame`, upgrade callbacks) | `upgrade_unchecked` | Excluded from OpenAPI; available only as an explicitly unchecked route. AsyncAPI is the appropriate contract. |
| extraction `rejection::*` | `Rejection` | Superset: one RFC 9457 family with `Responses`; 400/401/403/406/413/415/422 are documented from the extractor graph. |
| `Handler`, `HandlerService`, `HandlerWithoutStateExt`, `Layered` | `Handler`, route attributes, `EndpointBuilder` | Equivalent typed handler use case. Arbitrary handler layers require `unchecked`; operation-local `Interceptor` is authoritative. Future/service wrapper types are internal plumbing in both designs. |
| `response::IntoResponse`, `IntoResponseParts`, `ResponseParts`, `Response`, `Result`, `ErrorResponse` | `IntoResponse`, `Responses`, typed wrappers, `Result`, `Problem` | Superset: every response type must provide both wire behavior and the complete Responses Object. Raw response parts and erased response errors are excluded. |
| tuple/status/string responses | `Reply`, `Created`, `Accepted`, `NoContent`, `Redirect<CODE>`, `Text`, `Binary<M>` | Superset: statuses and media types are types. Bare `StatusCode`, `String`, `&str`, and ad-hoc tuples are intentionally rejected. `()` is the explicit empty 200. |
| `AppendHeaders`, response `Extension` | `WithHeaders<T, H>`, `HeaderParams` | Superset for wire headers: fields are encoded and described together; repeated `Set-Cookie` values remain separate. Non-wire response extensions are internal. |
| `Redirect` | `Redirect<301/302/303/307/308>` | Superset: invalid redirect statuses do not implement the response traits. |
| `Html<T>` | none | Excluded: HTML/templates are outside this REST framework. |
| `NoContent` | `NoContent` | Equivalent 204 response. |
| `Sse`, `sse::Event`, `KeepAlive` | `Sse<S>`, typed `Event<T>`, `KeepAlive` | Superset under `openapi32`: event items have `itemSchema`; constructors, event fields, keep-alive interval and text/comment are present. |
| `routing::{get,post,put,patch,delete,head,options,trace}` and service variants | route attributes and `EndpointBuilder` | Superset for handlers because inputs/outputs derive the operation. Raw service variants are `unchecked` only. |
| `connect`, `on`, `MethodFilter` | `#[operation(method = ...)]` | Equivalent only when the method is expressible: standard fields in 3.1, additional operations such as `CONNECT` under `openapi32`. |
| `any`, `any_service` | none | Excluded: wildcard method routing cannot produce a closed OpenAPI Path Item. |
| `MethodRouter`, `Route`, `Router::route` | `Endpoint`, `IntoEndpoints`, `EndpointBuilder`, `Router::mount`, `routes!` | Superset: a mountable endpoint always has an Operation description. |
| `Router::{nest,merge,with_state}` | `Router::{nest,merge}`, typed context passed to `build` | Equivalent composition; state is dependency injection rather than an erased router slot. |
| `Router::{layer,route_layer}` | `intercept`, `observe`; `layer_unchecked` | Superset when authoritative. Arbitrary Tower layers are explicitly unchecked and stamp the document non-authoritative. |
| router fallbacks and method-not-allowed fallback | `FallbackPolicy`, `not_found`, `method_not_allowed` | Equivalent boundary behavior. Fallbacks are not invented as documented operations. |
| route/service fallbacks, `route_service`, `nest_service`, `as_service`, `into_service` | `route_unchecked`, `UncheckedService` | Unchecked only because an opaque service cannot state its operation contract. |
| `into_make_service`, connect-info make service | `Service`, `Server`, `BoundServer` | Equivalent server handoff without exposing make-service plumbing. |
| `middleware::{from_fn,map_request,map_response,from_extractor}` and `Next` | `Interceptor`, `OperationContribution`, `Next` | Superset for behavior that can affect the exchange. A contribution is mandatory; otherwise use `unchecked`. |
| `AddExtension`, response-body middleware adapters and public middleware future types | DI / typed response wrappers / internal futures | No separate public primitive is needed; the authoritative equivalents are typed at their point of use. |
| `error_handling::{HandleError, HandleErrorLayer}` | `Result<T,E>`, `IntoProblem`, `ApiError`, interceptor responses | Superset: error mapping and documented statuses cannot drift. |
| `serve`, `Serve`, `WithGracefulShutdown`, `Listener`, `ListenerExt`, `TapIo`, `IncomingStream` | `Server`, `BoundServer`, sealed `Listener`, `Shutdown`, `ConnectInfo` | Equivalent for std/Tokio TCP, HTTP/1 and HTTP/2. Arbitrary transports, Unix sockets, tap-I/O hooks and HTTP/3 are not in the current server contract. |
| root `ServiceExt`, direct Tower `Service` | `Service::into_tower_unchecked`, `UncheckedService` | Unchecked only. The authoritative `Service` intentionally has no Tower impl. |
| root `Error`, `BoxError`, `http` re-export | `Error`, typed rejections, `http` module, `openapi` re-export | Equivalent foundations; boxed handler errors are replaced by typed response/error contracts. |

## Axum-extra 0.12.6

| Axum-extra primitive | Kynos equivalent | Status and boundary |
| --- | --- | --- |
| `body::AsyncReadBody` | `BinaryStream<S, M>` | Superset for responses because media type is explicit; structured streamed request parsing is intentionally not exposed. `openapi32`. |
| `Either`, `Either3` … `Either8` | nested `OneOf<L,R>` for request media; `Reply` or `Negotiated` tuples up to eight for responses | Superset: request alternatives dispatch by content type and reject duplicates/unsupported types; response alternatives keep statuses or Accept negotiation explicit. |
| `Cached<T>` | no public primitive | Extraction memoization is an internal optimization and must not change the handler contract. |
| `CookieJar`, `PrivateCookieJar`, `SignedCookieJar` | `Cookies<T>`; `Auth<S>` for credential cookies | Superset for declared cookies and authentication. Dynamic whole-jar mutation/iteration is excluded. `cookie`. |
| enhanced `extract::Form<T>` and `Query<T>` | `Form<T>`, `Query<T>` | Equivalent typed codecs; Kynos additionally contributes schemas and rejections. |
| `Host`, `Scheme`, `TypedHeader<T>` | `Headers<T>` / `#[derive(Headers)]` | Superset: every read header is declared as a group and described. Reserved `Accept`, `Content-Type`, and `Authorization` use their dedicated contracts. |
| `OptionalPath<T>` | none | Excluded: every OpenAPI path-template parameter is required. |
| `OptionalQuery<T>` | optional fields in `Query<T>`; `QueryString<T,M>` when the entire encoding is the contract | Equivalent describable cases without making a parameter group itself disappear. |
| `WithRejection<E,R>` | a custom `FromRequest*` implementation whose rejection implements `Responses` | Superset: custom rejection mapping remains part of the operation contract. |
| `JsonDeserializer<T>` | `Json<T>` | Delayed deserialization is excluded because it lets handler control flow alter parse/rejection semantics after extraction. |
| dynamic multipart extractor | `MultipartForm<T>` | Superset for declared multipart fields; dynamic field streams are excluded. |
| `JsonLines<AsExtractor>` | none | Structured streamed request parsing is excluded from the current contract. |
| `JsonLines<AsResponse>` | `JsonLines<S>` | Superset under `json + openapi32`, with `itemSchema`. |
| `Protobuf<T>` | `Protobuf<T>` | Equivalent request/response codec with schema contribution. `protobuf`, prost 0.14.4. |
| `Attachment<T>`, `FileStream<S>` | `WithHeaders<Binary/BinaryStream, H>` | Equivalent composable response, including `Content-Disposition`, size/range headers and typed 206 variants through `Reply`; no filesystem-specific response type is prescribed. |
| response `MultipartForm` / `Part` | typed `MultipartForm<T>` / `FilePart` | Superset: one declared schema works for request and response multipart. |
| `JavaScript<T>`, `Css<T>`, `Wasm<T>` | `Binary<M>` or `Text` with an application-defined `MediaType` marker | Equivalent without one wrapper per MIME name. |
| `InternalServerError<T>` | `Problem`, `ApiError` | Superset: RFC 9457 shape and 500 response description are coupled. |
| `ErasedJson` | none | Excluded: erased JSON cannot supply a structural schema. Use `Unchecked<T>` only when unconstrained JSON is intentional. |
| `HandlerCallWithExtractors`, `IntoHandler` | `Handler` inference and route macros | Equivalent capability; adapter types remain internal. |
| handler `Or` fallthrough | none | Excluded: handler order changing which contract handles a request is not a closed operation. |
| `middleware::option_layer` | conditional construction of an `Interceptor`/`Observer`; `layer_unchecked` for Tower | Equivalent authoritative composition where describable, otherwise unchecked. |
| `routing::Resource` | `Group`, route attributes, `routes!` | Equivalent resource grouping without prescribing CRUD names. |
| `TypedPath`, `WithQueryParams`, typed router methods, `vpath!` | generated `endpoint::uri(...)`, `PathParams`, `QueryParams`, `path!` | Superset: one route declaration validates names and generates exact, percent-encoded path/query URIs. |

## Axum-macros 0.5.1

| Axum-macros primitive | Kynos equivalent | Status and boundary |
| --- | --- | --- |
| `#[derive(FromRequest)]`, `#[derive(FromRequestParts)]` | `PathParams`, `QueryParams`, `Headers`, `Cookies`, `SecurityScheme`, plus manual `FromRequest* + Describe` | Superset: arbitrary composite extraction is allowed only when its description is implemented too. |
| `#[derive(FromRef)]` | `#[derive(Provider)]`, `Provides<T>` | Superset compile-time dependency association. |
| `debug_handler`, `debug_middleware` | normal trait diagnostics from route attributes, `Handler`, `Interceptor`, `Observer` | No separate debug-only API is required; the authoritative bounds are always checked. |
| `#[derive(TypedPath)]` | route attributes plus generated `endpoint::uri(...)` | Superset: the path, handler extraction, OpenAPI operation and reverse URI are generated from one declaration. |

Public rejection structs, opaque future types, layer/service adapters, and
sealed helper traits are accounted for by their owning family above. Kynos does
not duplicate implementation-detail wrappers merely to match rustdoc item
count.

## Kynos-only contract surface

The reverse comparison is as important as the axum-to-Kynos direction. These
Kynos primitives have no direct axum-family equivalent:

| Kynos primitive | Why it is additional |
| --- | --- |
| `kynos-openapi` model, `Router::openapi`, `openapi_as`, `validate` | First-class OpenAPI 3.1/3.2 generation and validation. |
| `Schema`, `Registry`, `Constraints`, `Unchecked<T>` | Structural JSON Schema is a compiler-visible requirement; deliberate weak schemas are marked. |
| `Describe`, `Responses`, `RequestContent`, `OperationContribution` | Runtime behavior and document contributions are paired traits. |
| `QueryString<T,M>` | OpenAPI 3.2 whole-query-string parameters. |
| `OneOf<L,R>` and `Accept<T>::respond` / private `Negotiated<T>` construction | Content-Type and Accept dispatch are request-driven and documented. |
| `Created<T>`, `Accepted<T>`, `Redirect<CODE>`, `WithHeaders<T,H>` | Status and response-header contracts are encoded in return types. |
| `JsonSeq`, typed `Sse<Event<T>>`, `BinaryStream<S,M>` | OpenAPI 3.2 `itemSchema` describes streamed items. |
| `Problem`, `IntoProblem`, `ApiError`, `Reply` | Framework and application errors share RFC 9457 and complete response sets. |
| `Provides`, `FromContext`, `Inject`, scopes, singleton/transient context facilities | Compile-time dependency injection without erased state. |
| `Authenticates<S>`, `Authenticator`, `Auth<S>`, `Scoped<S,R>` | Authentication enforcement and Security Requirements are one declaration. |
| `Interceptor` versus `Observer` | Mutating middleware must declare effects; observation is statically non-mutating. |
| typed built-in request ID, limits, rate policy, CORS, compression and tracing contracts | Policy configuration composes with operation descriptions. |
| `TrailingSlashPolicy` | One canonical app-wide strict/308 policy preserving exact path casing. |
| `Server::prepare` / `BoundServer` and TLS client-certificate coupling | Bound addresses have an explicit lifecycle; mTLS changes the security contract. |
| `test` contract-conformance helpers | Tests can assert both HTTP behavior and declared responses. |
| `unchecked` markers and non-authoritative document stamp | Escape hatches are visible in the emitted artifact rather than silent omissions. |

## Deliberate exclusions

The following are closed decisions for the current scope, not missing parity:
WebSockets; HTML/templates/static-asset helpers; wildcard/`any` routes; raw
requests, bodies, headers, services and runtime-chosen statuses; erased state
or JSON; dynamic cookie jars and multipart fields; original/nested URI routing;
handler fallthrough; delayed JSON deserialization; structured streamed request
parsing; arbitrary Tower composition outside `unchecked`; route-local slash or
case normalization; and custom transports, Unix sockets or HTTP/3.
