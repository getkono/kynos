# Performance

What a feature costs the request path, how that cost is counted, and which
measurement a given shape of code owes. [`nfr.md`](nfr.md) records which of
these run today; this document is about the method.

Every section except [Rationale](#rationale) states a rule that binds
implementation work. One of the five kinds below runs today and the rest do
not, and the [taxonomy](#the-taxonomy)'s last column is where that is admitted
rather than implied.

The one that runs has already earned the document: the routing path was
required to allocate nothing, had never been measured, and allocates seven
times for a static match. [`nfr.md`](nfr.md#routing) carries the numbers and
what they do and do not establish.

## The boundary

Two repositories measure Kynos, and the split is by what the question *names*.

**This repository owns what names Kynos.** A feature flag, the interceptor
stack, the router's own tables — no other library has these, so no comparison
answers them.

**[`kynos-bench`](https://github.com/getkono/kynos-bench) owns what any HTTP
server library would answer.** Throughput, tail latency, resident memory at
scale. Such a number is worth reading only beside the same number from another
framework, which is the harness's whole purpose.

The axis is *specific versus general*, not performance versus correctness. It
decides each row in [`nfr.md`](nfr.md)'s `performance` category, and it moved
three of them: document generation cost, route resolution at a thousand
operations, and per-layer overhead each name something only Kynos has.

**Everything this repository measures is counted rather than timed**, and that
follows from the boundary rather than qualifying it. A question specific to
Kynos is a question about structure — how many allocations, how wide a future,
how much monomorphized IR — and structure is countable exactly. A wall-clock
figure on a shared `ubuntu-latest` runner measures the runner, which is why
[`nfr.md`](nfr.md#tooling-gaps)'s exclusion of `criterion` still holds. A gate
that cannot fail honestly is worse than no gate.

## The taxonomy

What each kind of measurement proves that no other kind does.

| Kind | Lives in | Runs under | Proves | Status |
| --- | --- | --- | --- | --- |
| Allocation count | its own integration target | `cargo nextest`, over `stats_alloc` | that a path allocates a bounded number of times | in use, at [`tests/alloc.rs`](../crates/kynos/tests/alloc.rs) |
| Size guard | [`tests/size.rs`](../crates/kynos/tests/size.rs), or a sibling `tests.rs` | `cargo nextest` | that a type or a future did not grow | in use for types; `planned` for futures |
| Off-path proof | a sibling `tests.rs` | `cargo nextest` | that a feature is unreachable from the request path | `planned` |
| Codegen delta | a feature sweep | `cargo llvm-lines` | what a feature costs in monomorphized IR | `needs-tooling`; `cargo-llvm-lines` is not installed |
| Binary delta | a feature sweep | `.text` of a fixed fixture | what a feature costs a linked artifact | `planned` |

**An allocation count needs its own target because a global allocator is
process-wide.** Installing one in the library's unit-test binary would perturb
every other unit test in it, so the counter cannot live in a sibling `tests.rs`
however much the feature it measures does. Nothing else here has that problem:
a size guard and an off-path proof are ordinary assertions and belong beside
the code they constrain.

**The counter is a dependency because `unsafe_code = "forbid"` is not liftable
by an `#[allow]`.** A `GlobalAlloc` implementation is `unsafe impl`, so this
workspace cannot write one anywhere;
[`architecture.md`](architecture.md#dependencies) records why taking a vetted
one keeps that invariant rather than bending it, and why the crate that
installs no allocator on its own behalf was the one worth taking.

The two sweep kinds are not tests. They build the same fixture at each feature
and compare artifacts, which no test harness can express, so they are a task
rather than a target and their baselines are committed files.

## The allocation

The taxonomy says what each kind proves. This says which kind a building block
owes — and, in the last column, which it does not. That column is what keeps
the method affordable, exactly as it does in
[`testing.md`](testing.md#the-allocation): without it, "measure the cost" reads
as "measure every way you can think of", and the shape nobody happened to think
about gets nothing.

Six shapes account for the routing stack.

| Shape | Recognised by | Owes | Does not owe |
| --- | --- | --- | --- |
| Type-level surface | a bound, a `const fn`, a phantom stack, a derive | a codegen delta | any runtime measurement — nothing runs |
| Per-request path element | it runs inside `Dispatch::serve` for every request | an allocation count over a replay, and a future-size guard | a timing figure |
| Per-layer element | an `Interceptor` in the erased chain | an allocation and future-size delta at stack depth 0/4/8 | a per-layer latency |
| Per-connection element | [`server/`](../crates/kynos/src/server/), TLS, the protocol configs | a size guard on per-connection state | per-request attribution; resident memory at scale is `kynos-bench` |
| Off-path element | the document model, the emitters, the validators, `describe` | a proof it is unreachable from the request path, and a binary delta | any per-request measurement |
| Opt-in payload codec | a body extractor or response codec behind a feature | a binary delta, and an allocation count on an operation that names it | a measurement on a route that never mounts it |

**A type-level surface owes a codegen delta and nothing else, for the same
reason [`testing.md`](testing.md#the-allocation) says it does not owe running.**
`CompatibleWith`, `statuses_disjoint` and the `Cons`/`Both` stacks are compared
during type checking and are absent from the binary, so a request never reaches
them. What they can cost is compile time and monomorphized IR, and a framework
whose declarations are types is exactly where that bill arrives.

**A per-layer element is measured as a delta, never as a total.** Each
interceptor is one `Arc<dyn ErasedInterceptor<C>>` indirection, so a stack's
cost is a function of its depth; a single figure for "the middleware" would
describe one arrangement and no other. Depths 0, 4 and 8 are the same three
points [`nfr.md`](nfr.md#middleware) already names.

**An off-path element owes a proof, which is the measurement.** The claim is
that the cost is zero, and zero is not something a counter can report
convincingly — a replay that never exercised the feature also counts nothing.
What settles it is reachability: the emitted `Document` is built once in
`Router::build` and read back only through `Service::openapi`, so nothing in
`Dispatch::serve` touches it. That is checkable, and until it is checked the
[README](../README.md)'s claim that there is no JSON Schema interpreter on the
hot path rests on reading the code.

**An opt-in codec is measured on a route that mounts it.** Measuring `json`
against an operation with no body would report zero and mean nothing. The
question the shape exists to answer is what the codec costs the operation that
asked for it, against the same operation without it.

## The feature grading

Every flag `crates/kynos` declares appears in exactly one column below. The
point of listing all of them is that a feature nobody examined must be
distinguishable from a feature examined and found free — the failure
[`testing.md`](testing.md#the-allocation) names, and the one a coverage number
cannot show.

| Grade | Owes | Flags |
| --- | --- | --- |
| Full battery | everything its shape owes above | `server`, `http1`, `http2`, `tls`, `json`, `form`, `multipart`, `protobuf`, `compression`, `cache`, `cookie`, `assets`, `assets-fs`, `docs`, `unchecked`, `openapi32`, `trace` |
| Off-path proof | a proof it is unreachable from the request path, and a binary delta | `openapi31`, `macros`, `yaml`, `test-util`, `uuid`, `time`, `time-chrono`, `time-jiff`, `decimal`, `decimal-rust`, `decimal-big` |
| Aggregate | nothing of its own; it is the union of what it enables | `default`, `full` |

**The grading is graded because the batteries cost different amounts.** A
scalar format whose whole contribution is a JSON Schema `format` cannot reach
`Dispatch::serve`, and spending an allocation replay on it would buy a zero
already implied by its shape. What it still owes is the proof of that, because
"cannot reach" is a claim about code rather than about intent.

**The table is not yet counted against the manifest.** Exhaustiveness here is
intended rather than asserted, against what
[`testing.md`](testing.md#cross-cutting) asks of the rest — a flag added to
`Cargo.toml` and not to this table fails nothing today. Closing that is a
[`containment:check`](../scripts/containment.py)-shaped job: read the table,
read `[features]`, and fail when the two part company.

## Thresholds

[`nfr.md`](nfr.md#thresholds) governs every number this method produces, and is
not restated here. A ceiling is set from the first recorded measurement and
never guessed.

Two consequences are worth stating where the measuring happens:

**A first measurement that disappoints is recorded, not fixed.** If the routing
path allocates where the requirement says it should not, the honest move is a
ceiling at the measured value and a characterization of the gap — per
[`testing.md`](testing.md#cross-cutting), an exclusion pairs with a test
asserting what happens today. Folding a fix into the change that first measured
something means the measurement was never reviewed on its own.

**Relations outlive absolutes.**
[`tests/size.rs`](../crates/kynos/tests/size.rs) is the shape to copy: it
asserts that `Error` is smaller than `Violation` and only loosely that it is
under 64 bytes, because the relation is the design property and the absolute is
a ratchet. A cost gate written as a tight absolute fails on an unrelated
toolchain bump and gets disabled.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why this is not in `kynos-bench`

The obvious filing puts everything with a number in it beside the harness, and
[`README.md`](README.md) said so until this document existed. What that misses
is that most of these questions have no comparison to make. "What does mounting
`Compression` add to an operation" is answerable against Kynos without another
framework in the room, and unanswerable with one, because no other library has
`Compression`. Sending it to a comparative harness would leave it unmeasured in
both places, which is where it has been.

The converse holds too, and is why the boundary is worth writing down rather
than merely observing. Throughput is meaningless here: a figure produced on a
CI runner shared with a linker says nothing about the framework, and the only
thing that would make it say something is the same figure from another server
on the same machine.

### Why the method is counted rather than timed

A timing gate on this repository's CI would have to be loose enough to survive
a noisy runner, and a gate that loose passes through the regressions it exists
to catch. [`nfr.md`](nfr.md#thresholds) already argues this about guessed
ceilings; a timed ceiling on shared hardware is a guessed ceiling with extra
steps.

Counting avoids the trade rather than splitting it. An allocation count is the
same integer on a loaded machine and an idle one, so the gate can be exact, and
an exact gate fails on the change that caused it rather than three merges later.
`[profile.bench]` in the workspace manifest keeps debug info under optimization
for anyone who does want a profile locally — that affordance already exists and
this document does not replace it.

### Why the measurements live beside the features

Everything in [`testing.md`](testing.md) is filed by the code it constrains, and
a cost assertion is not different in kind from a correctness one. A central
cost target would collect assertions about modules whose authors never see
them, and the first time one failed, the person who broke it would be reading a
file they had no reason to know existed.

The allocation counter is the exception, and it is a mechanical one: a global
allocator is process-wide, so the counter has to own its process. That is a
fact about Rust rather than a judgement about where the assertion belongs.
