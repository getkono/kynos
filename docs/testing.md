# Testing

What each kind of test can prove that no other kind can, where it lives, and
which kind a given module owes. [`nfr.md`](nfr.md) records which guarantees
these are asked to enforce; this document is about the mechanics.

## The taxonomy

| Kind | Lives in | Runs under | Proves | Status |
| --- | --- | --- | --- | --- |
| Unit | a sibling `tests.rs`, or an inline `mod tests` while the module is one file | `cargo nextest` | internal logic, including private items | in use |
| Doctest | the item's own documentation | `mise run test:doc` | that documented code compiles, and that undocumentable code does not | in use |
| Integration | [`crates/kynos/tests/`](../crates/kynos/tests/) | `cargo nextest` | that the public surface composes as a user would compose it | in use |
| UI snapshot | `crates/kynos/tests/ui/` | `trybuild` | the exact text of a diagnostic | built |
| Property | `crates/kynos-openapi/tests/`, over `support/`'s generators | `proptest` | round-tripping, determinism and totality over generated documents | built |
| Conformance | a harness over a fixture app | `TestClient` over live responses | *emitted ⊇ observable* against a running service | in use |

A module becomes a directory once it holds two independently-changing concerns
or exceeds ~400 lines, and its tests move to a sibling `tests.rs` at that point.
That is why unit tests appear at
[`di/tests.rs`](../crates/kynos/src/di/tests.rs),
[`schema/tests.rs`](../crates/kynos/src/schema/tests.rs) and
[`response/negotiate/tests.rs`](../crates/kynos/src/response/negotiate/tests.rs)
rather than inline.

Each integration file exists for one reason. `hermeticity.rs` and `ui.rs` are
different kinds of thing and are covered below.

`conformance.rs` runs now that the router and `test/` have landed, and both of
its assertions pass. `every_declared_response_is_exercised` carried an
`#[ignore]` naming a 413 that `BodyRejection` no longer declares — the defect
the harness found on its first run, recorded at
[`error/rejection.rs`](../crates/kynos/src/error/rejection.rs)'s `TooLarge`
comment and fixed in the same push. The attribute outlived its reason and went
with it.

| File | Asserts |
| --- | --- |
| [`pipeline.rs`](../crates/kynos/tests/pipeline.rs) | an `async fn` is a `Handler`, `routes!` collects it, `Endpoints` accepts it, mounting reaches the context that supplies its dependencies, each route attribute writes its own method, and both ends of the arity list typecheck |
| [`derives.rs`](../crates/kynos/tests/derives.rs) | every derive expands to a well-formed implementation of the trait it claims, counted against the macros the crate exports |
| [`errors.rs`](../crates/kynos/tests/errors.rs) | each extractor rejects with the rejection type its signature names |
| [`reporting.rs`](../crates/kynos/tests/reporting.rs) | every error type a caller can receive is `Error + Send + Sync + 'static` |
| [`typed_uri.rs`](../crates/kynos/tests/typed_uri.rs) | a route attribute's `relative_uri` percent-encodes its parameters |
| [`size.rs`](../crates/kynos/tests/size.rs) | a build failure does not inline a `Violation`, and a `Result` costs no more than it |
| [`conformance.rs`](../crates/kynos/tests/conformance.rs) | that the responses a suite observed match what the document promises, and that every declared response was exercised |
| [`compile/panic_recovery.rs`](../crates/kynos/tests/compile/panic_recovery.rs) | `catch_panics` refuses to compile under `panic = "abort"` |

`crates/kynos-openapi/tests/` holds four more: `properties.rs` and
`templates.rs` for the document and path-template properties, `wire.rs` for the
per-type wire shapes, and its own `size.rs`. `support/` beside them is not a
target — it is the generator module the property files share, included by
`#[path]` because an integration binary cannot be depended on.

`panic_recovery.rs` is a `harness = false` test target rather than an ordinary
one, because [`mise run panic:check`](../mise.toml) asserts that *building* it
fails and greps the compiler's message. A passing build is the failure
condition.

The UI suite does not run under coverage instrumentation: `trybuild` spawns its
own `cargo`, and `llvm-cov`'s flags reach the child and perturb the exact stderr
a snapshot records. [`mise run ui:check`](../mise.toml) is its own task and its
own CI step for that reason — and the exclusion belongs on the coverage command
rather than on the nextest profile, because a profile-wide filter would remove
the suite from every job that sets `NEXTEST_PROFILE`.

## The allocation

The taxonomy says what each kind of test proves. This says which kind a module
owes — and, in the last column, which kinds it does not.

