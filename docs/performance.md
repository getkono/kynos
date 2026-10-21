# Performance

What a feature costs the request path, how that cost is counted, and which
measurement a given shape of code owes. [`nfr.md`](nfr.md) records which of
these run today; this document is about the method.

Every section except [Rationale](#rationale) states a rule that binds
implementation work. All six of the kinds below run today, five of them only
for part of what they cover, and the [taxonomy](#the-taxonomy)'s last column is
where that is admitted rather than implied.

[`containment:check`](../scripts/containment.py) holds the first of those two
numbers against the Status column's `in use` cells, so rewording the sentence
means editing the rule in the same commit. The second is a concession each cell
makes in prose, with no token a rule could count, so it is recounted by hand
when the taxonomy moves.

[`nfr.md`](nfr.md#routing) carries what the allocation count measures on the
routing path and what it does and does not establish.

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
| Allocation count | its own integration target | `cargo nextest`, over `alloc_counter` | that a path allocates a bounded number of times | in use, at four targets: [`kynos/tests/alloc.rs`](../crates/kynos/tests/alloc.rs) for the routing path and for why a response's header map is grown rather than presized, [`kynos/tests/alloc_body.rs`](../crates/kynos/tests/alloc_body.rs) for body erasure, [`kynos/tests/alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) for what a payload codec adds, and [`kynos-openapi/tests/alloc.rs`](../crates/kynos-openapi/tests/alloc.rs) for what producing a description costs at 10, 100 and 1000 operations. Several rather than one because an integration binary cannot be depended on, so a second crate that counts cannot reach the first one's harness |
| Size guard | [`tests/size.rs`](../crates/kynos/tests/size.rs), or a sibling `tests.rs`, or beside the count that shares its fixture | `cargo nextest` | that a type or a future did not grow | in use for types, and for the futures every request carries: the one `Service::call` returns and an endpoint's at [`tests/alloc.rs`](../crates/kynos/tests/alloc.rs), and the one that call boxes at [`router/dispatch/tests.rs`](../crates/kynos/src/router/dispatch/tests.rs), since only the crate can name it. A guard outside a box sees only the pointer to it |
| Off-path proof | a sibling `tests.rs`, and a table [`containment:check`](../scripts/containment.py) reads | `python3 scripts/containment.py`, `cargo nextest` | that a feature is unreachable from the request path | in use, for the off-path flags and for the document, the registry, the validators and `jsonschema`: a table in [`testing.md`](testing.md#the-off-path-proof) held by `mise run containment:check`, plus the field witness in [`router/dispatch/tests.rs`](../crates/kynos/src/router/dispatch/tests.rs); `describe` is the one off-path shape no row holds, and the emitters are held by the `yaml` flag's row rather than by one of their own |
| Codegen delta | a feature sweep | `cargo llvm-lines` | what a feature costs in monomorphized IR | in use, via `mise run cost:features` over [`cost/fixture.rs`](../crates/kynos/cost/fixture.rs); reports a trend once master settles, refuses a release whose numbers are not the recorded ones, and sets no ceiling, and sees the generics that fixture instantiates rather than the whole surface — so a feature that grows the dependency graph can shrink this number by sharing instantiations out of upstream rlibs, and a negative row is a relocation rather than a saving |
| Binary delta | a feature sweep | `.text` of a fixed fixture | what a feature costs a linked artifact | in use, over [`cost/binary.tsv`](../crates/kynos/cost/binary.tsv); reports a trend once master settles, refuses a release whose numbers are not the recorded ones, and sets no ceiling. The fixture uses none of these features, so a zero row says the linker stripped — or the collector never instantiated — what nothing called, rather than that the feature is free to a program that uses it. A second sweep answers the question that fixture structurally cannot, over [`cost/codec.tsv`](../crates/kynos/cost/codec.tsv): what mounting a codec costs, weighed on a fixture that mounts one |
| Instruction count | [`kynos-profile`](../crates/kynos-profile/), an unpublished member | `mise run profile:requests`: gungraun over Callgrind and DHAT, read by [`profile_report.py`](../scripts/profile_report.py) | what a request of each [`kynos-bench`](https://github.com/getkono/kynos-bench) scenario executes between a built request and a drained response, once the process has served one — instructions in the program object, and heap blocks and bytes — and which functions they were spent in; what one variable costs, as a scenario's difference from the *base* it differs from in that variable alone; and, in a compression sweep of Kynos's own, what an encode adds at each body size, which [`middleware.md`](middleware.md) reads `min_size`'s default from | in use, in process and, for the `connection_*` groups, through `Server::serve_io` over an in-memory duplex, so hyper's parse and per-connection state are on the measured path and the socket is not. Every request-path feature is built; a scenario is driven by one poll or, where it needs timers or wakeups, by a current-thread runtime. Recorded in [`requests.tsv`](../crates/kynos-profile/requests.tsv); held exact across `--repeat` runs except in a group marked `trend`; reports a trend once master settles and sets no ceiling |

**An allocation count needs its own target because a global allocator is
process-wide.** Installing one in the library's unit-test binary would perturb
every other unit test in it, so the counter cannot live in a sibling `tests.rs`
however much the feature it measures does. Nothing else here has that problem:
a size guard and an off-path proof's field witness are ordinary assertions and
belong beside the code they constrain.

**An off-path proof has two halves, because a request path is not a set of
files.** The witness sits beside the code and pins what a request can reach
through the dispatch table, so a new field is a compile failure until someone
argues for it. The naming rule in [`containment.py`](../scripts/containment.py)
reads [`testing.md`](testing.md#the-off-path-proof)'s table off disk and counts
it against the source, so a site that names an off-path element and is in no
row is a failing build until someone adds it and says why a request cannot
reach it. Neither half carries the proof alone: the witness cannot see a
mention of the document model in a file it never names, and the rule cannot see
past an erased `dyn` boundary into the callee behind it.

**The counter is a dependency because `unsafe_code = "forbid"` is not liftable
by an `#[allow]`.** A `GlobalAlloc` implementation is `unsafe impl`, so this
workspace cannot write one anywhere;
[`architecture.md`](architecture.md#dependencies) records why taking a vetted
one keeps that invariant rather than bending it, and why the crate that
installs no allocator on its own behalf was the one worth taking.

The two sweep kinds are not tests. They build the same fixture at each feature
and compare artifacts, which no test harness can express, so they are a task —
`mise run cost:features`, over [`cost/fixture.rs`](../crates/kynos/cost/fixture.rs)
— rather than a target, and their baselines are the committed
[`cost/binary.tsv`](../crates/kynos/cost/binary.tsv) and
[`cost/codegen.tsv`](../crates/kynos/cost/codegen.tsv), which
`mise run cost:record` writes.

**A sweep is compared at a release, not at a pull request.** It is some fifty
builds, so no pull request waits on it: `full.yml` runs it as a trend once
master settles after a merge, and on the release pull request with
`KYNOS_COST=check`, which fails unless recording would change nothing.
Re-recording in a pull request of its own is then where a release's cost is
reviewed, and it leaves each release tag carrying its own numbers, which is what
the report's since-release section reads back. The comparison covers each delta
and, under the recording toolchain, the baseline point's absolute — the floor a
change that costs every program alike moves while leaving every delta still.

**The codec sweep is the same task over a fixture that varies on purpose.**
[`cost/codec.rs`](../crates/kynos/cost/codec.rs) mounts one operation each way
per codec behind that codec's flag, over a transport floor mounted at every
point, and its baseline row is that floor alone. Holding the program fixed is
what makes `fixture.rs` attribute a delta to a feature; here the program is
*meant* to differ, because "the operation that mounts the codec" is the thing
being weighed and it cannot exist in a build with the flag off. The two fixtures
therefore answer opposite questions and neither subsumes the other, which the
two files' `compression` rows show side by side.
`mise run cost:codecs` runs this half alone, as `mise run cost:codegen` runs
the monomorphized-IR half of `fixture.rs`'s sweep alone; `mise run cost:record`
writes their baselines with the binary one.

**An instruction count is exact only in the program object, so that is the
number recorded.** Callgrind's total for one request moves between runs of one
binary, and every instruction that moves is glibc's: `malloc`'s cost depends on
the heap's history, and `memcpy`'s on where the allocator put the buffers.
[`profile_report.py`](../scripts/profile_report.py) therefore splits each count
by object, records what ran in the benchmark binary — Kynos and every Rust
dependency, statically linked — and reports what ran in libc beside it without
comparing it. The allocator's work is not dropped by that: DHAT's block and byte
counts depend on what was asked for rather than on how it was found, and both
are recorded. The program count is exact per host rather than everywhere:
`memchr`, which `serde_json` scans with, picks its routines from the CPU it
finds. So the benchmark serves one request before the measured one, which moves
the one-time detection out of the region, and the baseline names the host it was
taken on so that a report from another one says so.

**It is held to the allocation count rather than trusted alongside it.** Five of
its benchmarks are the shapes and stacks
[`tests/alloc.rs`](../crates/kynos/tests/alloc.rs) counts with `alloc_counter`,
and the report reads that file's tables and fails unless DHAT's block count for
each is the same number. That check earned its place on its first run: gungraun
finds the region by looking for the benchmark in each allocation's backtrace,
DHAT keeps twelve frames by default, and eight interceptors allocate deeper than
that — so an eight-layer stack read *fewer* blocks than no stack at all, with no
error anywhere. Equality is what catches that, where a ceiling would not, and it
has a cost worth knowing before it arrives: for those five rows `alloc.rs`'s
ceilings become exact. A change that removes an allocation lowers the ceiling in
the same change, or the profile job on `master` reports the disagreement after
the merge.

**The run of record is CI's.** The committed baseline names the host it was
taken on, and a report from another host says so. The job uploads each run in
the baseline's format, so recording the baseline is copying that file over
`requests.tsv` in a pull request of its own.

**No shape owes it, and it replaces nothing a shape owes.** It is the instrument
the counted kinds are *attributed* with: an allocation count says a request
allocates eight times, and this says which frames spent the instructions around
them. A number here is work done rather than time taken — one vector instruction
counts as one — so it is not a latency and says nothing `kynos-bench` would.

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
| Off-path element | the document model, the emitters, the validators, `describe` | a proof it is unreachable from the request path, a binary delta, and an allocation count on generation | any per-request measurement |
| Opt-in payload codec | a body extractor or response codec behind a feature | a binary delta, and an allocation count on an operation that names it | a measurement on a route that never mounts it |

**A type-level surface owes a codegen delta and nothing else, for the same
reason [`testing.md`](testing.md#the-allocation) says it does not owe running.**
`CompatibleWith` and the `Cons`/`Both` stacks are compared
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
`Dispatch::serve` touches it. That is checked. `containment:check` holds four
elements — the emitted `Document`, an owned `Registry`,
`Validator` and `jsonschema` — to the sites
[`testing.md`](testing.md#the-off-path-proof) allows them, and a witness in
[`router/dispatch/tests.rs`](../crates/kynos/src/router/dispatch/tests.rs) pins
every field a request reaches through the dispatch table, so a new one is a
compile failure until someone argues for it.

**Off the request path is not the same as free, which is why the shape owes a
third thing.** Zero per request says nothing about what building the document
costs the process that builds it, and that cost scales in a dimension no
per-request measurement has: the number of operations declared.
[`kynos-openapi/tests/alloc.rs`](../crates/kynos-openapi/tests/alloc.rs) counts
one `to_json` and one `emit` at 10, 100 and 1000 of them, against recorded
ceilings and a per-decade sub-quadratic relation. It is billed here rather than
under a per-request shape precisely because a request never reaches it.

**An opt-in codec is measured on a route that mounts it.** Measuring `json`
against an operation with no body would report zero and mean nothing. The
question the shape exists to answer is what the codec costs the operation that
asked for it, against the same operation without it.

Both halves of that bill run.
[`tests/alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) counts each
codec on an operation that names it, and
[`cost/codec.tsv`](../crates/kynos/cost/codec.tsv) records the `.text` delta of
the fixture that mounts it, one row per codec over the transport floor's
baseline row.

**What those figures include is more than Kynos, and saying so is part of the
measurement.** Mounting a codec pulls in the crate that implements it —
`serde_json`, `serde_html_form`, `multer`, `prost`, `async-compression` — and
the operation declares a payload type carrying two derives. `compression` is
where the distinction stops being a caveat: nearly all of its row is the gzip,
brotli and zstd backends, so the row prices the encoders rather than the
interceptor over them. A figure that attributed that to the framework would be
a wrong number rather than an imprecise one.

**Most of multipart's decoding bill is `multer`'s.** On `codecs.multipart_echo`,
90 text parts fed to it as one chunk, it allocates 12 blocks per part: a
header map and a copy of each header value,
its own copy of the field name (three times: parsed, looked up against the size
limits, kept as the current field), the parsed `Mime`, and the delimiter
`format!`ed afresh at each field and at each read of its data. That is 1,086 of the
row's 1,565 blocks. Kynos keeps the chunk `multer` yields for a part rather than
copying it through `Field::bytes`, which saved up to two more per part. What remains
Kynos's is [`Part`](../crates/kynos/src/extract/body/multipart.rs)'s owned
currency — a name and a media type each way, and a `String` field's copy out
of the shared body — and one buffer for the whole response.

**A streamed response is counted on its drain.** Its status is committed before
a record is written, so what JSON Lines, JSON text sequences and Server-Sent
Events cost is spent while the body is polled, and
[`tests/alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) counts that
region alone. Records are written in turn into one shared block and split off
as frames, so the relation held there is that a record costs no allocation of
its own: sixteen small records cost what one does.

**A response's header map is grown, not presized, because sizing saves no
block.** `http` allocates a map's index table and its entries apart, so the
first field a codec writes costs two blocks (656 bytes) whatever capacity the
map was built with. Sizing trades bytes alone: the next step down from the
default's six fields holds three in 328 bytes, and a compressed response carries four, so it would
grow and pay two blocks more. `sizing_a_header_map_saves_no_block` in
[`tests/alloc.rs`](../crates/kynos/tests/alloc.rs) holds the premise, and turns
red when an `http` release makes presizing worth reopening.

**A cache that does not derive tags allocates no block of its own on a hit
beyond its future.** The lookup borrows its `PrimaryKey` from the request, and
`Age` — a static `0` when no whole second has passed — is written straight onto
the response, so what `interceptors.cache_hit` allocates past the boxed future
is the replayed response itself. Under `deriving_etags()`, a hit on a stored
response that carries no `ETag` derives the tag again, at four blocks or more:
`format!` starts the string too small and grows it twice for the 18-byte tag
(three times when the digest has four or more leading zero hex digits), and
`HeaderValue` boxes a shared handle to the string because its length is short
of its capacity. A miss whose response may be stored pays for storing it: an
owned key, the stored response's `Arc` and its copy of the response's fields,
and a copy of the request's header map. That copy is the whole map rather than
the fields `Vary` names, because `Vary` is a response field and is written
after the request has been handed on; a request that cannot be stored, or that
hits, copies none of it.

## The feature grading

Every flag `crates/kynos` declares appears in exactly one column below. The
point of listing all of them is that a feature nobody examined must be
distinguishable from a feature examined and found free — the failure
[`testing.md`](testing.md#the-allocation) names, and the one a coverage number
cannot show.

| Grade | Owes | Flags |
| --- | --- | --- |
| Full battery | everything its shape owes above | `server`, `http1`, `http2`, `tls`, `json`, `form`, `multipart`, `protobuf`, `compression`, `cache`, `cookie`, `assets`, `assets-fs`, `docs`, `unchecked`, `openapi32`, `trace`, `macros`, `pattern` |
| Off-path proof | a proof it is unreachable from the request path, and a binary delta | `openapi31`, `yaml`, `test-util`, `uuid`, `time`, `time-chrono`, `time-jiff`, `decimal`, `decimal-rust`, `decimal-big` |
| Aggregate | nothing of its own; it is the union of what it enables | `default`, `full` |

**`macros` is graded full despite adding no runtime code.** It gates every
derive, a derive is a type-level surface, and the shape table bills that a
codegen delta — which is a measurement an off-path proof does not include.
Grading it off-path would have filed the one flag most able to move
monomorphized IR under the grade that never looks at it.

**The grading is graded because the batteries cost different amounts.** A
scalar format whose whole contribution is a JSON Schema `format` cannot reach
`Dispatch::serve`, and spending an allocation replay on it would buy a zero
already implied by its shape. What it still owes is the proof of that, because
"cannot reach" is a claim about code rather than about intent.

**Both halves of the off-path grade now run.**
[`testing.md`](testing.md#the-off-path-proof)'s table holds every off-path
flag to the files that write its gate and, usually, call its crate, and
`containment:check` fails a flag graded off-path here with no row there — so
the two documents cannot drift apart in the direction that loses a proof.
[`cost/binary.tsv`](../crates/kynos/cost/binary.tsv) records a `.text` delta
for each off-path flag that compiles alone, `openapi31` and the four `time`
and `decimal` backends among them. `time` and `decimal` have no row of their
own because neither compiles alone, which `features:check` already probes.

**The table is counted against the manifest.**
[`containment:check`](../scripts/containment.py) reads the rows above and
`crates/kynos`'s `[features]`, and fails when a flag is declared and ungraded,
graded and undeclared, or graded in two rows — naming the flag rather than
reporting that two numbers differ. Both sides are read off disk, so no count is
stated here: per [`testing.md`](testing.md#cross-cutting) a number would be a
third place the same set is written down, and it would put two branches each
adding a flag on the same line where the table puts them on different ones.

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

The release gate compares absolutes exactly and is not that gate, because it
holds no ceiling to ratchet. It fails on a toolchain bump by design, and what
clears it is recording the new numbers, not raising a limit: a bump then costs
one reviewed re-record per release rather than a gate someone turns off.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why this is not in `kynos-bench`

The obvious filing puts everything with a number in it beside the harness. What
that misses is that most of these questions have no comparison to make. "What
does mounting `Compression` add to an operation" is answerable against Kynos
without another framework in the room, and unanswerable with one, because no
other library has `Compression`. Sending it to a comparative harness would leave
it unmeasured in both places.

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

### Profiling locally

Where a count says *how much* and the question is *where*, `profile:requests`
writes `target/gungraun/profile-attribution.md`, which CI uploads with the run:
for each scenario, the fifteen frames `callgrind_annotate --inclusive=yes`
ranks highest, and DHAT's allocation sites by the innermost frame in a
workspace crate. The Callgrind files beside it under
`target/gungraun/kynos-profile/` hold the rest — `--tree=caller` over one says
who called the allocator. A timed profile — `perf record` over the same
benchmark binary, which `[profile.bench]` builds with symbols — is the
corroboration a count cannot give, and it stays on the machine that took it
for the reason the section above gives.

**An ablation is a scenario and its base.** A scenario's `base` names the one
it differs from in exactly one variable — a layer, a coding, a method — and
`profile-report.md` gives the difference as that variable's marginal cost. A
row changing two variables at once has no base, since its difference would
attribute both to neither.

**The region drains the body.** Both drivers poll the response body to its
end inside the measured region, so a streamed body's production is counted
with the request. The frames are kept in a buffer reserved in setup, so
neither collecting them nor dropping them is counted.
`Poll`, the default, polls once with no executor and refuses a request that is
not ready, for the reason [`tests/support/counting.rs`](../crates/kynos/tests/support/counting.rs)
gives. `Runtime` enters a current-thread tokio runtime built in setup, for what
needs a timer or a wakeup — a timeout, a queued permit, an event stream's
keep-alive — and its own cost is the `json-small-runtime` row's difference
from `json-small`.

**Exactness is asserted.** `mise run profile:requests --repeat N` runs the
benchmark N times and fails when any program or DHAT count differs between
runs. A group that cannot be exact, because what it counts reads the wall
clock or a key the process draws at random, is named on `requests.tsv`'s
`# trend:` line: it is reported but never compared, against the baseline or
across runs, and its marginal costs are marked approximate. Each such row
outside `connection_h2` has a group of its own, so the rest of its table stays
exact. Four are marked. `connection_h2`: HTTP/2 Huffman-codes the
`Date` value into every response, and what that costs moves by a few
instructions as the clock's characters change. `rate_limit`, a `Poll` group
holding `INTERCEPTORS`' `rate-limit` row alone: its quota keys, its window
arithmetic and the remaining count and reset it writes are read off the wall
clock, so its count moves by a few instructions with the time it ran at.
`form_echo`, `CODECS`' `form-echo` row: `serde_html_form` groups the pairs it
decodes in an `IndexMap` under `std`'s `RandomState`, so how far each field's
lookup probes moves its count by a few instructions with the process's key,
and its heap not at all. `request_id_random`, `INTERCEPTORS`'
`request-id-random` row: `Random` keys its hash per process, and while
`Random` itself executes the same instructions and allocates the same block
whatever its key, glibc's `malloc` takes a path a few hundred instructions
longer in some processes than in others, so its count moves and its heap does
not. The runtime row and
`connection_h1` measured exact, the latter because its setup serves the
warming connection at the start of a wall-clock second: hyper renders `Date`
again, allocating, on the first response of each second, and a region a
second boundary separated from that render would count it.

**The connection path is a region of its own.** A `connection_*` row encodes
its catalog request to octets in setup and writes it into a duplex the server
was handed through `Server::serve_io`; the region is that connection's future
driven to its end under `Runtime`, and the response is read back and checked
in teardown. Every connection ends itself — `connection: close` over HTTP/1.1,
a client `GOAWAY` over HTTP/2 — and the server's direction of the duplex is
grown in setup, so writing the response reuses memory as a socket's send
would. Its difference from its in-process base is the server path's;
[`nfr.md`](nfr.md#routing) attributes it. A socket, a syscall and a kernel
buffer stay `kynos-bench`'s.

### Why the measurements live beside the features

Everything in [`testing.md`](testing.md) is filed by the code it constrains, and
a cost assertion is not different in kind from a correctness one. A central
cost target would collect assertions about modules whose authors never see
them, and the first time one failed, the person who broke it would be reading a
file they had no reason to know existed.

The allocation counter is the exception, and it is a mechanical one: a global
allocator is process-wide, so the counter has to own its process. That is a
fact about Rust rather than a judgement about where the assertion belongs.
