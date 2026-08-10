# Testing

What each kind of test can prove that no other kind can, and where it lives.
[`nfr.md`](nfr.md) records which guarantees these are asked to enforce; this
document is about the mechanics.

## The taxonomy

| Kind | Lives in | Runs under | Proves | Status |
| --- | --- | --- | --- | --- |
| Unit | a sibling `tests.rs` beside the module | `cargo nextest` | internal logic, including private items | in use |
| Doctest | the item's own documentation | `mise run test:doc` | that documented code compiles, and that undocumentable code does not | in use |
| Integration | [`crates/kynos/tests/`](../crates/kynos/tests/) | `cargo nextest` | that the public surface composes as a user would compose it | in use |
| UI snapshot | `crates/kynos/tests/ui/` | `trybuild` | the exact text of a diagnostic | built |
| Property | `crates/kynos-openapi/tests/properties.rs` | `proptest` | round-tripping, determinism and totality over generated documents | built |
| Conformance | a harness over a fixture app | `proptest` over live responses | *emitted ⊇ observable* against a running service | not built |

A module becomes a directory once it holds two independently-changing concerns
or exceeds ~400 lines, and its tests move to a sibling `tests.rs` at that point.
That is why unit tests appear at
[`di/tests.rs`](../crates/kynos/src/di/tests.rs),
[`schema/tests.rs`](../crates/kynos/src/schema/tests.rs) and
[`response/negotiate/tests.rs`](../crates/kynos/src/response/negotiate/tests.rs)
rather than inline.

Four of the five integration files exist for one reason each;
`hermeticity.rs` is a different kind of thing and is covered below.

| File | Asserts |
| --- | --- |
| [`pipeline.rs`](../crates/kynos/tests/pipeline.rs) | an `async fn` is a `Handler`, `routes!` collects it, `Endpoints` accepts it, and mounting reaches the context that supplies its dependencies |
| [`derives.rs`](../crates/kynos/tests/derives.rs) | every derive expands to a well-formed implementation of the trait it claims |
| [`typed_uri.rs`](../crates/kynos/tests/typed_uri.rs) | a route attribute's `uri` percent-encodes its parameters |
| [`compile/panic_recovery.rs`](../crates/kynos/tests/compile/panic_recovery.rs) | `catch_panics` refuses to compile under `panic = "abort"` |

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

Every case in `tests/ui/` obeys it: `tests/ui/pass/` holds one control per
negative, and a case whose control cannot be written does not land — it goes in
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

## The compile-only guard

```rust
if std::hint::black_box(false) { .. }
```

This asserts that a call *typechecks* without executing it. It is used in
[`tests/pipeline.rs`](../crates/kynos/tests/pipeline.rs) and
[`tests/compile/panic_recovery.rs`](../crates/kynos/tests/compile/panic_recovery.rs),
and it exists because the pre-v1 API skeleton is `todo!()`-bodied: the types are
the deliverable, and running them would only prove that `todo!()` panics.

`black_box` rather than `if false`, because the compiler must not be permitted
to prove the branch dead and skip the analysis that is the entire point.

The guard is temporary by design. Every use of it is a marker for a body that
has not been implemented, and each should disappear as its body lands rather
than being kept as a testing idiom.

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
Eleven traits already carry `#[diagnostic::on_unimplemented]` —
`Provides`, `Handler`, `FromRequestParts`, `FromRequest`, `Describe`,
`RequestContent`, `IntoResponse`, `Responses`, `Schema`, `MapKey` and
`Alternative`. Each one replaces the compiler's generic "the trait bound is not
satisfied" with a message naming the fix.

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
