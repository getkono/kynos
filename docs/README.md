# Design Documentation

This directory records the decisions that shape Kynos, and is normative for
them. Where a document here states a rule, that rule binds implementation work.

It does not restate the specifications themselves. Standards texts and their
precedence live in [`references/`](../references/README.md), and
[`standards.md`](standards.md) is the mapping between the two — which document
governs which module, and where each departure from one is argued. The case for
the framework, and the list of features it deliberately rejects, lives in the
[README](../README.md).

## Documents

| Document | Consult when |
| --- | --- |
| [`architecture.md`](architecture.md) | Touching the runtime boundary, adding a dependency, or introducing anything to the public API surface |
| [`state.md`](state.md) | Working on dependency injection, the application context, or anything a handler receives that the request did not carry |
| [`handlers.md`](handlers.md) | Working on extraction, responses, or what a handler signature is allowed to say |
| [`schema.md`](schema.md) | Working on how a Rust type becomes JSON Schema: which types are describable, what `format` each carries, and where a constraint may be declared |
| [`errors.md`](errors.md) | Working on failure: what a handler returns when it cannot succeed, how an extractor rejects, and what reaches a client |
| [`security.md`](security.md) | Working on authentication: where a credential travels, what a guard declares, and what Kynos deliberately does not verify |
| [`routing.md`](routing.md) | Working on path templates, routers, groups, or how router scope becomes document scope |
| [`middleware.md`](middleware.md) | Working on interceptors, contributions, escape hatches, or `tower` interoperability |
| [`standards.md`](standards.md) | Working on anything a specification governs: which document and section binds a middleware, where a departure is argued, and what is known not to conform |
| [`testing.md`](testing.md) | Deciding where a test belongs, or what kind of test a guarantee needs |
| [`nfr.md`](nfr.md) | Adding a module-level guarantee, or deciding what a change must prove before it lands |
| [`performance.md`](performance.md) | Deciding what a feature costs the request path, and which measurement that shape of code owes |
| [`releasing.md`](releasing.md) | Cutting a release, or diagnosing why one did not publish |

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

One document is not normative at any heading: [`releasing.md`](releasing.md) is
a runbook. It records what a person does to cut a release, which is a procedure
rather than a rule binding the code.

## Relationship to the code

These documents describe the contract. It is written first, so the
implementation has something to satisfy — and where a document still specifies
something that does not exist, it says so explicitly and names the state as
designed rather than built. [`nfr.md`](nfr.md) is where that ledger is kept:
it records which guarantees CI actually enforces, and it is worth keeping only
while it never claims one that CI does not.

Benchmark methodology is not here. A measurement any HTTP server library would
answer — throughput, tail latency, resident memory at scale — lives with the
harness that runs it, in
[`getkono/kynos-bench`](https://github.com/getkono/kynos-bench), because a
measurement definition is worth little separated from the code that produces the
measurement.

What a *Kynos* feature costs the request path is a different question, and it is
here: no comparison answers it, because no other library has the feature.
[`performance.md`](performance.md) holds the boundary and allocates a counted
method to each shape of the routing stack.
