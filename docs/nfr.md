# Non-Functional Requirements

What each part of Kynos must prove, how it is proven, and whether that proof
runs today. The last column is the honest one: this document is worth keeping
only if it never claims a guarantee CI does not actually enforce.

## Status

| Status | Meaning |
| --- | --- |
| `enforced` | A CI job runs this today. Regressions fail the build |
| `planned` | The requirement is settled; the tooling is not installed. The tool to add is named |
| `blocked-on-impl` | The surface is still `todo!()`-bodied, so there is nothing to assert against yet |

Currently wired: `cargo-nextest`, `cargo-llvm-cov`, `cargo-hack`, `convco`, and
rustdoc with `missing_docs = "deny"`. Not yet present: `cargo-public-api`,
`cargo-semver-checks`, `cargo-fuzz`, `proptest`, `criterion`. `trybuild` sits in
`[workspace.dependencies]` and is consumed by nothing.

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
| [Macros](#macros) | `crates/kynos-macros/` |
| [Observability](#observability) | the `trace` feature — no module yet |

## Document model

`crates/kynos-openapi/` is both the IR core and the specification emitter.

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| compatibility | Public API surface is diffed on every change; an addition requires explicit budget approval, a removal fails the build | `cargo-public-api` | `planned` |
| compatibility | Every release tag is gated on semantic-version correctness | `cargo-semver-checks` | `planned` |
| correctness | The IR round-trips through serialization losslessly | `proptest` over generated IR values | `planned` |
| correctness | Emitted documents validate against both 3.1 and 3.2 validators | CI step over a fixture app covering the full type matrix | `planned` |
| correctness | Emitted documents are byte-deterministic across runs and platforms | CI comparing repeated generation and cross-OS builds | `planned` |
| dx | No public item exposes `Pin`, `BoxFuture` or a tokio type | `cargo-public-api` assertion | `planned` |
| operability | `--check` mode exits nonzero on drift from the committed document | Used as a required gate on the framework's own examples | `blocked-on-impl` |
| performance | Generation time and output size scale sub-quadratically in operation count | Measured at 10/100/1000 operations with a fitted-slope assertion | `planned` |

The `dx` row currently holds by construction — the crate has no runtime
dependency at all, which is deliberate — but nothing prevents that from
changing, which is why it is listed rather than assumed.

Ordering is `IndexMap`-backed throughout, so determinism is a design property;
the requirement above is that it be *verified*, not merely intended.

## Routing

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| performance | Zero heap allocations on the routing path | Counting allocator asserting `alloc_count == 0` across a 10k-request replay | `blocked-on-impl` |
| performance | Route resolution p99 ≤ TBD at 1000 registered operations | `criterion` with a regression gate | `blocked-on-impl` |
| reliability | Route conflicts and ambiguity are rejected before the service runs | `trybuild` compile-fail suite for statically expressible conflicts; `Router::validate` for those only visible once the tree is assembled | `blocked-on-impl` |
| operability | Metric labels derive from operation IDs, never request paths | Unit test asserting label cardinality is constant under adversarial path input | `blocked-on-impl` |

## Extraction

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | No extractor panics on any input | `cargo-fuzz` target per extractor, run nightly, corpus committed | `blocked-on-impl` |
| security | Body size, header count and header size limits are enforced by default | Rejection at limit+1 with no allocation proportional to input size | `blocked-on-impl` |
| correctness | Every Rust type expressible as a handler input has a valid JSON Schema projection | Property test over a macro fixture set, validated against 3.1 and 3.2 validators | `blocked-on-impl` |
| dx | Every rejection produces an error naming the field and the fix | `trybuild` UI tests plus snapshot tests on runtime error bodies | `blocked-on-impl` |

## Middleware

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| correctness | Emitted document ⊇ observable responses | Conformance harness property-testing live responses against the generated document across the owned-layer matrix | `blocked-on-impl` |
| correctness | Contribution composition is order-sensitive and deterministic | Permuted stacks produce differing, stable documents | `blocked-on-impl` |
| reliability | `Opaque` propagates to every affected operation and omits none | Unit test over a synthetic router tree | `blocked-on-impl` |
| performance | Per-layer added p99 ≤ TBD | `criterion` at stack depth 0/4/8 with a regression gate | `planned` |

The first and third rows are the enforcement of
[`middleware.md`](middleware.md). Until they run, the soundness invariant is an
intention rather than a guarantee.

## Runtime

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | Graceful shutdown drains all in-flight requests with zero dropped responses | Integration tests in `crates/kynos/src/server/tests.rs` covering HTTP/1 drain, HTTP/2 stream drain, TLS handshake cancellation, and timeout exhaustion | `enforced` |
| reliability | Backpressure is bounded by default via queue depth and timeouts | Load test at 2× capacity asserting bounded memory and shed responses rather than unbounded growth | `blocked-on-impl` |
| performance | Syscalls per request ≤ TBD | `strace -c` assertion over a fixed request count | `planned` |
| performance | Idle memory per connection ≤ TBD at 100k connections | Nightly load test measuring RSS delta | `planned` |
| compatibility | `Listener::Tokio` is the only public item naming a tokio type | `cargo-public-api` assertion over the framework surface | `planned` |
| compatibility | The runtime is named under `crates/kynos/src/server/` and nowhere else | CI grep for `tokio` outside that module tree | `planned` |

The last two rows are the enforcement of the tokio-only policy in
[`architecture.md`](architecture.md#runtime-policy). There is no runtime
abstraction trait to keep private, so what CI has to check is the opposite:
that direct tokio use stays inside the one module that is allowed to have it,
and that it reaches users only through the listener handover it is meant to.

## Macros

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| dx | No diagnostic names an internal type | Exhaustive `trybuild` UI snapshot suite, reviewed on every change | `planned` |
| dx | Incremental rebuild after a one-line handler edit ≤ TBD at 100 operations | `cargo build --timings` in CI with a trend gate | `planned` |
| dx | `cargo expand` output compiles standalone and is human-readable | CI test compiling expanded fixtures directly | `planned` |
| reliability | All diagnostics carry user spans | UI tests asserting error locations point into user source | `planned` |

Compile time needs a trend line rather than a spot check: it is the failure mode
that kills macro-heavy type-level frameworks, and it degrades gradually enough
that no single change ever looks responsible.

## Observability

No module exists yet; the `trace` feature is a facade over `tracing` and the
subscriber stays the application's.

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| operability | Spans conform to OpenTelemetry HTTP semantic conventions | Integration test asserting attribute names against a pinned semconv version | `planned` |
| operability | Metric cardinality is bounded by operation count | Test asserting series count is invariant under 10k distinct request paths | `blocked-on-impl` |

## Workspace

| Category | Requirement | Method | Status |
| --- | --- | --- | --- |
| reliability | The declared MSRV builds | `mise run msrv:check`, dedicated CI job | `enforced` |
| reliability | Every reachable feature combination compiles | `mise run features:check` (`cargo hack --feature-powerset`) | `enforced` |
| reliability | Tests are hermetic; no shared state, no ordering dependence, no retries | `cargo-nextest` process isolation, `retries = 0`, guarded by `crates/kynos/tests/hermeticity.rs` | `enforced` |
| reliability | Panic recovery refuses to compile under `panic = "abort"` | `mise run panic:check` | `enforced` |
| reliability | Commits follow Conventional Commits | `convco`, via git hook and CI | `enforced` |
| dx | Every public item is documented | `missing_docs = "deny"` plus `mise run docs:check` | `enforced` |
| dx | Every public item has a compiling doc example | Doctests already run via `mise run test:doc`; *presence* of an example per item is unenforced | `planned` |
| compatibility | Public API item count is tracked as a budget | `cargo-public-api` count with a committed baseline | `planned` |
| performance | The benchmark suite runs nightly with regression alerting | `kynos-bench`, so erosion surfaces as a trend rather than at release | `planned` |

## Tooling gaps

Five crates stand between this document and its enforcement. Roughly in order of
what they unblock:

| Tool | Unblocks | Notes |
| --- | --- | --- |
| `cargo-public-api` | Four `compatibility`/`dx` rows across the document model and runtime | The single highest-leverage addition: it enforces the architecture policy mechanically rather than by review |
| `trybuild` | Compile-fail and UI rows in routing, extraction and macros | Already in `[workspace.dependencies]`; needs only a consumer |
| `proptest` | IR round-tripping, schema projection, the conformance harness | |
| `cargo-fuzz` | Extractor panic-freedom | Needs a committed corpus and a nightly job |
| `cargo-semver-checks` | Release-tag gating | Only meaningful once the API surface stops being `todo!()`-bodied |

`criterion` is intentionally absent from this list. Benchmarks live in
[`getkono/kynos-bench`](https://github.com/getkono/kynos-bench) along with the
methodology defining every performance row above, because a threshold is
meaningless apart from the harness that produced it.
