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

Tests move to a sibling `tests.rs` once a module passes ~400 lines, whether or
not that module also becomes a directory — the two halves of the layout rule are
separate, and [`nfr.md`](nfr.md#the-module-size-budget) says why the second one
is a prompt rather than a trigger. That is why unit tests appear at
[`error/rejection/tests.rs`](../crates/kynos/src/error/rejection/tests.rs) beside
a module well past that line — eight rejection types, and a count that moves
whenever one of them gains a status — and at
[`middleware/compression/tests.rs`](../crates/kynos/src/middleware/compression/tests.rs)
beside another, rather than inline.

The sibling file is the settled shape here even below that line — `di/`,
`schema/` and `response/negotiate/` all keep one while sitting well under 400 —
so what the rule really fixes is the point past which staying inline stops being
a choice. Only one module in the workspace still holds an inline `mod tests`,
and it is 110 lines.

Each integration file exists for one reason. `hermeticity.rs` and `ui.rs` are
different kinds of thing and are covered below.

`alloc.rs` is a third: it asserts a cost rather than a behaviour, and the kind
it belongs to is allocated by [`performance.md`](performance.md#the-taxonomy)
rather than by the table above. The two documents divide by question — this one
says what a guarantee owes, that one says what a *feature costs the request
path* — and `alloc.rs` is filed here as well because the inventory above claims
to be every integration target, and a claim of completeness is worth only as
much as its exceptions. [`alloc_body.rs`](../crates/kynos/tests/alloc_body.rs)
and [`alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) are filed here
under the same exception and for the same reason: they assert a cost too, and
each is a target of its own rather than more rows in `alloc.rs` because a
`#[global_allocator]` measures the whole binary it is installed in, so a body
constructor's number and a codec's cannot share a file with the routing path's.

`conformance.rs` runs now that the router and `test/` have landed, and both of
its assertions pass. `every_declared_response_is_exercised` carried an
`#[ignore]` naming a 413 that `BodyRejection` no longer declares — one of
[the defects the harness found](#what-the-harness-found-on-its-first-run). The
attribute outlived its reason and went with it.

| File | Asserts |
| --- | --- |
| [`pipeline.rs`](../crates/kynos/tests/pipeline.rs) | an `async fn` is a `Handler`, `routes!` collects it, `Endpoints` accepts it, mounting reaches the context that supplies its dependencies, each route attribute writes its own method, and both ends of the arity list typecheck |
| [`derives.rs`](../crates/kynos/tests/derives.rs) | every derive expands to a well-formed implementation of the trait it claims |
| [`errors.rs`](../crates/kynos/tests/errors.rs) | each extractor rejects with the rejection type its signature names |
| [`reporting.rs`](../crates/kynos/tests/reporting.rs) | every error type a caller can receive is `Error + Send + Sync + 'static` |
| [`typed_uri.rs`](../crates/kynos/tests/typed_uri.rs) | a route attribute's `relative_uri` percent-encodes its parameters, and that the hand-written fixture it encodes with describes what it encodes — a `Schema` body nothing executes cannot disagree with the `encode` beside it |
| [`size.rs`](../crates/kynos/tests/size.rs) | a build failure does not inline a `Violation`, a `Result` costs no more than it, and which of the three bodies Kynos erases are zero-sized — the reasons `alloc_body.rs`'s counts read the way they do, filed here because a `size_of` needs no allocator |
| [`alloc.rs`](../crates/kynos/tests/alloc.rs) | what the routing path allocates per route shape, that a replayed request costs what the first one did, and what one interceptor adds to a request, at stack depth 0/4/8, with the width of the future dispatch returns guarded beside it. It owns a `#[global_allocator]`, which is why it is a target of its own rather than a sibling `tests.rs`: installed in the library's unit-test binary the counter would reach every unit test in it. Its counting harness is [`tests/support/counting.rs`](../crates/kynos/tests/support/counting.rs), included by `#[path]` so a second counting target can own its own allocator without a second copy of the rationale. It also carries the two assertions that hold the *instrument* rather than the router, once for both counted targets — that a count is the measuring thread's alone, and that a driver which stopped counting reallocations, or fresh allocations, goes red |
| [`alloc_body.rs`](../crates/kynos/tests/alloc_body.rs) | what erasing a body through `UnsyncBoxBody` costs: nothing for an empty body, which erases a zero-sized type, and one allocation for any body that does not. A second `#[global_allocator]` target, for the reason the first one is one; the zero-sized facts behind both numbers are size guards and live in `size.rs` |
| [`alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) | what each opt-in payload codec adds to an operation that mounts it, against the same service's bodyless and transport floors — and how compression's cost grows with the body it holds. A further `#[global_allocator]` target rather than a second module of `alloc.rs`, because a `#[global_allocator]` is per process and each integration target is one process, installing the same counter `alloc.rs` does by including [`tests/support/counting.rs`](../crates/kynos/tests/support/counting.rs), so one driver decides what both files' numbers mean. It deliberately does not restate `work_on_another_thread_is_not_counted` |
| [`conformance_corpus.rs`](../crates/kynos/tests/conformance_corpus.rs) | that the committed corpus is what this build emits, and that it still carries the 3.2 constructs it exists to pin — asserted against the committed *text*, since what a downstream repository reads is the file |
| [`conformance.rs`](../crates/kynos/tests/conformance.rs) | that the responses a suite observed match what the document promises, and that every declared response was exercised |
| [`matrix.rs`](../crates/kynos/tests/matrix.rs) | the same two assertions over every layer Kynos owns, which is the only place a wrong *document* fails against responses that actually happened. Static assertions about that document belong in `description.rs` even when the matrix is what found them: the matrix reports which promise went unkept, and the small fixture there says which rule was broken. That rule is about a document; an assertion about a *type* is a different question and stays with the type, which is why the `ShortCircuit` sweep is in `interceptors.rs` — see [below](#the-sweep-and-the-matrix-assert-one-property-over-two-sets) |
| [`dispatch.rs`](../crates/kynos/tests/dispatch.rs), [`routing.rs`](../crates/kynos/tests/routing.rs), [`panics.rs`](../crates/kynos/tests/panics.rs) | every outcome one request can reach, the routes the router declines, and that recovery happens only where it was asked for |
| [`limits.rs`](../crates/kynos/tests/limits.rs), [`interceptors.rs`](../crates/kynos/tests/interceptors.rs), [`middleware.rs`](../crates/kynos/tests/middleware.rs), [`cors.rs`](../crates/kynos/tests/cors.rs), [`description.rs`](../crates/kynos/tests/description.rs), [`sse.rs`](../crates/kynos/tests/sse.rs) | each interceptor doing what it declares, setting only what it declared, and declaring it on exactly the operations it covers. `middleware.rs` also holds `partial` and `ranged_assets`, which assert that compression leaves anything a byte range is calculated against alone — a range is calculated over the encoded octets, so re-encoding a 206 puts a `Content-Range` on a body it is wrong about, and encoding a 200 that advertises `Accept-Ranges` puts one strong `ETag` over two representations. `ranged_assets` is the second half end to end: it resumes an asset download against the tag it was served with and splices the two halves back into the file. `description.rs` carries the same scope question one level down in its second section: which *statuses* within an operation a response field's declaration reaches, which is where `Accept-Ranges`, `Content-Range` and the 416 are each pinned to the statuses that give them a meaning — and where a response header nothing in the handler writes is pinned too, since a header group's and an interceptor's alike are filed under a wildcard and have to reach the exact key a consumer resolves to. Its third section is that question on the tag axis: which of the four tag scopes reaches the operation's `tags`, in what order, and whether each scope that names a tag also registers its metadata in the document's `tags` — a name arriving without its metadata is an `UndocumentedTag` warning on every operation carrying one, so both halves are asserted for every scope |
| [`rate_limit.rs`](../crates/kynos/tests/rate_limit.rs) | the shipped limiter over a store: one quota and several, burst, keying, exemption, and both failure policies — and, since an application may replace the algorithm outright, that a `RateLimitPolicy` Kynos does not ship reaches the wire with its own `Retry-After` — behaviour that is a property of a *sequence* of requests rather than of any one |
| [`client.rs`](../crates/kynos/tests/client.rs) | the `TestClient`'s own surface rather than the harness's: every method the router accepts, a query string, a cookie jar, a peer address, the three body setters, and the two assertions a suite would otherwise hand-roll — a 206 checked as a `Content-Range` *and* a body that fills it, and a finite event stream read as its events |
| [`cookies.rs`](../crates/kynos/tests/cookies.rs) | that two `Set-Cookie` fields reach the wire as two, which no unit test of either end can see |
| [`localization.rs`](../crates/kynos/tests/localization.rs) | that a negotiated language reaches the wire and that `Vary` accumulates rather than replaces when a second interceptor also varies — the two properties neither end can see. Two `Accept-Language` field lines are read as one list, which no test of the parser can reach because only a request carries two; and a localized response paired with `Compression` carries both `accept-encoding` and `accept-language`, where either interceptor alone would see only its own contribution |
| [`unchecked.rs`](../crates/kynos/tests/unchecked.rs) | that the escape hatches serve, that the router's own machinery still covers them, and what the waiver leaves on the document |
| [`assets.rs`](../crates/kynos/tests/assets.rs) | both asset modes, and the stored-coding surface — that two representations get two strong tags, that a resume across them is refused, that a 304 answers per representation, and that `Vary` is sent only by the files that negotiate: what an embedded set describes, what a served directory records instead, that traversal is refused end to end, and the whole range surface a file answers with — the 206 carrying exactly the octets its `Content-Range` names, the 416 stating the complete length, an unusable field ignored, and `If-Range` and `If-None-Match` deciding which of the two a client gets |
| [`docs.rs`](../crates/kynos/tests/docs.rs) | that a mounted reference is two described operations and not a waiver: what the two routes register, that the description served is byte-for-byte the one `openapi` emits, and that a nested mount moves both routes *and* the pointer the page carries — the one property that cannot hold unless both halves are rendered after the prefix is known. Which of the two titles the page carries is here for the same reason: it is resolved against the finished document, so no test of `page::render` has one to default to |
| [`ranged.rs`](../crates/kynos/tests/ranged.rs) | ranged delivery over a `ByteSource` that is not a filesystem: every status sections 13 and 14 allow, that a matching condition beats a range, that a tag outranks a date, and that HEAD carries every field and no content |
| [`determinism.rs`](../crates/kynos/tests/determinism.rs) | that one API emits one description whatever process emits it, by re-executing the test binary three times and byte-comparing — and that a component is registered after everything it refers to |
| [`cache.rs`](../crates/kynos/tests/cache.rs) | that a hit is served, that a response stating no lifetime is not, and that a `Conditional` over a `Cache` answers with no body — properties of a *sequence* of requests |
| [`compile/panic_recovery.rs`](../crates/kynos/tests/compile/panic_recovery.rs) | `catch_panics` refuses to compile under `panic = "abort"` |
| [`metaschema.rs`](../crates/kynos/tests/metaschema.rs) | that an emitted document validates against the OAI's own published meta-schema, read from `references/` — the one assertion this repository does not write itself |
| [`ledger.rs`](../crates/kynos/tests/ledger.rs) | the derives and route attributes `kynos-macros` declares, counted against the sets `derives.rs` and `pipeline.rs` witness |
| [`src/server/tests.rs`](../crates/kynos/src/server/tests.rs) | the runtime-I/O row's allocation: a real socket, over accept, shutdown, drain and TLS. It is a sibling `tests.rs` rather than an integration target because it reaches internals no public path exposes |

`metaschema.rs` and `ledger.rs` are the two targets `crates/kynos/Cargo.toml`
excludes from the published archive. Both read something above the package root
— the OAI's vendored schemas, and the macro crate's own source — and
`cargo package` carries a package directory and nothing beside it, so an archive
holding either would hold a test that could only fail. Each is a property of the
workspace, and stays where the workspace is; `containment:check` fails a build
where any *published* file reaches out the same way.

`crates/kynos-openapi/tests/` holds five more: `properties.rs` and
`templates.rs` for the document and path-template properties, `wire.rs` for the
per-type wire shapes, its own `size.rs`, and its own
[`alloc.rs`](../crates/kynos-openapi/tests/alloc.rs) — what one `to_json` and
one `emit` allocate at 10, 100 and 1000 operations, that each decade's growth
factor is below the quadratic one in allocations and in output bytes alike, and
that a repeated emission costs what the first one did. It is a target of its
own for the reason `kynos`'s namesake is: a `#[global_allocator]` is
process-wide, so a counter installed in the library's unit-test binary would
reach every unit test in it. Two targets rather than one because an integration
binary cannot be depended on — each crate that counts installs the counter
itself. `support/` beside them is not a target — it is the generator module the
property files share, included by `#[path]` for the same reason.

`crates/kynos/tests/support/` is the same idiom: the fixture app the runtime
targets drive, one request builder over the public `Service::call`, and
`counting.rs`, whose `#[global_allocator]` line a target installs
process-wide by including it — so that module belongs only to a target that
wants the counter, and carries its own request builder for the same reason. Over
`Service::call` rather than [`TestClient`](../crates/kynos/src/test/mod.rs),
because `test-util` is not a default feature — a target reaching for the client
compiles to nothing under `mise run test:baseline`, and that task is only a
baseline while the feature stays off. `conformance.rs` deliberately keeps its
own fixture: it is described in two places as the runnable form of
`examples/testing.rs`, and a reader checks that correspondence by eye.

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

That child `cargo` is not free of this repository's configuration, which is the
half a snapshot's author has no reason to expect. `trybuild` generates a
standalone workspace under `target/tests/trybuild/` and its manifest carries no
`[profile]` section, so a profile written in the root `Cargo.toml` never reaches
the fixtures — but cargo discovers *configuration* by walking up from the
working directory, and that generated project sits inside this repository, so
[`.cargo/config.toml`](../.cargo/config.toml) does reach it. The fixtures build
at the dev profile declared there rather than at cargo's default, which is why
`target/tests` is 2.3 GiB and not 11. Debug information is not what a snapshot
records, so nothing about the suite's output turns on it; a change to that file
that did reach the output would show up as every snapshot moving at once.

Both coverage tasks carry it. `coverage:ci` always did; `coverage` did not,
which mattered because `hooks:pre-push` runs that one — so every push ran the
suite under exactly the instrumentation this paragraph says perturbs it.

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
five of forty style/location pairs, and `explode_defaults_to_true_for_the_two_styles_that_pair_names_with_values`
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
Anything more would have asserted that `todo!()` panics. That rule is spent: the
API-skeleton milestone is over, the bodies landed, and what it deferred has been
paid — `router/`, `extract/params/`, `response/codec/`, `response/stream/`,
`middleware/`, `security/` and `src/test/` each left zero executing test
functions behind. It is recorded rather than deleted because a future skeleton
milestone would reach for it again, and because the shape of what it deferred is
the reason those modules were the last to be covered.

**Conformance has an outward-facing half, and Kynos owns it.** The harness
checks a running service against its own description and exports nothing, which
answers "does this service keep its promises" and not "are the promises the ones
a client generator was built against". Neither repository can check the second
from its own side, so the checkable thing between them is a committed corpus:
[`tests/fixtures/conformance/`](../crates/kynos/tests/fixtures/conformance/),
regenerated with `mise run fixtures:generate` and compared on every run.

Ownership was worth settling rather than assuming. The acceptance contract this
came from says a downstream generator must "pass fixtures generated by Kynos"
and "the same Kynos-generated conformance fixtures" — Kynos emits the contract,
the generator consumes it, and the fixtures are the contract written down. The
corpus carries the constructs a 3.2 generator is forked to understand and a 3.1
one cannot express: `itemSchema`, `contentMediaType`, `contentSchema` and the
SSE envelope.

It is only sound because emission is byte-stable. Without
[`determinism.rs`](../crates/kynos/tests/determinism.rs), "the committed file
equals a freshly generated one" would be a statement about the order two
`HashMap`s happened to iterate in.

**Conformance is a system obligation, not a module one.** No allocation above
substitutes for it, which is why it has a row of its own. The parsing half still
has a corpus waiting: the three active references carry several hundred official
example documents in fenced blocks — 66 JSON and 83 YAML in `3.1.2.md` alone —
and extracting them is the intended source for
[`nfr.md`](nfr.md#document-model)'s *emitted documents validate against both 3.1
and 3.2 validators*. Most fences hold a single object rather than a whole
document, so the extractor is a piece of work in its own right.

### Where the macro crate's tests live

`kynos-macros` cannot depend on `kynos`, so anything needing the facade — a
derive's *expansion* compiling, a diagnostic's wording — lives in
`crates/kynos/tests/`. What stays in the macro crate is what can be checked
without it: the attribute grammars, the shape checks the derives share, and
both the signature and the documentation the typed-URI emitter writes.

Its diagnostics are held twice on purpose, and the halves do different jobs.
`derive/tests.rs` asserts *which* rule fired, counted against the
`syn::Error::new` sites so a rule added without a case fails the build;
`tests/ui/macros/` asserts what that rule *says*. Neither substitutes for the
other: a count cannot read a message, and a snapshot suite cannot notice a rule
nobody wrote a case for.

Nothing here renders the documentation that emitter writes, and nothing here
can: rendering it needs the facade, and this crate cannot depend on it.
Nothing in `crates/kynos` renders it either, but that is a fact about the tree
rather than a property of it — route attributes appear in `crates/kynos/src`
only inside doctest fences, rustdoc does not extract doctests from doctests,
and `examples/` and `tests/` are never doctested, so `test`, `test:doc` and
`docs:check` today pass whether or not a macro's emitted rustdoc is
well-formed. One route attribute on a documented public item would change
that.

Until one exists, the sweep in `route/tests.rs` stands in for the render by
asserting the property CommonMark's indented-code-block rule makes
load-bearing: no emitted line's leading whitespace reaches four columns,
counting a tab to the next stop as CommonMark does. It is a string check
rather than a render, so every other way to break emitted documentation — an
unannotated fenced block, a broken intra-doc link, a `#` colliding with a
heading — remains unobserved, and it covers the typed-URI emitter alone.

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

That leaves three shapes a test target is built at — every feature on, the
default set, and `openapi31` alone — and a target gated on one optional feature
apiece is at none of them.
[`alloc_codecs.rs`](../crates/kynos/tests/alloc_codecs.rs) is that target: five
modules, one codec each, over a shared harness gated on their disjunction. The
sets it is interesting at are `openapi31 + macros + F`, and no task built one —
`features:targets` builds one feature at a time against `openapi31`, so `macros`
and a codec are never in the same build, and `features:check` passes
`--no-dev-deps`. [`mise run lint:codecs`](../mise.toml) is the six missing sets.
It is a Clippy run rather than a test run because what those sets alone can see
is a compile-time consequence — an item dead once one codec is off, an import
with no user — rather than an assertion that fails; a misspelled feature *name*
was never the exposure, since `unexpected_cfgs` validates one against the whole
feature list wherever the file compiles at all. `202cfa5` is the class, and it
was found by hand-linting the six sets before there was a task that did.

A second target now sits at exactly those six sets and arrived after the task
that lints them:
[`cost/codec.rs`](../crates/kynos/cost/codec.rs), the fixture
[`performance.md`](performance.md#the-taxonomy)'s codec sweep weighs. It is an
example rather than a test, so `--all-targets` is what reaches it, and the sets
it is *measured* at are the sets it is already linted at — which is why it
needed no entry of its own.

A fourth shape is a *dependency's* feature forced on: one no manifest in the
workspace asks for, and that Cargo unifies in anyway from whatever graph a
downstream program builds. [`mise run test:arbitrary-precision`](../mise.toml)
builds `kynos-openapi`'s library with `serde_json/arbitrary_precision` on, under
which a `serde_json::Number` serializes as a one-field struct only serde_json's
own serializer reads back as a number — so how `to_yaml` handles one is
observable there and nowhere else. It runs `emit::tests::yaml` alone, with
`--no-tests=fail` so a rename cannot leave it passing over nothing, because
parsing breaks separately under that graph: a numeric keyword inside an untagged
enum such as `RefOr` fails to deserialize, and the property round-trips fail on
that before emission is reached. A dev-dependency asking for the feature is the
shorter spelling and the wrong one here, since it unifies into every
`--all-targets` build and leaves the default number path untested.

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

**Name the set where the set has names.** Counting is the weaker form of the
same check, and `every_interceptor_kynos_ships_is_accounted_for` in
[`tests/interceptors.rs`](../crates/kynos/tests/interceptors.rs) is where the
difference showed. A count reports that two numbers differ; a set of type names
reports *which* interceptor nothing accounts for. It also stops two branches
each adding one from colliding, since an alphabetical insertion puts them on
different lines where a shared count puts them on the same one. The same file
now names the `ShortCircuit` set the same way, after eight of its ten members
were found describing a response with no content while sending a problem
document. Eight hand-written cases would have been eight places for a ninth
implementation to be missing from, which is why the set is asserted and the
sweep derives what it must drive from it.

The declared side of each of the three is read off disk, walked rather than
transcribed, over the directory the trait's implementations actually live in:
`src/middleware/` for the interceptor and observer sets, and the whole of
`crates/kynos/src` for the `ShortCircuit` set, because `Infallible` implements
it in `src/response/mod.rs`. Scoping a walk to less than that is this
paragraph's own cautionary tale one level up. A transcribed list is a third
place the set is written down, and it went wrong exactly as that predicts: the
observer counter opened ten files, `compression.rs` was not among them, and an
`Observer` implemented there would have been counted by nothing while both
counters kept passing. Walking the directory removes the list rather than
maintaining it, and lets the check hold at baseline features too, since source
text exists on disk whether or not the feature that compiles it is on.

### The sweep and the matrix assert one property over two sets

`every_short_circuit_declares_the_content_it_sends` and the conformance matrix
both hold a description to the exchange it describes, and both are kept,
because neither set contains the other.

The sweep drives a value of every short circuit this build compiled and can
construct — nine of the ten with every feature on, six at the default set — and
compares what `into_response` wrote against what `Responses` declared, with no
document, no client and no route in between. That reaches the 406 in
`compression` and the 400, 413 and 415 in `decompression`, none of which any
fixture app in the suite provokes. The matrix is the other direction: it holds
whatever actually happened on a live exchange, which includes an application's
own short circuit and a handler — code the sweep cannot enumerate, because its
set is what Kynos ships. The overlap has a measured direction: of the eight
implementations that were declaring nothing while sending a problem document,
the matrix reached five and the sweep reached all eight.

That is also why the sweep stays in `interceptors.rs` rather than moving to
`description.rs` under the rule in the table above. That rule routes static
assertions about a *document* — which statuses and fields an operation
declares. The sweep asserts nothing about a document: it reads two associated
items off one value, and it derives what it drives from the `ShortCircuit` set
asserted a few lines above it in the same file.

## The off-path proof

[`performance.md`](performance.md#the-allocation) grades the document model, the
emitters, the validators and `describe` as *off-path elements*, and an off-path
element owes "a proof it is unreachable from the request path" rather than a
measurement. Zero is not something a counter reports convincingly: a replay that
never exercised the feature counts nothing either, and reads the same.

The proof is stated negatively, because a request path is not a set of files.
Walking `crate::` mentions out of `router/dispatch.rs` does not enumerate one:
`Describe` and `Schema` implementations sit in the same files as the
`FromRequest` and `IntoResponse` implementations beside them, so a file-level
closure either drags in the builder — and fails on `describe` itself — or stops
short and passes vacuously. What is enumerated instead is the off-path side:
each element, the identifier that names it, and the sites allowed to name it.
Every other file is on the request path by default, so a new site is a failing
build until someone adds it to a row and says why a request cannot reach it.

Two things are graded off-path and each gets rows here. The first four rows are
*elements*: the document model, the registry that mints its schemas, the
validators, and the JSON Schema interpreter.
[`performance.md`](performance.md#the-allocation) names four in its shape table
and this is not that list. Three of its four are here — the document model, the
validators and, held by the `yaml` row's emitter site further down rather than
by one of these four, the emitters. The fourth, `describe`, is the one shape no
row holds and cannot be: it is the site allowed by each of the rows whose
element it builds, so a row naming it would be circular. What puts it off the
path is that `Router::build` has returned before a service exists. Going the
other way, the registry is in no shape-table row — it is here because it is what
mints a schema, and the mint site is the stronger claim — and neither is the
JSON Schema interpreter, whose row is the one the README's claim is about and
allows `test/conformance.rs` alone: `describe` does not build it, and nothing on
either side of that row names the other.

The ten rows after those are the *flags*
[`performance.md`](performance.md#the-feature-grading) grades `Off-path proof`,
which owe the same argument one at a time. A flag is a weaker thing to hold than
an element — it names no type — so what a row holds is where the flag is
*written*: the crate its gated code calls, and the `#[cfg(feature = "…")]` that
compiles it. That is enough for the claim being made, and the ten fall into four
kinds. Six compile the code the proof has to keep a request away from: `uuid`
and the four `time`/`decimal` backends contribute `Schema`
implementations, which need the `&mut Registry` only `describe` mints, and
`yaml` contributes an emitter method on a document a request never holds.
`test-util` compiles the conformance harness behind one gate on `pub mod test`,
whose interpreter is the `jsonschema` row above. `time` and `decimal` compile
nothing on their own — each is a `compile_error!` without a backend, which
[`features:check`](../mise.toml) probes — so their rows hold the two files that
say so, and their real cost is their backends' rows. `openapi31` compiles
nothing conditionally at all, which its row states. In every case a gate written
outside the sites its row allows is the first sight of that stopping being true,
and it fails the build.

`macros` is the eleventh flag performance.md could have graded here and does
not: it is graded a full battery, because a derive is a type-level surface that
owes a codegen delta, which an off-path proof does not include.

Which flags belong here is not this document's to decide, and is not
transcribed. `containment.py` reads the `Off-path proof` row of performance.md's
grading table and requires every flag in it to appear in an *Element* cell of
the table below, so regrading a flag into that column is a failing build until
its row exists. That closes the one drift the grading table cannot see on its
own: a full battery either runs or does not and an aggregate owes nothing, but a
proof is an argument, and an argument that was graded and never written reads
exactly like one that was written and holds. The check is forward only — a row
for a flag graded elsewhere is not an error, since an element may be worth
holding under any grade.

**Fourteen rows, and the count is the check.** A row deleted or truncated
away would otherwise leave the gate reporting that every rule holds while the
element it named went unchecked, which is the one failure a gate must not
have. The count is stated here for the reason `architecture.md` states its
allowance count: a table nothing sizes is a table a blank line can silently
halve.

| Element | Named by | Named only in | Why a request cannot reach it |
| --- | --- | --- | --- |
| the emitted document | `Document` | `router/describe.rs`, `router/docs/mod.rs`, `router/install.rs`, `router/mod.rs`, `router/service.rs`, `server/mod.rs`, `server/tls/document.rs`, `test/conformance.rs`, `unchecked.rs` | every site builds it, annotates it, or hands it back to the application. `docs::render` serializes it once while the router is built, so the endpoint serving a description holds finished bytes rather than a `Document`, and `Service` reads it back only through `Service::openapi` |
| the schema registry | `Registry::{new,default}` | `router/describe.rs` | the one registry a build mints is consumed by `describe`, which has finished before a service exists to accept a request |
| the document validators | `Validator` | `router/describe.rs` | a description is validated where it is built. The build either fails or drops the validator, and nothing on the request path holds one to run |
| the JSON Schema interpreter | `jsonschema` | `test/conformance.rs` | it is behind `test-util` and exists to check an observed response against the description. The request parser is the other projection of the same declaration and interprets no schema |
| the `openapi31` feature | `not(feature = "openapi31")` | `lib.rs`, `crates/kynos-openapi/src/lib.rs` | nothing is conditional on it. Both sites are the `#[cfg(not(feature = "openapi31"))] compile_error!` that refuses a build without it, and the 3.1 object model it names is compiled unconditionally. What that model does is held by the document, registry and validator rows above, and a request reaches none of the three |
| the `yaml` feature | `serde_yaml_ng`, `feature = "yaml"` | `crates/kynos-openapi/src/emit/mod.rs`, `error/mod.rs` | `Document::to_yaml` is a method on the emitted document, reached only through `Service::openapi` after the build has finished. `Error::Yaml` carries a failure that emitter produced and is constructible nowhere else |
| the `test-util` feature | `feature = "test-util"` | `lib.rs` | one gate, on `pub mod test`. What it compiles is the conformance harness, whose interpreter is the `jsonschema` row above |
| the `uuid` feature | `uuid`, `feature = "uuid"` | `schema/impls/{mod,identifier}.rs` | its whole contribution is `impl Schema for Uuid`, and `Schema::schema` takes the `&mut Registry` that only `describe` mints |
| the `time` feature | `feature = "time"` | `lib.rs`, `schema/impls/mod.rs` | it carries no types: enabled without a backend it is a `compile_error!`, which `features:check` probes. The two sites are the gate that says so and the module it would open; the cost is its backends' rows |
| the `time-chrono` feature | `chrono`, `feature = "time-chrono"` | `lib.rs`, `schema/impls/temporal/{mod,chrono}.rs` | `Schema` implementations for the crate's date and time types, behind the `&mut Registry` only `describe` mints |
| the `time-jiff` feature | `jiff`, `feature = "time-jiff"` | `lib.rs`, `schema/impls/temporal/{mod,jiff}.rs` | the same, for the other backend: `Schema` implementations reachable only through a registry a build has already consumed |
| the `decimal` feature | `feature = "decimal"` | `lib.rs`, `schema/impls/mod.rs` | as `time`: no types of its own, a `compile_error!` without a backend, and the two sites are the gate and the module it opens |
| the `decimal-rust` feature | `rust_decimal`, `feature = "decimal-rust"` | `lib.rs`, `schema/impls/decimal/{mod,rust_decimal}.rs` | `Schema` implementations for the crate's decimal type, behind the `&mut Registry` only `describe` mints |
| the `decimal-big` feature | `bigdecimal`, `feature = "decimal-big"` | `lib.rs`, `schema/impls/decimal/{mod,bigdecimal}.rs` | the same, for the other backend |

A site path is relative to `crates/kynos/src/` unless it starts at `crates/`,
which makes it relative to the repository root and is how a row names a file in
a sibling crate. The scope [`containment.py`](../scripts/containment.py) counts
a row against follows from the row's own sites: always `crates/kynos/src/`, plus
one `crates/<name>/src/` tree per crate-qualified site. So the widening and the
reason for it are one edit, and a row reaching into another crate cannot be
written without saying where. Deriving it per row rather than declaring one
scope for the table is what makes the widening safe: `Document`, `Registry` and
`Validator` are all *declared* in `kynos-openapi`, and a table-wide scope
spanning both crates would fail those three rows on sight while proving nothing
about the crate a request runs in. A derived tree that is not a directory fails
the row rather than scanning nothing: the derivation is string surgery over a
crate name nothing else here spell-checks, and a misspelled one reads as a legal
site while narrowing the row back to the home scope, where every spelling it
names is still written.

A *Named by* cell holds one of two kinds of token, and may hold several of
either as a comma-separated list of backticked entries. The first is an
identifier or a path of them, brace-expanding the way the *Named only in* column
does when one element has more than one spelling that reaches it:
`Registry::{new,default}` holds both, because `Registry::new` is
`Self::default()` and a row holding only `new` would let a derived `default()`
mint a registry anywhere. The second is a `feature = "…"` gate, written as the
`#[cfg]` attribute writes it, for an element whose whole contribution is what a
gate compiles — a flag is not a Rust name, so there is nothing else to name it
by. A gate is read as the predicate around it rather than as the text of it: the
enclosing `#[cfg]`, `#![cfg]`, `cfg_attr` or `cfg!` is walked with its
delimiters balanced, and the flag counts where that walk reaches it.

The two questions a row asks read that walk at different strictnesses, and the
difference is what a cell's polarity does and does not buy. Whether the cell's
own claim is still live is polarity-exact: the spelling matches only where the
predicate names the flag at the polarity the cell wrote, so a spelling may be
written negated, as `not(feature = "…")`, and then matches only what the flag
compiles by being *off*. The `openapi31` row is the case that needs the negated
spelling, since both of its sites are the `compile_error!` that refuses a build
without the flag. Whether a *site* names the flag is polarity-blind: the
offender scan reports a disallowed site naming the flag at either polarity,
because being off-path is a matter of naming the flag at all, and a cell states
a polarity as the row's own claim rather than as a filter over the tree. So the
positive `test-util` cell reports a disallowed
`#[cfg(not(feature = "test-util"))]`, and the negated `openapi31` cell reports a
disallowed `#[cfg(feature = "openapi31")]`. Write the cell at the polarity the
row's reason is about, and do not expect it to exempt the other polarity from
the offender scan.

The macro is in that list because it is the form that compiles in *every*
configuration and branches at run time, so what it guards is on the request path
in every build; a rule anchored on the attribute forms alone read it as naming
no flag and let the site past in silence. It is read under any of the three
delimiters a macro invocation may take, since `cfg!{…}` and `cfg![…]` gate a
build exactly as `cfg!(…)` does, while the attribute forms are read only under
`(` — `#[cfg{…}]` is not source `rustc` accepts. The name must stand bare:
`other::cfg!`, and the `macro_rules!` metavariable `$cfg!`, resolve to whatever
the module exports or the caller passed, which a scan reading text cannot tell
from the gate, so neither is read as one.

The two kinds of token are matched over different text, and the string literals
are the whole of the difference: an identifier over source with its comments,
its literals and its inline `#[cfg(test)]` modules removed, a gate over the same
source with the literals kept, because the flag name is one. Comments go from
both corpora. A gate written in a comment or a rustdoc example is a mention no
build compiles, and a rule reading one would report a row as holding on the
strength of a sentence about it — a renamed flag staying green off a stale
comment is the failure a naming rule is least able to survive. A feature row
usually carries both — the code its gate compiles, and the crate that code
calls — as in `` `uuid`, `feature = "uuid"` ``. A cell the rule cannot read
fails the build rather than passing quietly, so teaching it a new kind of token
is part of writing the row that needs one.

Each spelling in a cell is held to naming something, one at a time rather than
as a union: a cell written `Registry::{new,defualt}` would otherwise pass on the
strength of `new` while a derived `default()` minted a registry anywhere. What a
spelling must name is a mention anywhere in the scope, sibling test files
included, rather than a site on the request path — a mint spelling earns its row
by being reachable, not by being reached, and the row is at its strongest when
nothing a request can run writes it at all. `Registry::default` is that case
today. A spelling nothing in the scope writes under any `cfg` is a rename or a
typo, and fails the build: it is a row holding nothing rather than an element
nothing reaches.

*Sibling* test files, and not every test: the same strip drops an inline
`#[cfg(test)] mod` body from either corpus, so what a spelling may be found in
is the source a request can run plus the `tests.rs` siblings beside it. That is
the layout rule's own corpus — a module's tests belong in a sibling — rather
than an approximation of "the tests", and it holds for a gate as much as for an
identifier: an inline test module is the other place a gate can sit that no
build outside `cfg(test)` compiles. A failure names which of the two corpora it
read, so a row that has stopped holding says what text it was held against. A
row whose cell the rule cannot read, or whose spelling names nothing, reports
that one failure and stops: its site list goes unchecked until the cell is
repaired, because an offender scan under a spelling already called
untrustworthy would render a verdict nobody should act on.

A cell names the shortest spelling that is unique in the workspace, not the
longest one that is unambiguous. A qualified path is what an import removes:
`use kynos_openapi::validate::{Validator, Violation};` leaves every later
mention of the type bare, so a row written `validate::Validator` would match the
one file that spells the path out and miss the file that imported it. `Validator`
is the whole token because `kynos_openapi::validate::Validator` is the only type
of that name in either crate. Where an identifier is not unique, the row names
what mints one instead. Uniqueness is the writer's judgement and the rule does
not check it — a spelling that names two types would hold both under one reason.
What the rule does check is that the spelling still names something, which is
what catches one that has quietly stopped matching.

`Registry` — the type — is deliberately not a row. `Describe::request_body`
takes `&mut Registry`, which puts the name in some eighty files by design, and a
row admitting all of them would admit anything. The mint site is the stronger
claim and the true one.

The two rules above are the ones this instantiates. **The set is named where the
set has names**: a failure reports which file names an off-path element, not
that two counts differ. **The declared side is read off disk**: the rule
computes the real set of naming files from the stripped source — whichever of
the two strippings the token kind asks for — so the only hand-written thing in
a row is the reason, which is exactly what a reviewer is being asked for when a
build fails here.

The rule cannot see the whole path on its own. `Dispatch` hands every request to
a trait object — `dyn ErasedTerminal`, `dyn ErasedInterceptor`, `dyn Observer`,
`dyn ErasedLayer` — and what sits behind one is declared in another file, which
a naming rule reads as an allowed site rather than as the request path. The
other half is a witness fn:
[`router/dispatch/tests.rs`](../crates/kynos/src/router/dispatch/tests.rs)
destructures `Dispatch`, `PathEntry` and `Served` exhaustively, so a field added
to any of the three stops the crate compiling until someone writes it into the
pattern. Nothing the dispatch table hands to an erased callee *that it stored
while the router was built* is something those three do not carry.

The qualifier is load-bearing and the unqualified form is false: `serve` hands
the callee the `Request`, and an `Observer` is handed a `Duration` and a
`&Response`, none of which is a field of any of the three. What the witness
pins is the stored half — the table's own shape — and that is what a new field
on it would change.

Neither does the pair compose into "a request cannot reach a `Document`". The
naming rule is per *file*, and three of the sites the document row allows —
`unchecked.rs`, `server/mod.rs` and `router/docs/mod.rs` — serve requests
themselves, so a new use of `Document` *inside* one of them is allowed by the
row and invisible to the witness. Read the two together as what they are: a
per-file naming rule, plus a ratchet on the dispatch table's fields. Narrowing
the allowance below file granularity is what would close that, and is filed as
[#131](https://github.com/getkono/kynos/issues/131).

That is narrower than "nothing reaches an erased callee", and deliberately.
`Service` is above the table: it owns the `Document` and hands the request to a
`dyn ErasedService` built where the document is in scope, so no field of the
three types witnesses it. That seam is held by the document row instead —
`router/describe.rs` and `router/service.rs` are two of the sites it allows, and
the row's reason is the argument for both. The two halves meet there: a witness
where a field carries something to an erased callee, a row where a file names
something the witness cannot see.

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

This rule is upheld by review and by that ledger, not by a counter — which
makes it the one exhaustiveness claim here that is *intended* rather than
asserted, against what "exhaustiveness is asserted, not intended" asks of the
rest. [`tests/ui.rs`](../crates/kynos/tests/ui.rs) says so where it
counts the schema table. Wiring it means reconciling 91 negatives against 92
controls first — a surplus on the control side, which is the harmless
direction — and the pairing is by meaning rather than by filename, so nothing
on disk says which control stands alone.

That ledger is where the rule earns its keep. `#[kynos::operation]` was
scheduled for two negatives, both of which produced exactly the right
diagnostic; no control could be written for either, because the attribute was
broken and *no* program using it compiled. Nothing else in the suite would have
noticed.

The `compile_fail` doctests that remain are a separate matter. Most have a
control beside them and some do not — a count nothing asserts, which is the
same gap as above rather than a second one; each is a single rule stated where a
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

[`alloc.rs`](../crates/kynos/tests/alloc.rs) used to rest on it and no longer
does. `stats_alloc` counted into process globals rather than thread locals, so
its tests contaminated each other as threads of one binary and, worse, were
contaminated by the harness thread `libtest` keeps alive beside the one running
a test — one process per test does not make one thread per process, and that
residue moved a replayed request's count on roughly one request in ten
thousand. `alloc_counter` counts per thread, so the file is now correct by
construction and passes with each of its tests on a concurrent thread of one
process.
[`work_on_another_thread_is_not_counted`](../crates/kynos/tests/alloc.rs) is
the assertion that holds the counter to it, once for both counted targets: the
property is `alloc_counter`'s rather than any fixture's.
`the_counter_reports_every_heap_operation_in_the_region` is filed beside it for
the same reason, and holds as much of the other half as a fixture can reach:
that a driver which stopped counting one of the two kinds of heap operation —
fresh allocations, reallocations — goes red. A driver under-reporting both
alike is cancelled by the delta it is asserted over, and is recorded in that
test's docblock rather than held. Both are properties of the counter and of the
one driver both targets share, so `alloc_codecs.rs` restates neither.

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

**So must its components.** A `const` assertion that fails — the interceptor
collision checks in [`middleware/stack.rs`](../crates/kynos/src/middleware/stack.rs)
are the ones in this tree — surfaces with its primary span in `core`'s own
`panic.rs`. rustc prints that line when it can read it and degrades to a bare
`note:` when it cannot, so whether `rust-src` is installed changes the recorded
text. `mise.toml` therefore lists it: a snapshot suite that passes on the
machine that recorded it and fails everywhere else is testing the environment.

**`on_unimplemented` attributes must land before any snapshot is recorded.**
Eighteen traits carry `#[diagnostic::on_unimplemented]` —
`Provides`, `Handler`, `FromRequestParts`, `FromRequest`, `Describe`,
`RequestContent`, `IntoResponse`, `Responses`, `Schema`, `MapKey`,
`Alternative`, `ShortCircuit`, `EndpointMeta`, `IntoEndpoints`, `Carries`,
`Languages`, `Rangeable` and `ByteSource`.
Each one replaces the compiler's generic "the trait bound is not satisfied"
with a message naming the fix.

`every_guided_diagnostic_has_a_snapshot` in
[`tests/ui.rs`](../crates/kynos/tests/ui.rs) maps each to the snapshot that
records it and counts the pairs against the attributes in the source. Eight of
the fourteen guided traits *at the time it was written* had none, so more than
half of what this requirement named was unchecked; the count has grown to
eighteen since, and the test is what kept the mapping level with it. The
mapping is written out rather than searched for, because half the
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
harness tests the framework's *claim* — that a running service never returns a
response the emitted document omits. Without it, the soundness invariant in
[`middleware.md`](middleware.md) would be an intention held up by review.

It is built, and it runs in two places.
[`tests/conformance.rs`](../crates/kynos/tests/conformance.rs) is the narrow
one, over the two operations `examples/testing.rs` assembles.
[`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) is the wide one, over every
layer Kynos owns: the operations, a credential guard, a `WithHeaders` return, a
redirect, a query group, and every interceptor at router or group scope.

It was built last rather than first because it needs a service that actually
runs. That ordering was a fact about the schedule, not a judgement about
priority — and the section below records what it found the first time it was
pointed at something.

## What the harness found on its first run

Twice now, the answer has been a defect no other kind of test in this document
could have seen. Both are recorded here because the harness is expensive to
justify on principle and cheap to justify on evidence.

**A 413 no operation could produce.** `BodyRejection` declared `413` on every
operation that reads a body, and the only thing that ever produced one was
`middleware::limits::BodySize`. A service without that limit therefore promised
a response it could not send. Line coverage cannot see this: every line of the
declaration runs, and the gap is between the document and the service rather
than inside either. The fix was to remove the variant — recorded at
[`error/rejection.rs`](../crates/kynos/src/error/rejection.rs)'s `TooLarge`
comment — and it is what let `every_declared_response_is_exercised` stop being
`#[ignore]`d.

**A response header declared where nothing resolves it.** An interceptor's
`Adds` group was filed under the `2XX` wildcard beside the operation's declared
`200`. The specification resolves an observed status to the exact key first, so
no reader of that 200 ever saw the header — and the `2XX` entry was a response
nothing could produce. `tests/matrix.rs` reported it nine times, once per
operation, on the first run of `assert_declared_responses_covered` over the
whole owned-layer matrix. Every other test in the suite passed throughout.

Both share a shape worth naming: the code was right, the document was wrong, and
the two disagreed about a *status* and a *header key* — which a live exchange
checked against the description is what exposes, because neither end alone holds
both halves. A claim about a *type* is the other case and takes the direct
assertion instead: the sweep in `tests/interceptors.rs` reads each short
circuit's declaration against the response it builds, with no request at all.
[`middleware.md`](middleware.md#declaring-is-not-describing) states that
division from the interceptor side.
