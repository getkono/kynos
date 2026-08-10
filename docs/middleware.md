# Middleware

## The rule

A layer's type must declare everything it can do to a response that the
handler's type does not already say.

Everything below follows from that sentence. The mechanism is
[`OperationContribution`](../crates/kynos/src/middleware/contribution.rs), and
the reason it
is a closed set rather than an open one is that an interceptor doing something
the set cannot express is doing something OpenAPI cannot describe.

## Soundness, not exactness

The invariant is that the emitted document is a superset of observable
responses:

> emitted spec ⊇ observable responses

It is deliberately not equality. Exactness is unenforceable — a panic, an
unhandled 500, or an upstream proxy can all produce a response no type in the
program predicted — and soundness is the property consumers actually depend on.
A client that handles every documented response and encounters an undocumented
one has been lied to. A client that handles a documented response which never
occurs has merely written dead code.

Stating the weaker invariant is what makes it enforceable, and an enforceable
weak claim is worth more than an unenforceable strong one.

## OperationContribution

Three properties, each load-bearing:

**It is inert data.** A contribution is inspectable at build time without
running the service. `Interceptor::contribution` is called once per operation
while the router is built, never per request. If learning what the stack emits
required executing the stack, the guarantee would be gone — a document you can
only obtain by running the server is a document you cannot check in CI.

**Composition is order-sensitive.** Contributions do not commute. Compression
rewriting headers after authentication has added a 401 produces a different
document than the reverse order, and the composition rules must reflect that
rather than sorting it away. `merge` returning
[`ContributionConflict`](../crates/kynos/src/middleware/contribution.rs) is the
other half of
this: two interceptors that disagree about what a 429 means are caught when the
router is built, not in production.

**It applies per-operation, after routing.** An interceptor mounted on a subtree
contributes to the operations in that subtree and to nothing else. Scope in the
document matches scope in the router.

## Opaque

The vocabulary is
[built](../crates/kynos-openapi/src/annotation/mod.rs) and
[checked](../crates/kynos-openapi/src/validate/rules/opaque.rs); what still
has a `todo!()` body is the router side that produces it.

`unchecked` and `Opaque` are cause and effect, not alternatives, and the
distinction is worth being precise about:

- **`unchecked` is the waiver.** Author-side, opt-in, feature-gated. It is the
  application owner asserting they know what they are doing, and it stays
  exactly as it is.
- **`Opaque` is the record that waiver leaves on the document.** Framework-side,
  derived, per-operation. Nobody writes `Opaque` by hand.

The invariant: **`Opaque` marks affected operations unverified, and never omits
them.** A document that silently drops an operation is worse than one that flags
it, because the omission is invisible to the consumer that trusts it.

Today the escape hatches in `crates/kynos/src/unchecked.rs` have three different
blast radii for the same underlying situation. The waiver must mark exactly what
it reaches:

| Escape hatch | Record | Where |
| --- | --- | --- |
| [`Unchecked<T>`](../crates/kynos/src/schema/unchecked.rs) | `x-kynos-unchecked` on the schema | Reporting is built; the annotation is emitted by a `todo!()` body |
| `layer_unchecked`, `into_tower_unchecked` | `x-kynos-opaque` on each covered operation | The covered subtree only, never the whole document |
| `route_unchecked` | An entry in `x-kynos-opaque-routes` | The document root; no `paths` key |
| `upgrade_unchecked` | An entry in `x-kynos-opaque-routes` | The document root; no `paths` key |

**A route with no expressible template gets no `paths` key.** This was once
specified as "operation emitted, flagged `Opaque`", which cannot be honoured:
a catch-all matches a set of paths that no single template describes, so every
key that could be minted is a claim about either the path or a parameter that
the service does not honour. The literal wildcard mints a parameter named
`*path`; a synthesized `{path}` promises a value that never contains an
unescaped `/` and always does; the bare prefix claims an operation that 404s.