That last column is the one that keeps a suite affordable. Without it,
"test thoroughly" reads as "test every way you can think of": the same property
gets asserted three times in three styles, and a module nobody happened to think
about gets nothing. Both failures are invisible in a coverage number.

Five kinds of code account for the workspace.

| Kind | Recognised by | Owes | Does not owe |
| --- | --- | --- | --- |
| Value type | a `Serialize`/`Deserialize` derive, and no logic beyond builders and accessors | the crate's round-trip and determinism properties, reached through a shared generator; and one exact-JSON case fixing its wire shape | per-field tests, accessor tests, a hand-written round-trip |
| Closed enumeration | an enum or `const` table mirroring a fixed list in the specification | one table test whose closure fails when a variant is added | cases covering some of the variants |
| Parser | an open input space — a `&str`, arbitrary JSON, a whole document | a property against an independently constructed oracle; and one case per error variant, counted against the source | round-tripping alone |
| Type-level surface | a trait, a bound, an arity impl, a derive, or a rule that something must not compile | a doctest for the rule, a `.stderr` snapshot for its wording, a witness fn for the bound | running it — above all against a `todo!()` |
| Runtime I/O | a socket, a timer, a task or a signal | an integration test over a real socket | a mock of the runtime |

A value type owes two things because neither implies the other. A round-trip
proves that `parse ∘ emit` is the identity, and a misspelled field name satisfies
that perfectly: nothing in the model sets `deny_unknown_fields`, so `descripton`
is absorbed by the flattened `Extensions`, written back unchanged, and compares
equal — while the real `description` stays `None` from end to end. The round-trip
proves nothing was lost. The exact-JSON case is what holds the shape to the
specification.

A closed enumeration is checked across the whole set because a sample of it
reads as the whole set and is not. `the_style_location_table_is_closed` asserted
five of forty style/location pairs, and `explode_defaults_to_true_only_for_form`
asserted two of eight — and the six it skipped included `cookie`, which 3.2
gives the same `explode` default as `form` and which the model answered `false`
for. The name claimed closedness; the body sampled.

*Independently constructed* is the whole of the parser rule. An oracle derived
from the parser under test agrees with it by construction, including wherever
both are wrong. `TemplateCase` in
[`tests/support/`](../crates/kynos-openapi/tests/support/mod.rs) is the shape to
copy: it carries the normalized form and the variable list that `build_template`
recorded while assembling the string, so the property compares the parser against
something that never consulted it.

A *property* is not the only shape that rule takes. Where the input space is
finite and small, enumerating it is the stronger statement, because a sweep is
total where a draw from the same space is a sample of it.
`wildcards_cover_their_class_and_nothing_else` sweeps all of `100..=599` against
a transcribed table, and
`every_arrangement_of_blank_lines_splits_without_losing_a_word` in
[`route/tests.rs`](../crates/kynos-macros/src/route/tests.rs) sweeps all
thirty-two arrangements of five lines. Read the Parser row as asking for an
independent oracle rather than for `proptest` specifically: a generator over a
space small enough to close is the weaker of the two.

An attribute grammar is not a parser in this sense. `RouteArgs::parse` reads
four keys and `wire_name` resolves three sources in precedence order: the input
space is the key set rather than the token stream, and generating over it
re-derives the match arms it was meant to check. Such a grammar owes what a
closed enumeration owes — one case per diagnostic, counted against the source —
with the wording left to a `.stderr` snapshot, where a reader sees it rendered.

### Two rules that are not code kinds

**A `todo!()`-bodied item owed its `no_run` doctest and nothing further.**
Anything more would have asserted that `todo!()` panics.

**That rule has now lapsed, and what it deferred is owed.** The API-skeleton
milestone is over: the bodies under `router/`, `extract/`, `response/`,
`middleware/`, `security/`, `schema/` and the derives are implemented. Each
`black_box(false)` guard marked one body whose obligation began when it landed,
and they have landed — so every such guard is now a test that should be replaced
by one exercising the body it was standing in for, and the modules the previous
paragraph excused are the ones carrying the largest gap. This is debt now, where
before it was design.

