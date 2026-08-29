# Architecture

## Runtime policy

Kynos runs on tokio. That is settled: there is no runtime abstraction, no
executor generic, no feature flag selecting another runtime, and no plan to add
one.

The temptation is to abstract over the runtime so a completion-based executor
could be swapped in later. The obstacle is not the executor, it is the I/O
model. tokio is readiness-based: a caller hands out `&mut [u8]` and polls until
the kernel is ready. A completion-based runtime gives the kernel ownership of
the buffer for the duration of the operation, so every operation takes an owned
`IoBuf` and returns a `BufResult`. No single trait spans both without either
forcing owned buffers on every caller or inserting a copy at the shim.
`hyper::rt::Read` and `hyper::rt::Write` are the obvious seam, and they are
readiness-shaped, so a completion runtime behind them pays for an intermediate
buffer that consumes much of what it came for.

**Policy:**

- tokio is a hard dependency, not a generic parameter. No runtime abstraction
  trait exists — not public, not internal. The server calls tokio directly.
- tokio types appear in zero handler-facing signatures. The one deliberate
  exception anywhere in the public API is
  [`server::address::Listener::Tokio`](../crates/kynos/src/server/address.rs)
  together with its `From<tokio::net::TcpListener>` conversion, which exist so
  an already-bound `tokio::net::TcpListener` can be handed to the server;
  naming the runtime there is the point, not a leak. A surface check counts
  both, because the conversion names the type as plainly as the variant does.
- The runtime coupling surface is exactly five points, and they all live in
  `crates/kynos/src/server/`: the accept loop and listener, connection socket
  read and write (the `tokio::io::{AsyncRead, AsyncWrite}` bound on the
  connection driver, adapted for hyper by `hyper_util::rt::TokioIo`), `spawn`,
  timers (request timeout, keepalive, shutdown grace), and the shutdown
  signal. Holding that count is about auditability and a small public surface,
  not about keeping a swap open.
- Bodies are streams of `Bytes`. A body producer is never runtime-aware —
  *except where the body's own contract is a timer*, which is the Server-Sent
  Events keep-alive and nothing else. See the allowance table below.
- File I/O, database pools, `spawn_blocking` and body producers sit outside that
  boundary and stay the application's concern.
- io_uring is not a second runtime and does not become one here. If
  completion-based I/O ever reaches Kynos it arrives through tokio itself; a
  parallel connection driver inside this crate is out of scope, and io_uring is
  not a design constraint today.

The coupling surface inside `server/` is exactly the five points above, and
that has not changed. What has is the claim that `server/` is the *only* place
the runtime is named. It was already false at two sites when it was written, and
a third has since been added deliberately.

A rule that is false is worth less than a list that is checkable, so the claim
is now an enumeration:

| Site | Names | Why it is not in `server/` |
| --- | --- | --- |
| `server/{accept,connection,mod}.rs`, `server/tls/` | the five coupling points | — |
| `middleware/limits.rs` | `tokio::{time::timeout, time::Instant, time::Sleep, time::sleep, sync::Semaphore}` | the timer wraps the chain's future, which does not exist until after routing; the permit bounds requests already in it; the body timer outlives both, because a streamed body is still being produced after the chain has returned |
| `middleware/compression/` | `tokio::io::{AsyncRead, AsyncWrite, ReadBuf}` | `async-compression`'s encoders are written against tokio's I/O traits; no byte here crosses a socket |
| `middleware/decompression/` | `tokio::io::{AsyncRead, ReadBuf}` | the same traits for the same reason, in the other direction: a client-compressed request body is decoded before an extractor sees it, which is as far from a socket as the encoders are |
| `response/stream/sse.rs` | `tokio::time::{Instant, Sleep, sleep}` | a keep-alive is a property of one body, and the connection driver cannot know a body is an event stream |
| `router/assets/fs/` | `tokio::fs::{metadata, read, File}`, `tokio::io::{AsyncReadExt, AsyncSeekExt}` | which file a request wants is not known until routing has chosen the operation, and the read is the operation; a byte range seeks to what it asked for rather than reading the file and discarding most of it |

