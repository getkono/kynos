# Middleware

## The rule

A layer's type must declare everything it can do to a response that the
handler's type does not already say.

Everything below follows from that sentence. The mechanism is the
[`Interceptor`](../crates/kynos/src/middleware/mod.rs) trait's three associated
types, and the reason they are a closed set rather than an open one is that an
interceptor doing something the set cannot express is doing something OpenAPI
cannot describe.

| Associated type | What it declares | What it obliges |
| --- | --- | --- |
| `Short` | the responses it can answer with alone | `Err(Short)` is the only way to answer without the handler |
| `Adds` | the response headers it attaches | `Continued<Adds>` is the return type, so it must attach them |
| `Reads` | the request headers it consumes | it is handed that group, and nothing else |

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

## Why the declaration is the signature

Three properties, each load-bearing:

**It is inert data.** What an interceptor declares is read from its types, so it
is inspectable without running the service — or, for the parts that are `const`,
without running anything at all. If learning what the stack emits required
executing the stack, the guarantee would be gone: a document you can only obtain
by running the server is a document you cannot check in CI.

**It cannot disagree with behaviour.** There is no `contribution` method,
because a method returning a description beside a method producing responses is
two statements of one fact. `Short` is both the declaration and the only way to
answer; `Adds` is both the declaration and the return type. An interceptor that
declares a 401 it never sends, or a header it never attaches, does not compile.

**Conflicts are a compile error.** Two interceptors covering one route and both
claiming 429, or both setting `Retry-After`, are rejected by
`Router::intercept`'s bound rather than by a check at build time. What survives
in [`ContributionConflict`](../crates/kynos/src/middleware/contribution.rs) is
the vocabulary for the subtrees where the types are erased and the check cannot
run — those taken under `layer_unchecked`.

**It applies per-operation, after routing.** An interceptor mounted on a subtree
covers the operations in that subtree and nothing else. Scope in the document
matches scope in the router.

## Declaring is not describing

Every header an interceptor sets is *declared*, so the conflict check sees it.
Whether it is *described* is a separate question, answered by
`HeaderParams::DESCRIBED`.

`Vary`, `Content-Encoding` and the CORS set are defined by HTTP itself and
handled by every client without being told, so their groups set `DESCRIBED` to
`false` and stay out of the emitted document. This does not weaken anything: a
second interceptor touching one of those names still fails to compile. The two
questions are "can this collide" and "does a consumer need to hear about it",
and only the first is about correctness.

A body is declared nowhere at all — `Continued::take_body` and
`Continued::set_body` need no declaration, because a body has no name to collide
on. Two interceptors rewriting one compose; two setting one header do not. An
encoding a consumer must know about is a header, which is why `Compression`
declares `Content-Encoding` rather than re-encoding silently.

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

Interceptors are erased *for execution*, at every level. A router, a group and
an endpoint hold the same list and run it the same way.

That is not a performance trade against a statically composed chain — it is the
only shape available.
[`Next<'a, C>`](../crates/kynos/src/middleware/mod.rs) has two parameters and
appears in `Interceptor::intercept`'s signature, so a chain composed in the
type system would need a tail parameter that infects every interceptor anyone
writes. The terminal is boxed regardless, since `Endpoint` returns an opaque
future and a router holds endpoints it cannot name. A route with no
interceptors calls the terminal directly and pays nothing.

A phantom list of interceptor *types* rides alongside for checking, which the
objection above does not reach: it is not a composed chain, nothing is called
through it, and `Next` keeps its two parameters. `Router`, `Group` and
`EndpointBuilder` each carry one, and
[`CompatibleWith`](../crates/kynos/src/middleware/stack.rs) is what
`intercept` bounds on — so two interceptors that would collide are rejected
where the second is mounted.

Whole stacks meet at `group`, `nest`, `merge` and `mount`, and `CompatibleStack`
checks that cross-product. `mount` is why `routes!` expands to a tuple rather
than to an `Endpoints`: a collection cannot say what its members carry, so
building one would erase the endpoint-scoped interceptors the check needs. Two
*operations* are never checked against each other, since no request reaches
both.

What the check compares is `const` data — `HeaderParams::NAMES` and
`ShortCircuit::STATUSES` — so it costs nothing at run time and nothing in the
emitted document. Header names compare case-insensitively, per RFC 9110.

An interceptor is handed the [`Route`](../crates/kynos/src/router/operation.rs)
it covers while it runs, through `Next::route`. That is what keeps a metric
label keyed by the `paths` key rather than by the request path, so label
cardinality is bounded and the label cannot disagree with the description.

It is *not* handed one while declaring, because there is no declaring step left
to hand it to. Declaring differently per operation is expressed by mounting
different instances at different scopes — which is the same principle as above,
and which every interceptor Kynos ships was already doing: all seven ignored the
`Route` argument the old `contribution` method gave them.

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
