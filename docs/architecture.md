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
- Bodies are streams of `Bytes`. A body producer is never runtime-aware, which
  is what keeps the coupling surface at five points instead of spreading through
  everything that can emit a response.
- File I/O, database pools, `spawn_blocking` and body producers sit outside that
  boundary and stay the application's concern.
- io_uring is not a second runtime and does not become one here. If
  completion-based I/O ever reaches Kynos it arrives through tokio itself; a
  parallel connection driver inside this crate is out of scope, and io_uring is
  not a design constraint today.

The policy is already visible in the tree: `crates/kynos-openapi/` carries no
runtime dependency at all, and `crates/kynos/src/server/` is the only place
the runtime is named. Work that would widen that set is the work this section
exists to reject. [Dependencies](#dependencies) applies the same containment
rule to the rest of the graph.

### Public API surface

- No async machinery in public signatures: no `BoxFuture`, no `#[async_trait]`,
  no user-visible `Pin<Box<dyn Future>>`, no hand-rolled `Stream`
  implementations. The rule is scoped to the checked surface: `unchecked`
  hands the service to `tower`, whose `Service::Future` is an associated type
  Kynos does not choose, so
  [`UncheckedService`](../crates/kynos/src/unchecked.rs) names a boxed future.
  That is the shape of the escape hatch rather than an exception to the rule,
  and it is the only one.
- `Send`-ness is decided once, at the runtime boundary — never per-trait, and
  never as a bound on a handler.
- No lifetimes in handler signatures. Generics that exist for performance stay
  private.
- Every public type is either a re-export from `http`, `bytes` or `serde`, or
  something Kynos is prepared to own indefinitely.
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
  the default build.
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
| Body trait and erasure | `http-body`, `http-body-util` | [`http/body.rs`](../crates/kynos/src/http/body.rs) | built |
| Protocol driver, HTTP/1 and HTTP/2 | `hyper` | [`server/connection.rs`](../crates/kynos/src/server/connection.rs), [`http/body.rs`](../crates/kynos/src/http/body.rs) | built |
| tokio adapters for the driver | `hyper-util` | [`server/connection.rs`](../crates/kynos/src/server/connection.rs) | built |
| HTTP/1 parsing | `httparse` | never — reached through `hyper` | built |
| HTTP/2 framing | `h2` | never — reached through `hyper` | built |
| TLS | `rustls`, via `tokio-rustls` | [`server/tls/`](../crates/kynos/src/server/tls/) | built |
| Route matching | `matchit` | [`router/`](../crates/kynos/src/router/) | chosen |
| Percent-encoding | `percent-encoding` | [`__private/uri.rs`](../crates/kynos/src/__private/uri.rs) | built |
| Errors | `thiserror` | ambient | built |
| Observability facade | `tracing` | [`server/`](../crates/kynos/src/server/), [`middleware/trace.rs`](../crates/kynos/src/middleware/trace.rs) | built in `server/`, designed in `middleware/` |
| Streaming bodies | `futures-core` | [`response/stream/`](../crates/kynos/src/response/stream/), gated on `openapi32` | designed |
| JSON | `serde_json` | ambient with `serde` | built |
| Form codec | `serde_urlencoded` | [`extract/body/form.rs`](../crates/kynos/src/extract/body/form.rs), [`response/codec/form.rs`](../crates/kynos/src/response/codec/form.rs) | designed |
| Multipart codec | `multer` | [`extract/body/multipart.rs`](../crates/kynos/src/extract/body/multipart.rs) | designed |
| Protobuf codec | `prost` | [`extract/body/protobuf.rs`](../crates/kynos/src/extract/body/protobuf.rs), [`response/codec/protobuf.rs`](../crates/kynos/src/response/codec/protobuf.rs) | designed |
| Cookies | `cookie` | [`extract/params/cookie.rs`](../crates/kynos/src/extract/params/cookie.rs) | designed |
| Scalar formats, identifiers | `uuid` | [`schema/impls/identifier.rs`](../crates/kynos/src/schema/impls/identifier.rs) | built |
| Scalar formats, dates and times | `chrono`, `jiff` | [`schema/impls/temporal/`](../crates/kynos/src/schema/impls/temporal/) | built |
| Scalar formats, decimals | `rust_decimal`, `bigdecimal` | [`schema/impls/decimal/`](../crates/kynos/src/schema/impls/decimal/) | `rust_decimal` built, `bigdecimal` chosen |
| Compression | `async-compression` | [`middleware/compression.rs`](../crates/kynos/src/middleware/compression.rs) | designed |
| tower interop, outward | `tower-service` | [`unchecked.rs`](../crates/kynos/src/unchecked.rs) | built |
| tower interop, inward | `tower` | [`unchecked.rs`](../crates/kynos/src/unchecked.rs) | designed |
| Document ordering | `indexmap` | [`kynos-openapi`](../crates/kynos-openapi/src/lib.rs) | built |
| YAML emission | `serde_yaml_ng` | [`kynos-openapi/emit/`](../crates/kynos-openapi/src/emit/) | built |
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

`matchit` and the three scalar-format rows are the `chosen` ones. Each is
recorded because the decision is made and the alternatives are closed — see
[below](#what-does-not-move-and-why) — but declaring a dependency the tree does
not name would break the consumed-by-a-member requirement in
[`nfr.md`](nfr.md#dependencies) for no gain. `matchit` arrives with the router
implementation; each scalar crate arrives with the `Schema` implementation that
names it, and becomes `built` in the same commit, since a leaf implementation
has no skeleton phase to be `designed` in.

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
become one; it reaches the tree only as a transitive dependency of `cookie`.

### Scope edges

**HTTP/3 and QUIC are out.** The server contract is HTTP/1 and HTTP/2. If
demand justifies HTTP/3 it arrives as an additive `http3` feature over `h3` and
`quinn`, alongside the existing driver rather than reshaping it.

**Custom transports and Unix sockets are out**, for the same reason the runtime
is: they widen the coupling surface the runtime policy exists to bound.

### What the table does not yet claim

Several rows are `designed` because the manifest runs ahead of the skeleton:
`multer`, `serde_urlencoded`, `async-compression` and `cookie` are declared by
`crates/kynos` and named by no code in it. Each is gated behind an
off-by-default feature, so no default build carries a dependency it does not
use — which is why `futures-core` became optional too, rather than being
compiled into every 3.1 build for a module 3.1 cannot reach.

Two rows are `designed` for a subtler reason: the crate is named, but only in
the bound of a body that is still `todo!()`. `futures-core` appears in the
`Stream` bounds of the response types under `response/stream/`, whose builders
are real and whose responses are not; `tower` appears only in the bound of
`layer_unchecked`. `tower-service` is separate and genuinely `built`, because
the outward direction — mounting a Kynos service into someone else's stack —
is implemented.

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

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

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
