# Design Documentation

This directory records the decisions that shape Kynos, and is normative for
them. Where a document here states a rule, that rule binds implementation work.

It does not restate the specifications themselves. Standards texts and their
precedence live in [`references/`](../references/README.md); the case for the
framework, and the list of features it deliberately rejects, lives in the
[README](../README.md).

## Documents

| Document | Consult when |
| --- | --- |
| [`architecture.md`](architecture.md) | Touching the runtime boundary, adding a dependency, or introducing anything to the public API surface |
| [`state.md`](state.md) | Working on dependency injection, the application context, or anything a handler receives that the request did not carry |
| [`handlers.md`](handlers.md) | Working on extraction, responses, or what a handler signature is allowed to say |
| [`schema.md`](schema.md) | Working on how a Rust type becomes JSON Schema: which types are describable, what `format` each carries, and where a constraint may be declared |
| [`errors.md`](errors.md) | Working on failure: what a handler returns when it cannot succeed, how an extractor rejects, and what reaches a client |
| [`routing.md`](routing.md) | Working on path templates, routers, groups, or how router scope becomes document scope |
| [`middleware.md`](middleware.md) | Working on interceptors, contributions, escape hatches, or `tower` interoperability |
| [`testing.md`](testing.md) | Deciding where a test belongs, or what kind of test a guarantee needs |
| [`nfr.md`](nfr.md) | Adding a module-level guarantee, or deciding what a change must prove before it lands |

## The anti-patterns are normative

The eleven [anti-patterns](../README.md#anti-patterns) live in the README
because that is where someone evaluating Kynos will look for them, but they
bind implementation work exactly as anything here does —
[`architecture.md`](architecture.md#invariants) derives them from its three
invariants, which is only meaningful if the list is a contract rather than
marketing. Each has a normative home in one of the documents above, which is
where its enforcement point is recorded.

## Normative and non-normative material

Sections headed `Policy`, `Invariants` or naming a requirement are normative. A
section headed `Rationale` is not: it records why a rule exists so the rule can
be argued with honestly later, rather than merely overturned. Nothing in a
`Rationale` section is an instruction.

## Relationship to the code

These documents describe the intended contract, which the pre-v1 API skeleton
does not yet implement in full. Where a document specifies something that does
not exist, it says so explicitly and names the state as designed rather than
built. That gap is the point: the contract is written first so the
implementation has something to satisfy.

Benchmark methodology is not here. It lives with the harness that runs it, in
[`getkono/kynos-bench`](https://github.com/getkono/kynos-bench), because a
measurement definition is worth little separated from the code that produces the
measurement.