**Conformance is a system obligation, not a module one.** No allocation above
substitutes for it, which is why the row stays in the taxonomy while unbuilt.
When it is built, the parsing half has a corpus waiting: the three active
references carry several hundred official example documents in fenced blocks —
66 JSON and 83 YAML in `3.1.2.md` alone — and extracting them is the intended
source for [`nfr.md`](nfr.md#document-model)'s *emitted documents validate
against both 3.1 and 3.2 validators*. Most fences hold a single object rather
than a whole document, so the extractor is a piece of work in its own right.

### Where the macro crate's tests live

`kynos-macros` cannot depend on `kynos`, so anything needing the facade — a
derive's *expansion* compiling, a diagnostic's wording — lives in
`crates/kynos/tests/`. What stays in the macro crate is what can be checked
without it: the attribute grammars, the shape checks the derives share, and the
signature the typed-URI emitter writes.

Its diagnostics are held twice on purpose, and the halves do different jobs.
`derive/tests.rs` asserts *which* rule fired, counted against the
`syn::Error::new` sites so a rule added without a case fails the build;
`tests/ui/macros/` asserts what that rule *says*. Neither substitutes for the
other: a count cannot read a message, and a snapshot suite cannot notice a rule
nobody wrote a case for.

### Cross-cutting

Three obligations hold whatever the kind.

**Every test target compiles and runs at baseline, not only under
`--all-features`.** [`mise run test`](../mise.toml) passes `--all-features` and
`features:check` passes `--no-dev-deps`, so until
[`mise run test:baseline`](../mise.toml) landed, no test target had ever been
built under `openapi31` alone — against the hundred-odd `openapi32` `#[cfg]`
sites in `kynos-openapi/src`. A feature gate no test build exercises is a gate
whose off-state is unknown, and the suite passing on the first baseline run does
not retire the obligation: it held by luck rather than by check.

**A gap [`nfr.md`](nfr.md) documents is characterized.** Excluding a known-lossy
shape from a generator keeps the property honest, but on its own it leaves the
behaviour unrecorded: closing the gap turns nothing red, and widening it turns
nothing red either. Each exclusion pairs with a test asserting what happens
today, named so it reads as a record rather than an endorsement, and each side
points at the other.

**Exhaustiveness is asserted, not intended.** Wherever a closed set has one case
apiece, a test counts the set against the cases and fails when the two part
company. `every_rejected_schema_type_has_a_case` in
[`tests/ui.rs`](../crates/kynos/tests/ui.rs) was the first; the model's wire
shapes and `SpecError`'s variants are counted the same way. A reviewer cannot
see the case that was not written.

## The pass-control rule

**Every compile-fail case gets a sibling passing case that differs in exactly
the property under test.**

A negative on its own cannot distinguish "the rule holds" from "the surface is
unusable". `compile_fail` asserts only that the block does not compile, so it
passes for the wrong reason whenever anything in the block is broken —
a missing implementation, a renamed module, a feature that happens to be off.

This is not hypothetical. Before the `Schema` implementations landed, the
compile-fail doctests in this crate were passing because *nothing* in a handler
signature typechecked, not because the rejections worked. The failure is silent
by construction: a test that asserts absence cannot report that it found too
much absence.

`tests/ui/` holds four groups: `macros/` for the attribute grammars, `schema/`
for the refusal table, `antipattern/` for the README's list, and `traits/` for
the bounds whose diagnostics have no other home.

Every case in `tests/ui/` obeys the pass-control rule: `tests/ui/pass/` holds
one control per negative, and a case whose control cannot be written does not land — it goes in
[`PENDING.md`](../crates/kynos/tests/ui/PENDING.md) with the blocker named.

That ledger is where the rule earns its keep. `#[kynos::operation]` was
scheduled for two negatives, both of which produced exactly the right
diagnostic; no control could be written for either, because the attribute was
broken and *no* program using it compiled. Nothing else in the suite would have
noticed.

The `compile_fail` doctests that remain are a separate matter. Four have a
control beside them and the rest do not; each is a single rule stated where a
reader needs it, and the tabular ones — the path-template rejections and the
`Schema` refusal table — have moved into the suite, where exhaustiveness can be
checked. `every_rejected_schema_type_has_a_case` in
[`tests/ui.rs`](../crates/kynos/tests/ui.rs) counts the refusal table's rows
against the cases, so a row added without one fails the build.

The `Provides` case has a positive control in
[`di/tests.rs`](../crates/kynos/src/di/tests.rs) rather than in the doctest.
That is weaker than a sibling block: a unit test and a doctest can drift apart,
and the reader of the compile-fail case does not see the control.

## The compile-only guard, retired

```rust
if std::hint::black_box(false) { .. }
```

This asserted that a call *typechecks* without executing it, and existed
because the pre-v1 API skeleton was `todo!()`-bodied: the types were the
deliverable, and running them would only have proved that `todo!()` panics.
`black_box` rather than `if false`, so the compiler could not prove the branch
dead and skip the analysis that was the entire point.

It is recorded here because it left a mark on the suite rather than because it
is available. **A guarded body holds no assertions.** Nothing inside one runs,
so an `assert_eq!` there is a claim the suite appears to make and never checks
— the one failure mode a reader cannot see, since the test passes and reads as
though it verified something. `routes_collects_every_operation` and
`endpoint_collections_compose` each asserted a count that way, and each got its
count back when the body landed.

Every use of the guard was a marker for an unimplemented body, and the bodies
have all landed. There are none left, and a new one is not a testing idiom to
reach for — it is the sign of a surface that should not have been written yet.

## Hermeticity

Tests are hermetic by construction, not by convention.

| Mechanism | Where |
| --- | --- |
| One process per test | `cargo nextest` |
| `retries = 0` | [`.config/nextest.toml`](../.config/nextest.toml) |
| `slow-timeout` terminating after four periods | same |
| `leak-timeout` failing the test | same |
| A guard test that fails under a shared process | [`tests/hermeticity.rs`](../crates/kynos/tests/hermeticity.rs) |

`hermeticity.rs` is the interesting one: its two tests observe the same `static`
and both assert they saw its initial value, which is only possible when each
runs in its own process. They pass under `cargo nextest run` and fail under
`cargo test`. That converts "we use nextest" from a README claim into a test.

A flake is an isolation bug. Retrying one hides the bug and keeps the suite
green, which is why `retries = 0` is in the config rather than left to a flag
someone might pass.

## Snapshots

A `.stderr` snapshot is the exact text `rustc` printed. Recording one is
mechanical — run the suite, inspect what `trybuild` wrote under `wip/`, and
promote it — but two things about it are not.

**The toolchain must be pinned.** [`mise.toml`](../mise.toml) pins rust to
1.97.1, and every snapshot in the tree is a snapshot of *that* compiler.
rustc rewords diagnostics between releases, so an unpinned toolchain turns a
UI suite into a source of failures that carry no information about the change
that triggered them. Bumping the pin and re-recording the snapshots is one
commit whose diff is reviewed as a diff.

**`on_unimplemented` attributes must land before any snapshot is recorded.**
Fourteen traits carry `#[diagnostic::on_unimplemented]` —
`Provides`, `Handler`, `FromRequestParts`, `FromRequest`, `Describe`,
`RequestContent`, `IntoResponse`, `Responses`, `Schema`, `MapKey`,
`Alternative`, `ShortCircuit`, `EndpointMeta` and `IntoEndpoints`. Each one
replaces the compiler's generic "the trait bound is not satisfied" with a
message naming the fix.

`every_guided_diagnostic_has_a_snapshot` in
[`tests/ui.rs`](../crates/kynos/tests/ui.rs) maps each to the snapshot that
records it and counts the pairs against the attributes in the source. Eight of
the fourteen had none, so more than half of what this requirement names was
unchecked. The mapping is written out rather than searched for, because half the
messages deliberately never spell the trait: `Handler`'s says "is not a Kynos
handler", which is the improvement rather than something to grep for.

Recording snapshots first would pin the generic message as the expected output,
so adding the attribute a trait needs would then show up as a test failure — and
a suite where improving a diagnostic breaks the build teaches contributors to
leave diagnostics alone. Order matters here in a way it does not for most
tests: write the diagnostic, then record what it says.

The requirement these snapshots enforce is
[`nfr.md`](nfr.md#macros)'s "no diagnostic names an internal type", which is the
reason the suite has to be exhaustive rather than illustrative. A diagnostic
nobody snapshotted is a diagnostic nobody checked.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why compile-fail cases live in doctests rather than in `trybuild`

The two do different jobs and the overlap is smaller than it looks. A doctest
lives beside the item it constrains, so a reader of `Redirect` meets the proof
that `Redirect<304>` is rejected without going anywhere. `trybuild` asserts the
*text* of the rejection, which is a different guarantee and belongs with the
macros, where the message is the product.

The split is therefore: a doctest for "this must not compile", a UI snapshot for
"and this is what it says". Migrating the first group into `trybuild` would move
proofs away from the items they document in exchange for a stricter assertion
nobody asked for.

### Why the conformance harness is the one that matters

Everything else in this document tests the framework's types. The conformance
harness would test the framework's *claim* — that a running service never
returns a response the emitted document omits. Until it exists, the soundness
invariant in [`middleware.md`](middleware.md) is an intention held up by
review.

It is last rather than first because it needs a service that actually runs, and
the surface is still `todo!()`-bodied. That ordering is a fact about the
schedule, not a judgement about priority.