**Six rows, and the count is the check.** [`nfr.md`](nfr.md#runtime) states the
containment requirement against this table rather than against `server/` alone,
so a seventh site is a failing build rather than a silently broken sentence.
`mise run containment:check` is where that count runs.

The last two were added deliberately, which is what the table is for. Serving
a file from disk means reading one, and which file is not known until routing
has chosen the operation — so there is nowhere in `server/` for it to live.
Decompression is the compression row's mirror: a client-compressed body is
decoded before an extractor sees it, on the same `async-compression` traits and
at the same distance from a socket. That each required an entry here, argued for
on its own terms, is the mechanism working rather than the mechanism being
worked around.

Decompression is also the case that shows why this count had to be wired rather
than asserted. It was a sixth site while the sentence above said there were
five, and the row that would have caught it was `planned`.

Moving the SSE timer into `server/` was considered and rejected. `TestClient`
and `Service::call` drive a built service with no server at all — which is what
[`examples/testing.rs`](../crates/kynos/examples/testing.rs) exists to show — so
a keep-alive owned by the connection driver would silently not happen in every
test and every tower deployment, and it would add a general mechanism to `Body`
for one caller.

`crates/kynos-openapi/` still carries no runtime dependency at all. Work that
would add a fifth row is the work this section exists to reject.
[Dependencies](#dependencies) applies the same containment rule to the rest of
the graph.

### Public API surface

- No async machinery in public signatures: no `BoxFuture`, no `#[async_trait]`,
  no user-visible `Pin<Box<dyn Future>>`, no hand-rolled `Stream`
  implementations. The rule is scoped to the checked surface: `unchecked`
  hands the service to `tower`, whose `Service::Future` is an associated type
  Kynos does not choose, so
  [`UncheckedService`](../crates/kynos/src/unchecked.rs) names a boxed future.
  That is the shape of the escape hatch rather than an exception to the rule,
  and it is the only one.

  The clause is about the surface, so a hand-rolled `Stream` on a type nobody
  can name is not an exception to it. Two exist and both are the same shape:
  [`Framed<S>`](../crates/kynos/src/response/stream/json.rs) and
  [`Records<S>`](../crates/kynos/src/response/stream/sse.rs) each take
  `S: Stream` as a bound and stay private, which is only possible because the
  caller brought the stream. What is left is the exception, and it is
  enumerated for the reason the runtime allowance is: a rule that is false is
  worth less than a list that is checkable.

  | Site | Hand-rolls | Why no bound can carry it |
  | --- | --- | --- |
  | [`extract/body/json_lines/records.rs`](../crates/kynos/src/extract/body/json_lines/records.rs) | `Stream` for the public `Records<T>`, the streamed JSON request body | a *request* body's stream is produced by Kynos rather than supplied by the handler, so there is no caller type to bound |

  **One public row, three sites, and the count is the check** — the count runs
  in [`nfr.md`](nfr.md#workspace). The two `Records` are unrelated types that
  ended up sharing a name: the private one frames SSE events on the way out,
  the public one decodes JSON records on the way in, and only the second is
  ever written in a handler signature.
- `Send`-ness is decided once, at the runtime boundary — never per-trait, and
  never as a bound on a handler.
- No lifetimes in handler signatures. Generics that exist for performance stay
  private.
- Every public type is either a re-export from `http`, `bytes` or `serde`, or
  something Kynos is prepared to own indefinitely.
- Fields the specification makes mutually exclusive are one enum, not several
  `Option`s, and a field whose legal values are a subset of some wider type is
  that subset. No validator rule restates either. See
  [why exclusions are types](#why-exclusions-are-types-rather-than-rules) for
  the bound on this.
- Features are additive only.
- Macros expand to readable code and carry user spans.
- Ship 1.0, freeze the core, and put subsequent velocity into satellite crates.

## Dependencies

The table below is the maximal scope: the whole of what Kynos intends to depend
on. A crate absent from it is not a candidate, and "can we add X?" is answered
by naming the row X displaces rather than by arguing that X is good.

**Policy:**

- Every dependency is either *ambient* or *contained*. Ambient dependencies may
  be named anywhere, and there are exactly four: `http`, `bytes`, `serde` and
  `thiserror`. The first three are the re-export allowance above; `thiserror`
  is a derive that reaches no public signature. Every other dependency may be
  named only under the path the table gives it. This is the runtime rule
  generalized, not a second rule.
- `httparse` and `h2` are never named. They are hyper's HTTP/1 parser and
  HTTP/2 framing layer and they reach Kynos through it. Naming either would
  mean Kynos had taken the protocol over, which is a decision this section
  closes rather than defers.
- A new dependency arrives feature-gated and additive, never as a widening of
  the default build. A corollary that has already bitten: a capability in the
  *default* build cannot be gated, so it cannot take a dependency at all —
  which is why base64 decoding and the constant-time comparison
  [`security/`](../crates/kynos/src/security/) needs are written here rather
  than taken from `base64` and `subtle`.
- **A row is removed when the module it names goes the other way.** `cookie`
  had a row and was named by no source line: the derive hand-rolled RFC 6265
  and the response side did not exist. Using it once the response side landed
  would have put `cookie::Cookie` in a public signature, against the re-export
  rule above. The row is gone rather than left as aspiration — the same
  correction `mime` and `pin-project-lite` already received.
- rustls is the only TLS backend that ships. The accept path keeps the socket
  and the rustls connection separable rather than fusing them into one opaque
  stream, so a second backend stays an additive change. No public trait
  abstracts the backend, today or later.

### The graph

| Layer | Crate | Named in | Status |
| --- | --- | --- | --- |
| Runtime, sockets, timers, signals | `tokio` | `server/` | built |
| Request and response types | `http` | ambient | built |
| Byte buffers | `bytes` | ambient | built |
| Body trait and erasure | `http-body`, `http-body-util` | [`http/body.rs`](../crates/kynos/src/http/body.rs), [`extract/body/`](../crates/kynos/src/extract/body/), [`test/mod.rs`](../crates/kynos/src/test/mod.rs) | built |
| Protocol driver, HTTP/1 and HTTP/2 | `hyper` | [`server/connection.rs`](../crates/kynos/src/server/connection.rs), [`http/body.rs`](../crates/kynos/src/http/body.rs) | built |
| tokio adapters for the driver | `hyper-util` | [`server/connection.rs`](../crates/kynos/src/server/connection.rs) | built |
| HTTP/1 parsing | `httparse` | never — reached through `hyper` | built |
| HTTP/2 framing | `h2` | never — reached through `hyper` | built |
| TLS | `rustls`, via `tokio-rustls` | [`server/tls/`](../crates/kynos/src/server/tls/) | built |
| Route matching | `matchit` | [`router/`](../crates/kynos/src/router/) | built |
| JSON Schema instance validation | `jsonschema` | [`test/conformance.rs`](../crates/kynos/src/test/conformance.rs), gated on `test-util` | built |
| Percent-encoding | `percent-encoding` | [`__private/uri.rs`](../crates/kynos/src/__private/uri.rs) | built |
| Errors | `thiserror` | ambient | built |
| Observability facade | `tracing` | [`server/`](../crates/kynos/src/server/), [`middleware/trace.rs`](../crates/kynos/src/middleware/trace.rs) | built |
| Streaming bodies | `futures-core` | [`response/stream/`](../crates/kynos/src/response/stream/), [`extract/body/json_lines/`](../crates/kynos/src/extract/body/json_lines/), [`http/body.rs`](../crates/kynos/src/http/body.rs), gated on `openapi32` | built |
| JSON | `serde_json` | ambient with `serde` | built |
| Form codec | `serde_urlencoded` | [`extract/body/form.rs`](../crates/kynos/src/extract/body/form.rs), [`response/codec/form.rs`](../crates/kynos/src/response/codec/form.rs) | built |
| Multipart codec | `multer` | [`extract/body/multipart.rs`](../crates/kynos/src/extract/body/multipart.rs) | built |
| Protobuf codec | `prost` | [`extract/body/protobuf.rs`](../crates/kynos/src/extract/body/protobuf.rs), [`response/codec/protobuf.rs`](../crates/kynos/src/response/codec/protobuf.rs) | built |
| Scalar formats, identifiers | `uuid` | [`schema/impls/identifier.rs`](../crates/kynos/src/schema/impls/identifier.rs) | built |
| Scalar formats, dates and times | `chrono`, `jiff` | [`schema/impls/temporal/`](../crates/kynos/src/schema/impls/temporal/) | built |
| Scalar formats, decimals | `rust_decimal`, `bigdecimal` | [`schema/impls/decimal/`](../crates/kynos/src/schema/impls/decimal/) | built |
| Compression | `async-compression` | [`middleware/compression/`](../crates/kynos/src/middleware/compression/) | built |
| tower interop, outward | `tower-service` | [`unchecked.rs`](../crates/kynos/src/unchecked.rs) | built |
| tower interop, inward | `tower` | [`unchecked.rs`](../crates/kynos/src/unchecked.rs) | built |
| Document ordering | `indexmap` | [`kynos-openapi`](../crates/kynos-openapi/src/lib.rs) | built |
| YAML emission | `serde_yaml_ng` | [`kynos-openapi/emit/`](../crates/kynos-openapi/src/emit/), [`error/mod.rs`](../crates/kynos/src/error/mod.rs) | built |
| Macro parsing | `proc-macro2`, `quote`, `syn` | [`kynos-macros`](../crates/kynos-macros/src/) | built |
| HTTP/3, QUIC | — | — | out of scope |

Three statuses, and the distinction is what keeps the table checkable:

| Status | Meaning |
| --- | --- |
| `built` | Reached by code that is implemented |
| `designed` | Declared by a member crate; the module that owns it is still skeleton |
| `chosen` | Settled as the answer, declared by nobody. It appears in no manifest and no lockfile |

A row whose *Named in* column says `never` or `ambient` is `built` when the
code that reaches it is implemented; it has no owning module to be a skeleton.
`httparse` and `h2` are the clear cases: no member declares either, and they
are reached only through `hyper`.

`chosen` currently has no occupants, and the rows that held it are the reason
the status is worth keeping. The three scalar-format rows were `chosen` while
the decision was made and the alternatives closed — see
[below](#what-does-not-move-and-why) — because declaring a dependency the tree
does not name would break the consumed-by-a-member requirement in
[`nfr.md`](nfr.md#dependencies) for no gain. Each arrived with the `Schema`
implementation that names it and became `built` in the same commit, since a leaf
implementation has no skeleton phase to be `designed` in.

`matchit` was the fourth and left the same way: it arrived with the router
implementation, which is exactly what a `chosen` row predicts happening to it.

`hyper` has two sites rather than one because the body handover is where its
`Incoming` type enters, and `http/body.rs` is by design the only place the
erased body is named. `hyper-util` is a separate row because it is a separate
allowance: it supplies the tokio adapters named in the runtime policy above,
and it does not reach the body.

The three scalar-format rows are a **new layer**, and saying so is the point.
This table was organized by transport and codec and had no home for a crate whose
only job is to give a value a JSON Schema `format` — which is why "can we add
`uuid`?" read as unanswerable rather than as settled. Nothing is displaced,
because there was nothing in that position to displace. The rule the layer
inherits is the ordinary one: each crate is named only under
[`schema/impls/`](../crates/kynos/src/schema/impls/), each arrives behind an
off-by-default feature, and none is reachable from a default build.

### What does not move, and why

**hyper.** hyper parses HTTP/1 with `httparse`, so owning the codec would mean
owning framing and buffering rather than parsing — and hyper's framing is close
to allocation-free already: header values are `Bytes` slices into the read
buffer, and the `HeaderMap` is recycled between requests on a connection. Two
limits are real, and are recorded here rather than argued away. hyper allocates
an 8 KiB write buffer at connection construction and never shrinks the read
buffer below 8 KiB, with no pool and no way to supply one, so roughly 16 KiB
per live connection is not configurable; `max_buf_size` moves the ceiling, not
the floor. And hyper releases HTTP/2 flow-control capacity as soon as a frame
is polled, so backpressure on an HTTP/2 request body is not expressible through
it — the `h2` crate's `release_capacity` is the only remedy, and taking it
means owning the connection driver. The question reopens on one measurement:
RSS delta at 100k idle keep-alive connections, with and without TLS. Until that
number exists it stays closed.

**matchit.** A `{param}` matches exactly one path segment, never crossing a
`/`, and captures a borrowed slice of the request path rather than an owned
string — which is what makes the zero-allocation requirement in
[`nfr.md`](nfr.md#routing) reachable at all. matchit also understands catch-all
patterns, which Kynos does not: a catch-all has no OpenAPI equivalent, so the
router rejects that syntax before matchit is asked to insert it. That check is
syntactic and belongs above the dependency rather than inside it, which is why
the anti-pattern and the crate can coexist.

**rustls.** It is the only TLS backend, and there is no seam that would let an
application supply another. What the design does preserve is the option to add
one: the accept path holds the socket and the rustls session as separable
values, so a kernel-TLS path could be introduced additively later. That costs
nothing today and is not a commitment — see the rationale below.

**Two date backends and two decimal backends, not one each.** These are the only
rows where Kynos ships alternatives, and the reason is that the alternatives are
not competing answers to one question. `chrono` and `jiff` divide by ecosystem
rather than capability; `rust_decimal` is a fixed 96-bit mantissa with a scale
ceiling of 28 and `bigdecimal` is arbitrary precision, so money and science want
different crates. Picking one would be choosing the user's problem for them,
which invariant 3 forbids. An umbrella feature defines each concept's shape once
so the backends cannot diverge in what they emit, and enabling an umbrella with
no backend does not compile. The `time` crate is **not** a backend and will not
become one; it reaches the tree only as a transitive dependency of `rcgen`,
which is itself a dev-dependency of the TLS example. It used to arrive through
`cookie` as well, which is gone.

### Three dependencies static assets and caching would want, refused

Each would be a *table* Kynos can write down and test as a closed enumeration,
which is the form this project prefers to a database it cannot check.

**No media-type guesser.** `mime_guess` bundles a generated database that only
sampling can verify. The table in
[`router/assets/media.rs`](../crates/kynos/src/router/assets/media.rs) is the
whole set, so a test asserts it is closed, sorted and free of duplicates — and
the emitted description prints whatever it resolved to, which makes a wrong row
visible rather than silent.
[`mime_names`](../crates/kynos-openapi/src/model/body/mime_names.rs) already
records why a media type is a `&'static str` here rather than a parsed `Mime`.

**No hasher.** An entity tag is a cache validator rather than a security
primitive: RFC 9110 section 8.8.3 asks only that it change when the
representation does, and nothing verifies it. `blake3` and `sha2` both arrive
with the `unsafe` this workspace forbids, for a 64-bit token. FNV-1a with the
length folded in is computed in `kynos-macros` at expansion time and never runs
in a served request.

**No HTTP-date crate**, and still none — but the premise under it changed.
This row used to read "Kynos sends no `Last-Modified` and reads no
`If-Modified-Since`, so there is no date to render or parse". Ranged delivery
sends one and reads one, so there is.

What the row refuses is a *dependency*, and the reason it gives applies
unchanged: a crate here would be a database only sampling can verify. An
HTTP-date is not that. RFC 9110 section 5.6.7 is a fixed-width grammar over a
closed set of day and month names — three formats, all enumerable — which is
exactly the shape this section says the project prefers to write down and test.
[`http/date.rs`](../crates/kynos/src/http/date.rs) is that table, and its round
trip is swept across a leap boundary rather than sampled.

The row's other clause was conditional and is honoured: "sending a date obliges
honouring a request that carries one back. Sending neither half is consistent;
sending one is not." Both halves landed together.

The ranking is unchanged. Section 8.8.2 gives `Last-Modified` one-second
resolution, so section 13.1.3 ranks `If-None-Match` above it and a strong entity
tag stays what Kynos reaches for first.

### The language-tag registry, refused

**No BCP 47 crate**, for the reason the three rows above give and with the same
split between a grammar and a database.

`unic-langid`, `icu`, `fluent` and `accept-language` would each arrive carrying
the IANA Language Subtag Registry — the answer to "is `en` a real language",
which is a table only sampling can verify and one that changes without this
repository noticing. What `response::language` needs is not that. RFC 5646
section 2.1 is a grammar over subtag shapes plus one closed list of seventeen
tags that predate it, and RFC 4647 section 3 is two matching algorithms over
subtag boundaries. All three are exactly the shape this section says the project
prefers to write down and test, and
[`response/language/tag/`](../crates/kynos/src/response/language/tag/mod.rs) is
that table: its grammar is swept over every shape the ABNF admits, and its
matcher over every range-and-tag pair in a closed alphabet.

So a [`LanguageTag`](../crates/kynos/src/response/language/tag/mod.rs) states that a
string *could* name a language and never that it does. `zz-Qaaa-QM` parses here
and names nothing, which costs a client the same default a request for `ja`
already gets — and closing that gap is the one thing a registry would buy, for
a dependency that would have to be right about the world rather than about a
grammar. [`nfr.md`](nfr.md) records it as a gap with the test that characterizes
it rather than as an omission.

The translation catalogue behind a localized response is a separate refusal and
a firmer one: it is the third invariant applied directly. A message catalogue,
its fallback policy and its translation quality are all things a layer above can
own, and `fluent` or `icu` in the dependency table would be Kynos choosing the
user's problem for them. Kynos negotiates the language and states which one it
chose; the strings are the application's, and
[`errors.md`](errors.md) records what that means for a problem detail.

`moka` and `jsonwebtoken` are a fourth kind. Both are named by one example and
by nothing under `src/`, which is the standing `rcgen`, `listenfd` and
`tracing-subscriber` already have: this table governs what Kynos depends on, not
what an example demonstrates.

### What each feature gates

The [README](../README.md#feature-flags) says what a flag *adds*, which is what
someone choosing one needs. This says what it *gates* and which document here
governs the thing behind it, which is what someone changing one needs. Fourteen
of these were named nowhere in this directory, and a flag whose module has a
normative home should be reachable from it.

A gate belongs on the `pub mod` line rather than on each item inside, so the
module column is also where the `#[cfg]` lives.

| Flag | Gates | Governed by |
| --- | --- | --- |
| `openapi31` | the 3.1 object model; the baseline every other flag builds on | [`standards.md`](standards.md) |
| `openapi32` | the 3.2 superset, `#[cfg]`-gated rather than runtime-optional | [`standards.md`](standards.md), [`routing.md`](routing.md) |
| `macros` | `kynos-macros`: the route attributes and the derives | [`handlers.md`](handlers.md) |
| `server` | `server/`, and with it the entire runtime coupling surface | [Runtime policy](#runtime-policy) |
| `http1`, `http2` | the protocol versions hyper drives; `server` alone is a `compile_error!` | [Runtime policy](#runtime-policy) |
| `tls` | `server/tls/`, the only place `rustls` may be named | [Runtime policy](#runtime-policy) |
| `json` | the application JSON codecs. *Not* document emission, which is unconditional | [`handlers.md`](handlers.md) |
| `form`, `multipart`, `protobuf` | the other request and response codecs, one module each under `extract/body/` and `response/codec/` | [`handlers.md`](handlers.md) |
| `cookie` | cookie parameters, response cookies and `SetCookies`. Names no crate: Kynos owns the RFC 6265 | [`standards.md`](standards.md) |
| `compression` | `middleware/compression/` and `middleware/decompression/`, two of the six runtime-allowance rows | [`middleware.md`](middleware.md) |
| `trace` | the `tracing` facade only; the subscriber stays the application's | [`middleware.md`](middleware.md) |
| `uuid`, `time-*`, `decimal-*` | one `schema/impls/` module each. `time` and `decimal` are umbrellas that define the shape both backends map onto | [`schema.md`](schema.md#behind-a-feature-flag) |
| `yaml` | the second document emitter, in `kynos-openapi` and re-exported | [`standards.md`](standards.md) |
| `test-util` | `test/`, and the JSON Schema validator its conformance assertions need | [`testing.md`](testing.md) |
| `cache` | `middleware/cache/` and `middleware/conditional/`: the seam, never a store | [`middleware.md`](middleware.md) |
| `assets` | `assets!` and `router/assets/`: a fixed set, so every path is a literal and nothing is waived | [`routing.md`](routing.md) |
| `assets-fs` | `router/assets/fs/`. Implies `unchecked`, because a directory's membership is not fixed | [`routing.md`](routing.md) |
| `docs` | `Router::docs`: the reference page and the description, as two described operations | [`routing.md`](routing.md) |
| `unchecked` | `unchecked.rs`, the only place `tower` may be named. Documented anti-pattern | [`middleware.md`](middleware.md) |
| `full` | every flag above except `unchecked` and `assets-fs`. A testing convenience, not a recommended default | — |

### Scope edges

**HTTP/3 and QUIC are out.** The server contract is HTTP/1 and HTTP/2. If
demand justifies HTTP/3 it arrives as an additive `http3` feature over `h3` and
`quinn`, alongside the existing driver rather than reshaping it.

**Custom transports and Unix sockets are out**, for the same reason the runtime
is: they widen the coupling surface the runtime policy exists to bound.

### What the table does not yet claim

Nothing, as of the skeleton landing. Every row is `built`: the API-skeleton
milestone is over, the `todo!()` bodies are implemented, and each crate the
table names is reached by code that runs.

That is a change worth recording rather than quietly deleting. `multer`,
`serde_urlencoded`, `async-compression` and `cookie` were `designed` because the
manifest ran ahead of the skeleton — declared by `crates/kynos` and named by no
code in it. `futures-core` and `tower` were `designed` for a subtler reason: the
crate was named, but only in the bound of a body that was still `todo!()`. All
six are now consumed at exactly the path this table gives them, which is the
property the *Named in* column exists to be checkable against.

Each optional row is still gated behind an off-by-default feature, so no default
build carries a dependency it does not use — which is why `futures-core` is
optional rather than compiled into every 3.1 build for a module 3.1 cannot
reach, and why `jsonschema` is reachable only through `test-util`.

`chosen` therefore has no occupants either. It stays in the status table because
the state it names is real and will recur: a decision made, and the alternatives
closed, before any manifest records it.

`mime` and `pin-project-lite` were in this list and are gone. Neither was a
deferred wiring job: media types are carried as `&'static str` on purpose, for
the reason [`mime_names.rs`](../crates/kynos-openapi/src/model/body/mime_names.rs)
records — the model must express media type *ranges* and vendor types that a
parsed `Mime` would normalize away — and the streaming surface pins nothing
by hand. A row whose module deliberately went the other way is not `designed`,
it is wrong.

This section exists because a dependency graph that overstates what is wired is
worse than no graph.

## Invariants

Three rules generate the framework's shape. The eleven
[anti-patterns](../README.md#anti-patterns) in the README are consequences of
them rather than independent decisions, which is why that list can be checked
for completeness instead of merely extended.

**1. Core names no external standard, version, or vendored syntax.**

Core speaks its own intermediate representation and nothing else. OpenAPI 3.1
and 3.2, JSON, path-template syntax and wire codecs are all projections of that
IR, and they belong in satellite crates. The test is mechanical: if a standards
body could revise it, it cannot be in core.

*Where that boundary falls today:* `kynos-openapi` is still one crate, but it
is split three ways —
[`model/`](../crates/kynos-openapi/src/model/) is version-agnostic data and
invariant-preserving constructors, [`emit/`](../crates/kynos-openapi/src/emit/)
turns it into an artifact at a chosen version, and
[`validate/`](../crates/kynos-openapi/src/validate/) checks one. `model/` is
what an eventual IR crate would be. The seam is not yet a crate boundary
because there is no second projection to justify one; when there is, drawing it
is a directory move rather than a redesign.

**2. Model the general case; today's common case is a degenerate instance.**

An operation is a signature over arbitrary HTTP mechanics that produces a
*sequence* of response events. Verb-plus-path is one projection of that
signature, and a single response is a sequence of length one. Building the
narrow version first means rewriting the type system when 3.2's
`additionalOperations`, streaming responses, or Moonwalk's signature model
arrive — none of which are hypothetical.

**3. Be the substrate, not the product.**

Anything a layer above could own, it should own: ORMs, templating, sessions,
scaffolding, authoring languages. The job is to make that layer possible.
Concretely, that means publishing the IR as a stable machine-readable artifact,
and supporting consumption of an external specification in order to *verify*
code against it — rather than competing with TypeSpec or Smithy for the
authoring slot.

*Where the line falls for a reference UI.*
[`router::docs`](../crates/kynos/src/router/docs/) ships two routes and the two
pages that boot Scalar and Redoc from a CDN, and ships neither renderer. A page
is a string with a script tag in it, so the whole integration is bytes and a
media type — no dependency, no bundled asset, and no version of anyone's UI in
this tree, which is why the graph above gains no row. `Docs::custom` is the
seam: a deployment wanting a third renderer, or a vendored bundle under a strict
CSP, supplies the page. What Kynos owns is the part only Kynos can — that the
description the page fetches is the description the router emits, mounted where
the router actually mounted it, which needs the document after mounting and
before serving and is a window no layer above has.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why exclusions are types rather than rules

The OpenAPI specification states a good many exclusions between optional fields:
a Parameter Object carries `schema` or `content`, a Media Type Object shows
`example` or `examples`, a Link Object names `operationRef` or `operationId`.
Each can be modelled as the fields the specification writes plus a rule that
rejects the bad combinations, or as a type whose values are the good ones.

`model/` takes the second, and the deciding argument is what happens to the
validator under the first. A rule no document can violate is not a check, it is
dead code — and a validator carrying dead rules is one a reader cannot trust to
be exhaustive. Removing them leaves a module whose contents are exactly what a
type cannot say on its own: uniqueness across a whole document, correspondence
between a path template and its parameters, names that must resolve against
declarations elsewhere.

The consequences are the point rather than a side effect. The illegal
combination cannot be built, cannot be parsed, and cannot be emitted, so the
three ways a description reaches a client generator are closed at once instead
of one at a time. The constructors are then a list of the specification's legal
combinations, which is a thing a reviewer can check against the specification.
And a field that only means something alongside another — `style` beside a
schema, never beside a `content` — lives in the variant that gives it meaning,
so setting it where it does not apply is not a mistake to report but a sentence
with nowhere to be written.

Exclusion is one of three cases, and the other two fall on either side of it.

A *narrowed domain* is a field whose legal values are a subset of some wider
type's. A Header Object's `style` is the extreme version: the specification
gives it exactly one legal value, `simple`. This is the same argument with one
field instead of two — the illegal values are illegal unconditionally, so a rule
checking for them is dead the moment the type stops admitting them. `HeaderStyle`
and `EncodingStyle` are `Style` narrowed this way.

A *pairing* is two fields that constrain each other while both stay settable,
and it stays a rule. A Parameter Object's `style` is legal or not depending on
its `location`, and `location` is a public field, so a valid pair can be
invalidated after construction; `IllegalStyle` therefore has work to do. The
distinction is not whether the specification writes a table but whether the
other side of the constraint is a live field. A header has no `in` to disagree
with — it is fixed by the header's position in the document — so the same table
collapses, for a header, into a domain.

What survives all three are facts about *names*: the key a value is filed
under, which no value's type reaches however narrow its fields become. A
`Content-Type` entry in a `headers` map is ignored by the specification, and no
`Header` can say so about itself, so that stays a rule beside uniqueness and
path correspondence.

The bound is round-tripping. `kynos-openapi` must hold descriptions it did not
produce ([`routing.md`](routing.md#why-the-model-is-more-permissive-than-the-router)),
so only combinations that make a *document* invalid may become unrepresentable.
A rule about what Kynos is willing to *serve* is a router rule, and belongs in
the narrower layer. Deprecation is not exclusion either: `SchemaObject::example`
is superseded by `examples` rather than excluded by it, and both stay so that a
parsed description survives the trip back out.

### Where io_uring would actually pay

io_uring wins where syscall counts are high: many small writes, static file
serving, proxying, very high accept rates, and large populations of idle
connections. It wins nothing where the workload is dominated by a database
round-trip or by serde CPU time, which describes most JSON APIs — including most
of what Kynos is for.

The diagnostic is therefore syscalls per request. At roughly four syscalls per
request there is nothing left to win, which is why the runtime question is
closed rather than deferred: the alternative was never going to pay for the
abstraction it would have cost.

Two deployment facts compound this. io_uring is Linux-only, and it is disabled
by seccomp policy by default on many managed platforms. That makes it a
deployment-restricted optimization, which is a poor thing to build a foundation
on.

### Why hyper stays

The case for owning the HTTP/1 codec rests on a category error: `httparse` is
already what hyper parses with, so a rewrite would not touch the parser. What
it would take over is framing and buffering — and measured against that, hyper
costs roughly nothing per request. A GET with standard headers allocates once
or not at all.

The honest counter-argument is the per-connection buffer floor. Roughly 16 KiB
that cannot be pooled or reclaimed between requests on an idle keep-alive
connection is around 1.6 GiB at 100k connections, against a requirement whose
threshold is still `TBD`. It is the one place in the comparison with a large
multiplier behind it. Three things blunt it: under rustls the TLS buffers per
connection are comparable or larger, so removing hyper's share may only halve
the real figure; `unsafe_code = "forbid"` means a Kynos codec starts behind
hyper's own parser, which uses uninitialized memory for the header array; and
owning HTTP framing means owning request-smuggling response permanently. The
resolution is a measurement, not more argument.

Upstreaming is not a schedule that can be planned on — hyper ranks correctness
above speed and speed above flexibility, and a seam for supplying a buffer pool
is exactly the flexibility it declines. Vendoring trades a maintained
dependency for a fork that ages.

The other reason to leave the codec alone is that the cheap wins are not in it.
Discarding the ALPN protocol already negotiated during the handshake and
letting the driver re-sniff it costs a read syscall per connection; cloning
per-connection TLS metadata per request copies the peer certificate chain each
time; erasing every body through a boxed trait object behind a mutex allocates
once per request for a body that is not shared. Those cost more per request
than hyper's entire HTTP/1 codec, and none of them require replacing it.

The second of the three is taken.
[`Connection`](../crates/kynos/src/extract/connection.rs) is built once per
accepted socket and reference-counted onto each request, so a certificate chain
is copied once per connection rather than once per request. It landed as the fix
for a defect rather than as an optimization — the metadata was private and
`#[expect(dead_code)]`, and the extractor that was meant to read it panicked on
every request — which is why the entry stays here rather than moving to a
benchmark: the cost was real, and removing it was not what motivated the
change.

### Why kernel TLS is deferred

Kernel TLS moves record encryption into the kernel once rustls has finished the
handshake, and rustls supports the handover directly. The published wins,
however, all come from `sendfile` on large static bodies — the path where the
userspace round trip disappears entirely. A small dynamic JSON response cannot
use it, so what remains is one avoided copy of a few kilobytes, against an AEAD
cost that is not the bottleneck. Measurements on receive-side kernel TLS have
shown worse tail latency than userspace TLS for request/response traffic, which
is precisely this workload's shape.

Runtime detection is also less safe than it looks. A sandboxed kernel can
accept the socket options that enable kernel TLS and then ignore them, which a
probe cannot distinguish from real support — and the failure mode is plaintext
on the wire rather than an error. Any adoption has to verify end to end that
bytes leaving the socket are actually encrypted, not merely that the setup
calls returned success.

Revisiting is worth it when all of these hold: a workload where a material
share of bytes are `sendfile`-able; a deployment target on a real kernel new
enough to handle TLS 1.3 key updates, with the TLS module loadable on the
nodes; a maintained Rust binding that has seen production use; and a profile
showing TLS is among the top costs. Until then the only thing worth spending is
the design constraint recorded above — keep the socket and the session
separable — which costs nothing and keeps the option alive.

### Language features that will reshape the surface

Async closures, async `Drop`, `AsyncIterator` and generator blocks, and
dyn-compatible async functions in traits each change the *shape* of an idiomatic
async API rather than merely adding to it. Anything exposed today in their
absence becomes the legacy path once they land. This is the strongest argument
for a small public surface: not maintenance cost in the abstract, but that a
narrow surface has less to be wrong about when the language moves.

### Surface area as the real liability

Public API item count is worth tracking in CI as a budget. It is a crude proxy,
but it is the one that correlates with maintenance burden, and an explicit
budget converts surface growth from something that happens into something that
is decided. The corresponding requirement is recorded in [`nfr.md`](nfr.md).
