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
  [`server::Listener::Tokio`](../crates/kynos/src/server.rs), which exists so an
  already-bound `tokio::net::TcpListener` can be handed to the server; naming
  the runtime there is the point, not a leak.
- The runtime coupling surface is exactly five points, and they all live in
  `crates/kynos/src/server.rs`: the accept loop and listener, connection socket
  read and write (the `hyper::rt` implementation), `spawn`, timers (request
  timeout, keepalive, shutdown grace), and the shutdown signal. Holding that
  count is about auditability and a small public surface, not about keeping a
  swap open.
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
runtime dependency at all, and `crates/kynos/src/server.rs` is the only place
the runtime is named. Work that would widen that set is the work this section
exists to reject.

### Public API surface

- No async machinery in public signatures: no `BoxFuture`, no `#[async_trait]`,
  no user-visible `Pin<Box<dyn Future>>`, no hand-rolled `Stream`
  implementations.
- `Send`-ness is decided once, at the runtime boundary — never per-trait, and
  never as a bound on a handler.
- No lifetimes in handler signatures. Generics that exist for performance stay
  private.
- Every public type is either a re-export from `http`, `bytes` or `serde`, or
  something Kynos is prepared to own indefinitely.
- Features are additive only.
- Macros expand to readable code and carry user spans.
- Ship 1.0, freeze the core, and put subsequent velocity into satellite crates.

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
