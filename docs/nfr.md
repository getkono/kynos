# Non-Functional Requirements

What each part of Kynos must prove, how it is proven, and whether that proof
runs today. The last column is the honest one: this document is worth keeping
only if it never claims a guarantee CI does not actually enforce.

## Status

| Status | Meaning |
| --- | --- |
| `enforced` | A CI job runs this today. Regressions fail the build |
| `planned` | The requirement is settled and the method needs no tool this repository lacks; it is simply not wired |
| `needs-tooling` | Settled, and blocked on installing something. The tool is named |
| `blocked-on-impl` | The surface the method would assert against does not exist yet |
| `blocked-on-dependency` | A pinned dependency does not expose what the requirement needs. The dependency and the remedy are named |
| `by-design` | The requirement is not met and will not be. The alternative was weighed and refused, and the trade is recorded |
| `kynos-bench` | Owned by [`getkono/kynos-bench`](https://github.com/getkono/kynos-bench), not by this repository |

`planned` and `needs-tooling` were one status, which made six rows look
blocked on a purchase they were not — a CI grep needs no tool. And every
performance row named `criterion` as something this repository would install,
while the closing section says it deliberately will not; those rows are
`kynos-bench` now, which is where the harness that gives a threshold meaning
already lives.

Currently wired: `cargo-nextest`, `cargo-llvm-cov`, `cargo-hack`, `convco`,
`trybuild`, `proptest`, and rustdoc with `missing_docs = "deny"`. Not yet
present: `cargo-public-api`, `cargo-semver-checks`, `cargo-fuzz`. `criterion` is
not on this list and will not be: benchmarks live in `kynos-bench`.

## Thresholds

Numeric ceilings are written `TBD` until they are measured.

> A threshold is set from the first recorded measurement, never guessed. Setting
> one is a change to this document, reviewed like any other.

A guessed ceiling is worse than no ceiling: it either fails constantly and gets
disabled, or passes trivially and hides the regression it was meant to catch.

## Modules

| Requirement group | Location |
| --- | --- |
| [Document model](#document-model) | `crates/kynos-openapi/` |
| [Routing](#routing) | `crates/kynos/src/router/` |
| [Extraction](#extraction) | `crates/kynos/src/extract/` |
| [Middleware](#middleware) | `crates/kynos/src/middleware/` |
| [Runtime](#runtime) | `crates/kynos/src/server/` |
| [Dependencies](#dependencies) | the whole workspace |
| [Macros](#macros) | `crates/kynos-macros/` |
| [Observability](#observability) | the `trace` feature — no module yet |

## Document model

`crates/kynos-openapi/` is both the IR core and the specification emitter.

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| compatibility | Public API surface is diffed on every change; an addition requires explicit budget approval, a removal fails the build | `cargo-public-api` | `needs-tooling` |
| compatibility | Every release tag is gated on semantic-version correctness | `cargo-semver-checks` | `needs-tooling` |
| correctness | The IR round-trips through serialization losslessly | `proptest` over generated IR values | `enforced`, with two exclusions below, each characterized |
| correctness | Every model type emits the field names and nesting the specification gives it | One exact-JSON case per type in `tests/wire.rs`, counted against the type list | `enforced` |
| correctness | The corpus a downstream generator is built against is the one this build emits | [`tests/conformance_corpus.rs`](../crates/kynos/tests/conformance_corpus.rs), comparing every committed document against a freshly emitted one | `enforced` |
| correctness | Emitted documents validate against both 3.1 and 3.2 validators | CI step over a fixture app covering the full type matrix | `planned` |
| correctness | Emitted documents are byte-deterministic across runs | [`tests/determinism.rs`](../crates/kynos/tests/determinism.rs), emitting one fixture description in three separate processes and byte-comparing | `enforced` |
| correctness | Emitted documents are byte-deterministic across platforms | A cross-OS CI job, which does not exist: every job runs on `ubuntu-latest` | `planned` |
| dx | No public item exposes `Pin`, `BoxFuture` or a tokio type | `cargo-public-api` assertion | `needs-tooling` |
| operability | `--check` mode exits nonzero on drift from the committed document | A binary target, used as a required gate on the framework's own examples | `blocked-on-impl` |
| performance | Generation time and output size scale sub-quadratically in operation count | Measured at 10/100/1000 operations with a fitted-slope assertion | `kynos-bench` |

The `--check` row's blocker was never a `todo!()` body: the workspace declares no
binary target at all, and no command-line surface is designed anywhere. It is
recorded here because the emitter is here, but it may well belong to a
satellite crate. That, and the two observability rows below, are the only three
`blocked-on-impl` rows left — the API-skeleton milestone accounted for the rest,
and each moved to the status its evidence actually supports rather than all of
them moving to `enforced`.

Two shapes are excluded from the round-trip property, and both are real gaps
rather than test convenience:

- **A JSON `null` example does not survive.** The loss is on the way in rather
  than the way out: `Some(Value::Null)` writes `null` faithfully, and
  `Option<Value>` then folds that `null` back into `None` when it is read. It
  costs a parameter's `example`, a schema's `const` and `default`, and the
  other `Option<Value>` fields alike. JSON `null` is a legal example and a
  legal default, so a description that uses one is silently changed. The remedy
  is a double-`Option` deserializer at each site.
- **A `PathItem` carrying both `$ref` and sibling fields loses the siblings.**
  It reads back as a `RefOr::Ref`. Kynos never emits one, but the type permits
  constructing it, so the model can hold a value it cannot write down.

Each is excluded in `tests/support/` so the property stays honest, and asserted
in `tests/wire.rs` so the behaviour is on the record. Without the second half an
exclusion is indistinguishable from an oversight, and closing the gap would turn
no test red — which is the wrong signal for work that fixes something.

The `dx` row currently holds by construction — the crate has no runtime
dependency at all, which is deliberate — but nothing prevents that from
changing, which is why it is listed rather than assumed.

Ordering is `IndexMap`-backed throughout, so determinism is a design property;
the requirement above is that it be *verified*, not merely intended. It is now
verified in one direction and not the other, which is why the row became two.

**Across runs is enforced, and it takes a second process to enforce it.**
`properties.rs`'s `serialization_is_deterministic` serializes one already-built
model `Document` twice, so the registry's `origins`, the router's `index_of` and
the validator's sets are never on its path — it is that weaker statement.
`tests/determinism.rs` re-executes the test binary instead, so each emission
rebuilds and re-walks all three from scratch under a fresh hash seed; a second
call would reuse the same maps and agree with itself trivially. Each is indexed
rather than walked, and that test is what keeps them so.

**Across platforms is not enforced, and saying so is the point of the split.**
Every CI job runs on `ubuntu-latest`. The plausible divergences are a path
separator reaching a component name and a float formatting differently, neither
of which anything here would currently catch.

## Routing

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| performance | Zero heap allocations on the routing path | Counting allocator asserting `alloc_count == 0` across a 10k-request replay, over routes of at most three parameters and no static/dynamic sibling overlap | `planned` |
| performance | Route resolution p99 ≤ TBD at 1000 registered operations | `criterion` with a regression gate | `kynos-bench` |
| reliability | Route conflicts and ambiguity are rejected before the service runs | `trybuild` compile-fail suite for statically expressible conflicts; [`tests/routing.rs`](../crates/kynos/tests/routing.rs) over `Router::validate` for those only visible once the tree is assembled, each refusal with its pass control | `enforced` |
| security | A served asset path is enumerated, never joined from request input | [`tests/assets.rs`](../crates/kynos/tests/assets.rs) asserting an embedded set registers only literal `paths` keys, and [`router/assets/fs/tests.rs`](../crates/kynos/src/router/assets/fs/tests.rs) sweeping every escape a resolver must refuse against a control that must not be | `enforced` |
| correctness | A route with no expressible template is recorded rather than described | [`tests/unchecked.rs`](../crates/kynos/tests/unchecked.rs) asserting a catch-all takes no `paths` key and reaches `x-kynos-opaque-routes` | `enforced` |
| operability | Metric labels derive from operation IDs, never request paths | [`tests/dispatch.rs`](../crates/kynos/tests/dispatch.rs) asserting `MatchedPath` is the template rather than the request target, so two concrete paths under one template produce one label | `enforced` |

The allocation row is scoped rather than absolute because of what the pinned
router actually guarantees; [`routing.md`](routing.md) records the two cases
and why they are reachable through ordinary REST shapes. Widening the scope
later is a measurement, not a rewrite.

## Extraction

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | No extractor panics on any input | `cargo-fuzz` target per extractor, run nightly, corpus committed | `needs-tooling` |
| security | Header count and header-list size are bounded by default | The driver is configured from [`Http1Config`](../crates/kynos/src/server/protocol.rs) and `Http2Config` on every connection; [`server/tests.rs`](../crates/kynos/src/server/tests.rs) asserts the configured cap is the one forwarded | `enforced` |
| security | A body-size limit is available, and once mounted is enforced *and* declared | [`tests/limits.rs`](../crates/kynos/tests/limits.rs) asserting rejection at limit+1, that a declared length past the limit is refused before the body is read, and that a service mounting none neither refuses nor declares a 413 | `enforced`; no default, deliberately, and `planned` for the allocation bound |
| security | Per-IP connection caps | none yet — see below | `planned` |
| correctness | Every Rust type expressible as a handler input has a valid JSON Schema projection | Property test over a macro fixture set, validated against 3.1 and 3.2 validators | `planned` |
| dx | Every rejection produces an error naming the field and the fix | `trybuild` UI tests, plus [`error/rejection/tests.rs`](../crates/kynos/src/error/rejection/tests.rs) counting every variant and asserting each renders a sentence rather than a debug dump | `enforced` for the counting; `planned` for the snapshots |
| security | A credential is read from the field its scheme declared, and from no other | `Carries` is emitted by the same derive as `describe`, so the two are one text; [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) drives a derived API-key carrier to 200, 401 and 403 over a live service | `enforced` |
| security | An authenticator cannot read a request field the scheme did not declare | Structural: `Authenticator::authenticate` receives `S::Presented` and is never given the request | `enforced` |

**There is deliberately no default body cap**, and the row above says so rather
than claiming one. This document previously read "body size, header count and
header size limits are enforced by default"; only the second and third were,
because those are the driver's and a body cap is an interceptor `Router::build`
does not mount. Making one default was rejected for three reasons, any one
sufficient: it would add 413 to every operation of every application that never
asked for one, it would make a user's own `BodySize` a `const` compile error
against `statuses_disjoint`, and it would buffer a length-less body — which is
the streaming upload the limit exists to leave alone. The framework's rule that
configuring a limit and documenting it are one action has a converse, and this
is it.

**The per-IP row has no method, and that is the requirement.** In the common
deployment every connection arrives from the load balancer's address, so a cap
counted in-process is either meaningless or a self-inflicted outage. An honest
one needs a trusted-proxy-header policy — which is authentication-adjacent and
belongs with [`security.md`](security.md) rather than here.

## Middleware

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| correctness | Emitted document ⊇ observable responses | [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) checking live responses against the generated document across the owned-layer matrix, in both directions | `enforced` |
| correctness | Contribution composition is order-sensitive and deterministic | Permuted stacks produce differing, stable documents | `planned` |
| reliability | `Opaque` propagates to every affected operation and omits none | Unit test over a synthetic router tree | `planned` |
| performance | Per-layer added p99 ≤ TBD | `criterion` at stack depth 0/4/8 with a regression gate | `kynos-bench` |
| correctness | A stored response is never served to a request its stored `Vary` does not select | [`middleware/cache/tests.rs`](../crates/kynos/src/middleware/cache/tests.rs) over the selection rules, plus [`tests/cache.rs`](../crates/kynos/tests/cache.rs) over a live sequence | `enforced` |
| correctness | A response that stated no freshness is never reused | [`tests/cache.rs`](../crates/kynos/tests/cache.rs) counting handler calls across three requests | `enforced` |
| correctness | A timeout answers a status the specification defines for an origin server | [`tests/limits.rs`](../crates/kynos/tests/limits.rs) over a live handler past its budget, and [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) against the emitted document | `enforced` |
| security | A credential-bearing header is recorded as present and never by value | [`middleware/trace.rs`](../crates/kynos/src/middleware/trace.rs) sweeping the whole `REDACTED` list, with a control asserting an ordinary header still records its value | `enforced` |
| security | A `__Host-` or `__Secure-` cookie carries what its prefix promises, or does not render | [`response/cookie/tests.rs`](../crates/kynos/src/response/cookie/tests.rs) over both prefixes, the wrong case, and each refusal with a control | `enforced` |
| correctness | A preflight permitting any header still names `authorization`, which no wildcard covers | [`cors/preflight_tests.rs`](../crates/kynos/src/middleware/cors/preflight_tests.rs) over the credentialed and uncredentialed answers | `enforced` |
| correctness | A 304 is minted only from a 200, never from another 2xx | [`tests/cache.rs`](../crates/kynos/tests/cache.rs) over a 204 carrying a matching validator | `enforced` |
| correctness | An `If-None-Match` on an unsafe method answers 412 rather than being ignored | — | `absent`, and recorded in [`middleware.md`](middleware.md): 412 is a status `NotModified` does not declare, and widening `Short` would add it to every covered operation |
| correctness | A non-error response to an unsafe method drops what was stored for that target | [`tests/cache.rs`](../crates/kynos/tests/cache.rs) over a live store-then-write-then-read sequence, with a control asserting a refused write drops nothing, plus a unit case over the status classes section 4.4 defines | `enforced` |
| security | A forwarding field is believed only where the application named the hop that wrote it | [`http/forwarded/tests.rs`](../crates/kynos/src/http/forwarded/tests.rs) over the hop, address and network policies including a forged chain, plus [`tests/rate_limit.rs`](../crates/kynos/tests/rate_limit.rs) asserting an unconfigured service ignores a claimed address | `enforced` |
| security | An unsafe request a browser says came from another site is refused | [`csrf/tests.rs`](../crates/kynos/src/middleware/csrf/tests.rs) over every rule in order, including `Sec-Fetch-Site` winning over a claimed trusted `Origin`, plus a live exchange in [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) | `enforced` |
| correctness | Content-coding negotiation follows section 12.5.3, including the wildcard form of an identity refusal | [`middleware/compression.rs`](../crates/kynos/src/middleware/compression.rs) over a table of every rule the section states | `enforced` |
| security | A response setting a cookie is never stored | `every_refusal_has_a_case` over the whole `Unstorable` set, counted against its variants | `enforced` |
| correctness | Both ways a header group reaches the wire write the same fields | [`response/headers.rs`](../crates/kynos/src/response/headers.rs) asserting the two paths *agree*, rather than asserting each | `enforced` |
| correctness | A re-encoded response states the length it actually sends | [`tests/middleware.rs`](../crates/kynos/tests/middleware.rs) over a handler that set its own length, comparing the stated value against the bytes received | `enforced` |
| correctness | A response carrying a strong validator is never content-coded | [`tests/middleware.rs`](../crates/kynos/tests/middleware.rs)'s `partial` module, with the weakly tagged control differing in exactly the `W/` prefix | `enforced` |
| correctness | An encoded stream decodes to exactly what the handler produced | [`compression/streaming.rs`](../crates/kynos/src/middleware/compression/streaming.rs) round-tripping a multi-frame body through both latency modes, and asserting the two modes differ in frame count and in size | `enforced` |
| correctness | A stored content coding carries a validator of its own, and a range is calculated over the octets that were sent | [`tests/assets.rs`](../crates/kynos/tests/assets.rs) over a fixture holding real `.br`, `.gz` and `.zst` siblings: two representations get two tags, a resume across them is refused, and a 304 answers per representation | `enforced` |
| correctness | A response that advertises `Accept-Ranges` is never content-coded | [`tests/middleware.rs`](../crates/kynos/tests/middleware.rs)'s `partial` for the rule and its control, and `ranged_assets` resuming an asset download against the tag it was served with | `enforced` |
| security | A compressed request body cannot cost more memory than the route's declared limit | [`tests/middleware.rs`](../crates/kynos/tests/middleware.rs)'s `decompression`, refusing a body that passes a limit on its encoded size and expands past the decoded one, with a control inside both bounds | `enforced` |
| correctness | Metadata describing a coded form does not outlive the decode | The same module, asserting `Content-Encoding`, `Content-Length` and `Content-Digest` as the handler receives them, with a control that carried no coding | `enforced` |

**The two decompression rows are one requirement seen twice.** RFC 9110 §8.4
makes the representation *the coded form*, so undoing the coding invalidates
every other statement about it — and a `Content-Length` that survived the decode
is both a lie and, since it is the number a naive cap would read, the mechanism
by which the first row would fail. That is also why
[`BodySize`](../crates/kynos/src/middleware/limits.rs) cannot be the guard here:
it measures the size an attacker sets freely. `Decompression` declares 413 for
that reason, which makes mounting the two together a compile error.

**The `Accept-Ranges` row is a known limit as much as a guarantee, and the limit
is now smaller than it was.** It says a static asset under `Compression` ships
uncompressed, which is a bandwidth cost on exactly the files worth encoding —
but a set whose build pipeline writes `app.js.br` beside `app.js` serves the
encoded form itself, under a validator minted for those octets. What is left is
a set with no stored coding, and a handler minting its own strong tag. It is recorded here rather than left to the
interceptor's documentation because it is a deliberate trade against RFC 9110
§8.8.1 — one strong validator cannot name both the identity file and an encoded
one, and the encoder is downstream of where both the range and the tag are
decided. [`middleware.md`](middleware.md) carries the reasoning and the two
deployments that get the compression back.

The first row is the enforcement of [`middleware.md`](middleware.md), and it
runs: without it the soundness invariant would be an intention rather than a
guarantee. It has already earned its keep twice — see
[`testing.md`](testing.md#what-the-harness-found-on-its-first-run).

## Runtime

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | Graceful shutdown drains all in-flight requests with zero dropped responses | Integration tests in `crates/kynos/src/server/tests.rs` covering HTTP/1 drain, HTTP/2 stream drain, TLS handshake cancellation, and timeout exhaustion | `enforced` |
| reliability | Backpressure is bounded by default via connection count, queue depth and timeouts | [`tests/limits.rs`](../crates/kynos/tests/limits.rs) asserting a request past the concurrency cap is shed with 503 rather than queued; a load test at 2× capacity for the memory bound | `enforced` for the shedding; `planned` for the load test |
| reliability | HTTP/2 request-body flow control is released as the body is consumed, not as frames arrive | Load test streaming a large body to a slow consumer, asserting the receive window closes | `blocked-on-dependency` |
| reliability | A streamed request body is decoded as it arrives rather than after it has been collected | [`extract/body/json_lines/tests.rs`](../crates/kynos/src/extract/body/json_lines/tests.rs) reading a body delivered one frame per byte, and every frame boundary of a fixed body | `enforced` for a body declaring a `Content-Length`; `by-design` under `BodySize` for a chunked one |
| performance | Syscalls per request ≤ TBD | `strace -c` assertion over a fixed request count | `kynos-bench` |
| performance | Idle memory per connection ≤ TBD at 100k connections | Nightly load test measuring RSS delta | `kynos-bench` |
| compatibility | `Listener::Tokio` is the only public item naming a tokio type | `cargo-public-api` assertion over the framework surface | `needs-tooling` |
| compatibility | Every `tokio` mention outside `crates/kynos/src/server/` appears in the allowance table in [`architecture.md`](architecture.md#runtime-policy), and the table has exactly five rows | CI grep for `tokio` outside that module tree, counted against the table | `planned` |

The last two rows are the enforcement of the tokio-only policy in
[`architecture.md`](architecture.md#runtime-policy). There is no runtime
abstraction trait to keep private, so what CI has to check is the opposite:
that direct tokio use stays where it is allowed, and that it reaches users only
through the listener handover it is meant to.

The containment row is written against an enumerated table rather than against
`server/` alone, and that is a correction rather than a loosening: the grep as
originally stated **fails today**, at `middleware/limits.rs` and
`middleware/compression.rs`, and had done since before it was written down.
Counting against a four-row table is checkable in this repository's
exhaustiveness idiom — a fifth site fails the build — where the older sentence
could only ever have been wired by deleting it.

The `blocked-on-dependency` row is the one requirement a pinned dependency
prevents rather than delays: hyper releases HTTP/2 flow-control capacity when a
frame is polled rather than when the body is consumed, so the receive window
never closes on a slow consumer. Driving `h2` directly is the only remedy, and
[`architecture.md`](architecture.md#dependencies) records why that trade is not
taken today.

The streaming row above it is the same family of fact one layer up, and it is
split because half of it holds. A request declaring a `Content-Length` is
decided from the head and its body passes
[`BodySize`](../crates/kynos/src/middleware/limits.rs) untouched, so
`Records<T>` receives it a frame at a time. A chunked request declares no
length, so a running count is the only bound there is and the interceptor
materialises the whole body before the handler is entered — records still
arrive one at a time, and nothing is saved.

What blocks it is the declared 413, not the erased body. A count that runs
while the handler reads only reaches its verdict once the handler has acted on
the bytes it was given, so streaming the body does not buy the cap back — it
moves the refusal behind whatever the handler already did with an oversized
payload. The two honest alternatives are worse than the buffer: a 413 sent
after those side effects, or a 411 refusing every length-less body and with it
every chunked upload. That is why the row reads `by-design` and not
`blocked-on-impl`; there is no missing constructor to write. The limit is
documented where the decision is made rather than only here, because that is
where someone mounting a cap will meet it.

## Dependencies

Containment rows enforcing the graph in
[`architecture.md`](architecture.md#dependencies). Each is a grep, and each
fails the build when a crate is named outside the module that owns it.

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| compatibility | `hyper` and `hyper-util` are named only in `server/connection.rs` and `http/body.rs` | CI grep over `crates/*/src`, excluding test modules | `planned` |
| compatibility | `tokio-rustls` and `rustls` are named only under `server/tls/` | CI grep over `crates/*/src`, excluding test modules | `planned` |
| compatibility | `matchit` may be named only under `router/` | CI grep over `crates/*/src` | `planned` |
| compatibility | `h2` and `httparse` are never named | CI grep over `crates/*/src`, allowing the `b"h2"` ALPN identifier | `planned` |
| compatibility | `tower` and `tower-service` are named only in `unchecked.rs` | CI grep over `crates/*/src` | `planned` |
| dx | Every crate in `[workspace.dependencies]` is consumed by a member | `cargo-udeps` or an equivalent manifest check | `needs-tooling` |

The last row now passes: `mime` and `pin-project-lite` are gone, `trybuild` and
`proptest` have consumers, and the codec crates `crates/kynos` declares without
yet naming are each behind an off-by-default feature.
[`architecture.md`](architecture.md#dependencies) lists them.

**Deferred by decision, not by oversight.** The eight containment greps above
and in [Runtime](#runtime) need no tool this repository lacks, and
`cargo-public-api` is the single highest-leverage addition on the tooling
list. Both were held back from the API-freeze push so that a committed
surface baseline is recorded against a surface that has stopped moving.

## Macros

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| dx | No diagnostic names an internal type | Exhaustive `trybuild` UI snapshot suite, reviewed on every change | `enforced` |
| dx | Incremental rebuild after a one-line handler edit ≤ TBD at 100 operations | `cargo build --timings` in CI with a trend gate | `planned` |
| dx | `cargo expand` output compiles standalone and is human-readable | CI test compiling expanded fixtures directly | `planned` |
| reliability | All diagnostics carry user spans | UI tests asserting error locations point into user source | `enforced` |

Compile time needs a trend line rather than a spot check: it is the failure mode
that kills macro-heavy type-level frameworks, and it degrades gradually enough
that no single change ever looks responsible.

## Observability

No module exists yet; the `trace` feature is a facade over `tracing` and the
subscriber stays the application's.

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| operability | Spans conform to OpenTelemetry HTTP semantic conventions | Integration test asserting attribute names against a pinned semconv version | `blocked-on-impl` |
| operability | Metric cardinality is bounded by operation count | Test asserting series count is invariant under 10k distinct request paths | `blocked-on-impl` |
| operability | A response the client did not receive is distinguishable from one it did | [`tests/sse.rs`](../crates/kynos/tests/sse.rs) dropping a live event stream's reader and asserting `on_disconnect` fires exactly once, with a control that reads a finite response to its end | `enforced` |

Both `blocked-on-impl` rows are blocked on the same thing — the module does not
exist — so neither is waiting on tooling. What did land is the seam they need:
`Observer` receives the matched [`Route`](../crates/kynos/src/router/operation.rs),
so a label can be keyed by operation rather than by request path, which is the
property the second row measures.

The third row is the one that was not simply missing but wrong. `on_response`
fires when the response head is ready, which for a stream or a download is
nowhere near when the peer has it — so a service that counted responses counted
deliveries it never made, and its latency figure measured producing a response
rather than sending one. `on_disconnect` reports the body dropped before its
last frame, which is the difference. It does not report a client that leaves
while the handler is still working: there is no body to drop until the handler
has produced one.
The module is not merely unwritten, though. Shipping one would put this
framework's release schedule in front of the OpenTelemetry project's, and would
fix by fiat what a span is called, which attributes it carries, which
semantic-convention version it targets and what a `traceparent` from an
untrusted caller is allowed to do — each an operator's decision.
[`examples/opentelemetry.rs`](../crates/kynos/examples/opentelemetry.rs) is the
answer instead: an interceptor whose `Reads` group is both the declaration and
the W3C propagation carrier, entered across `next.run` so the span covers the
handler. It carries no dependency into the library, and both rows above stay
open against a `kynos-otel` that may never be written.

## Workspace

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | The declared MSRV builds | `mise run msrv:check`, dedicated CI job | `enforced` |
| reliability | Every reachable feature combination compiles | `mise run features:check` (`cargo hack --feature-powerset`) | `enforced` |
| reliability | Every test target compiles and runs at baseline features, not only `--all-features` | `mise run test:baseline` | `enforced` |
| reliability | Tests are hermetic; no shared state, no ordering dependence, no retries | `cargo-nextest` process isolation, `retries = 0`, guarded by `crates/kynos/tests/hermeticity.rs` | `enforced` |
| reliability | Panic recovery refuses to compile under `panic = "abort"` | `mise run panic:check` | `enforced` |
| reliability | Commits follow Conventional Commits | `convco`, via git hook and CI | `enforced` |
| compatibility | Every hand-rolled `Stream` implementation is private, except the one row in [`architecture.md`](architecture.md#public-api-surface), and there are exactly three of them | CI grep for `Stream for` over `crates/*/src`, excluding test modules, counted against the table and the two private sites its prose names | `planned` |
| dx | Every public item is documented | `missing_docs = "deny"` plus `mise run docs:check` | `enforced` |
| dx | Every public item has a compiling doc example | Doctests already run via `mise run test:doc`; *presence* of an example per item is unenforced | `planned` |
| compatibility | Public API item count is tracked as a budget | `cargo-public-api` count with a committed baseline | `needs-tooling` |
| performance | The benchmark suite runs nightly with regression alerting | `kynos-bench`, so erosion surfaces as a trend rather than at release | `kynos-bench` |

## Tooling gaps

Five crates stand between this document and its enforcement. Roughly in order of
what they unblock:

| Tool | Unblocks | Notes |
| --- | --- | --- |
| `cargo-public-api` | Four `compatibility`/`dx` rows across the document model and runtime | The single highest-leverage addition: it enforces the architecture policy mechanically rather than by review |
| `trybuild` | Compile-fail and UI rows in routing, extraction and macros | Already in `[workspace.dependencies]`; needs only a consumer |
| `proptest` | IR round-tripping, schema projection, the conformance harness | |
| `cargo-fuzz` | Extractor panic-freedom | Needs a committed corpus and a nightly job |
| `cargo-semver-checks` | Release-tag gating | Meaningful now that the surface is implemented; needs a release tag to compare against |

`criterion` is intentionally absent from this list. Benchmarks live in
[`getkono/kynos-bench`](https://github.com/getkono/kynos-bench) along with the
methodology defining every performance row above, because a threshold is
meaningless apart from the harness that produced it.
