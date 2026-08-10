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

> **Not yet implemented.** This section specifies the contract the middleware
> and router implementation must satisfy. No `Opaque` type exists today.

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

| Escape hatch | Today | Contract |
| --- | --- | --- |
| [`Unchecked<T>`](../crates/kynos/src/schema/unchecked.rs) | Per-item annotation, reported by `validate` | Unchanged — already correct, and the model for the rest |
| `route_unchecked` | Operation absent from `paths` | Operation emitted, flagged `Opaque` |
| `layer_unchecked`, `into_tower_unchecked` | Whole document stamped non-authoritative | `Opaque` on the covered subtree only |
| `upgrade_unchecked` | Absent from `paths` | Still absent, and still reported by `validate` |

`upgrade_unchecked` is the one exception, and it is not a gap to be closed
later. OpenAPI describes HTTP request/response semantics; a connection that has
upgraded away from HTTP has no vocabulary in any version of the specification.
Emitting an entry no consumer could act on would be worse than the honest
absence, which `Router::validate` reports regardless.

`x-kynos-document-not-authoritative` survives as a *derived* summary — true when
any operation is `Opaque` — rather than as the mechanism. That preserves the
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

## Conformance

The invariant is a claim about a running service, so it needs to be tested like
one. A conformance harness property-tests live responses against the emitted
document across the full matrix of owned layers, in CI.

Without that harness this document is marketing. The requirement is recorded in
[`nfr.md`](nfr.md#middleware).

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