Soundness does not require a `paths` entry — it requires that a consumer is
never *unaware* of what the service serves. A root-level array is visible,
greppable, diffable and reported by `validate`, and it leaves the path rules
in [`validate/rules/paths.rs`](../crates/kynos-openapi/src/validate/rules/paths.rs)
meaning what they say.

That also dissolves the `upgrade_unchecked` exception rather than adding one.
A connection that has left HTTP has no vocabulary in any version of the
specification, so it too is a route with no expressible template: same record,
different reason, no special case.

Two records rather than one stretched over both, because there are two
situations. `layer_unchecked` covers *real operations on real paths* whose
behaviour is unverified — the path is true, so the operation stays in `paths`
and carries a marker. `route_unchecked` has no operation to hang a marker on.

`x-kynos-document-not-authoritative` survives as a *derived* summary — true when
any operation is opaque or any route is recorded — rather than as the
mechanism. It is recomputed rather than set, in both directions, so a document
edited after the fact cannot keep a stamp it no longer earns or lose one it
does. That preserves the
one-glance signal for a consumer while confining the damage to the operations
actually affected: one unchecked layer on one subtree must not taint three
hundred operations it never touches.

## Tower interop

Interoperability is asymmetric, and deliberately so:

- **Outward is free.** A Kynos application mounts into an outer `tower` stack
  without declaring anything. Whatever wraps the service is outside the document
  and always was — `Service::into_tower_unchecked` already exists for this.
- **Inward requires a declaration.** A `tower` layer placed *inside* the stack
  must either be expressed as an `Interceptor` with a contribution, or be taken
  under a waiver that marks its subtree `Opaque`.

This keeps the invariant intact while leaving the ecosystem's migration path
open, which matters more than purity: an application that cannot adopt Kynos
incrementally will not adopt it.

## Composition

Interceptors are erased, at every level. A router, a group and an endpoint hold
the same list and compose it the same way.

That is not a performance trade against a statically composed chain — it is the
only shape available.
[`Next<'a, C>`](../crates/kynos/src/middleware/mod.rs) has two parameters and
appears in `Interceptor::intercept`'s signature, so a chain composed in the
type system would need a tail parameter that infects every interceptor anyone
writes. The terminal is boxed regardless, since `Endpoint` returns an opaque
future and a router holds endpoints it cannot name. A route with no
interceptors calls the terminal directly and pays nothing.

An interceptor is handed the [`Route`](../crates/kynos/src/router/operation.rs)
it covers, both when it declares its contribution and when it runs. Declaring
per-operation keeps the contribution inert — the operation is known at build
time — while letting one interceptor say different things about different
operations. Running with it is what keeps a metric label keyed by the
`paths` key rather than by the request path, so label cardinality is bounded
and the label cannot disagree with the description.

## Conformance

The invariant is a claim about a running service, so it needs to be tested like
one: a conformance harness property-testing live responses against the emitted
document across the full matrix of owned layers, in CI.

That harness does not exist. Until it does, the soundness invariant above is an
intention rather than a guarantee, and this section is a requirement rather
than a description. It is recorded in [`nfr.md`](nfr.md#middleware).

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### What a tower layer actually does to completeness

Four behaviours account for essentially all of it:

1. **Short-circuiting** — returning 401 or 429 without reaching the handler, so
   the document omits a status the service demonstrably returns.
2. **Body and header rewriting** — changing the shape of a response the handler's
   type already described.
3. **Route injection** — serving a path that appears nowhere in `paths`.
4. **Retry** — silently altering the idempotency semantics a consumer reasons
   about, without changing any individual response.

The first three are visible in a response. The fourth is not, which is why
declaration rather than observation is the only mechanism that catches it.

### The cost of owning the common layers

Owning the common layers has an ergonomic dividend beyond correctness: it
removes `tower-http` version-skew pain, which is a real and recurring tax on
applications in this ecosystem.

The price is bus factor — on the order of fifteen crates of ongoing maintenance.
The mitigation is to keep each one small enough that a single contributor can
hold it in their head and own it.
