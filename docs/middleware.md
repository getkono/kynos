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

**Where a described header lands.** An interceptor's `Adds` group is filed
against the *successful* responses the operation already declares, one entry
each — not under a `2XX` wildcard beside them. A consumer resolving a status
takes the exact key first, so a header under `2XX` next to a declared `200` is
one no reader of that operation's 200 will ever find, and the `2XX` entry is
then a response the service cannot produce. An operation declaring no success
at all — a redirect — gets no entry rather than an invented one: the header is
still sent, and understating a description by one header beats claiming a
response that does not exist.

A body is declared nowhere at all — `Continued::take_body` and
`Continued::set_body` need no declaration, because a body has no name to collide
on. Two interceptors rewriting one compose; two setting one header do not. An
encoding a consumer must know about is a header, which is why `Compression`
declares `Content-Encoding` rather than re-encoding silently.

## Why the rate-limit headers keep a prefix

`RateLimit` emits `X-RateLimit-Limit`, `X-RateLimit-Remaining` and
`X-RateLimit-Reset`, and RFC 6648 has deprecated `X-` prefixes for new headers
since 2012. The choice is deliberate.

The unprefixed triple belongs to `draft-ietf-httpapi-ratelimit-headers`, which
has already *replaced* it with a single structured `RateLimit` field plus
`RateLimit-Policy`. These names are `DESCRIBED`, so they reach generated
clients — which makes a wrong name expensive rather than cosmetic. Emitting the
unprefixed triple would squat three names a working group is actively revising,
and claiming settled ground that is not settled is the failure this project's
architecture notes exist to catch.

The reversal is cheap and additive when the draft lands: a second `HeaderParams`
group and a type-state transition on `RateLimit`, shaped exactly like
`Cors::document_response_headers`.

One thing worth knowing about the two halves. The 429's headers ride on
`Responses`; a success's ride on `Adds`. They never co-occur on one response,
because a short-circuit never calls `with_headers` — so the conflict check,
which compares `Adds` against `Adds`, is not weakened by the pair.

## What bounds a request before an interceptor runs

An interceptor covers the operations in its subtree, which means it runs after
routing — and a request that never reaches routing is bounded by something else
or by nothing. The table is here because "does Kynos have payload limits" has
four different answers depending on which layer is asked.

| Vector | HTTP/1 | HTTP/2 | Server | Interceptor | Bounded by default? |
| --- | --- | --- | --- | --- | --- |
| Request line and URI length | must fit `max_buffer_size` (≈417 KiB) | `max_header_list_size`, 16 KiB | — | — | yes, loosely |
| Header count | `max_headers`, 100 → 431 | by list size rather than count | — | — | yes |
| Header-list size | `max_buffer_size` | `max_header_list_size` | — | — | yes |
| Query-string length | subsumed by the URI | subsumed by the list size | — | — | yes, loosely |
| Body size | — | — | — | `BodySize`, when mounted | **no, deliberately** |
| Request-head read time | `header_read_timeout`, 30 s | n/a | — | — | yes |
| Slow body | — | — | — | `Timeout`, *outside* `BodySize` | **no** |
| Keep-alive idle | `header_read_timeout` covers the wait for the next head | `Http2KeepAlive`, unset | — | — | HTTP/1 only |
| Handler runtime | — | — | — | `Timeout`, when mounted | **no** |
| Total connections | — | — | `max_connections`, 10 000 | — | yes |
| Per-IP connections | — | — | — | — | **no**, and see below |
| Concurrent in flight | — | `max_concurrent_streams`, 200 per connection | — | `Concurrency`, when mounted | partial |
| Request rate | — | — | — | `RateLimit`, when mounted | no |
| Request smuggling | hyper and `httparse` | n/a | — | — | yes, and not Kynos's |
| Reset flood | — | `max_pending_accept_reset_streams`, `max_local_error_reset_streams` | — | — | yes |
| TLS handshake stall | — | — | `handshake_timeout`, 10 s | — | yes, with `tls` |
| Decompression bomb | — | — | — | — | n/a: Kynos never decompresses a request body |

Three rows are worth reading twice.

**A body cap is not default, and that is a decision.**
[`nfr.md`](nfr.md#extraction) records the three reasons. The shortest is that a
default limit would add 413 to every operation of every application that never
asked for one — and this framework's whole position is that a declared response
is a promise.

**The slow-body row depends on mounting order.** `BodySize` reads a length-less
body frame by frame, so a client sending one frame slowly holds that loop open.
`Timeout` wraps whatever is beneath it, which means it bounds the read only when
it is mounted *outside* the limit doing the reading. The types do not enforce
the order; `a_timeout_mounted_outside_a_body_limit_bounds_the_read` in
[`tests/limits.rs`](../crates/kynos/tests/limits.rs) pins it, and this paragraph
is where a reader learns it.

**A per-IP cap is absent rather than pending.** Behind a load balancer every
connection arrives from one address, so a cap counted in-process is either
meaningless or a self-inflicted outage. An honest one needs to know which
forwarded-for headers to trust, which is a security policy rather than a limit.

### A response no type predicts

The soundness invariant is *emitted ⊇ observable responses* for the responses
Kynos produces. A **431** from hyper's own header parsing is not one of them: it
is written by the protocol driver before any route matched, so it reaches no
operation and no `Responses` implementation ever saw it. It joins the panic, the
unhandled 500 and the upstream proxy on the list of responses the invariant does
not reach — named here rather than left to be discovered, because a consumer
meeting one is entitled to know Kynos never claimed otherwise.

## Preflight

A CORS preflight is answered by the router, not by a chain.

It is registered as an operation on the matched path while the service is built,
after the description has been assembled. That ordering is the whole design:

- **It contributes no `paths` key.** Not because a filter removes it, but
  because `describe` had already finished when it was created.
- **It appears in no `Allow` header.** The `Allow` loop runs before
  registration, so a 405 still names only the operations the description
  declares.
- **A path that declares its own `OPTIONS` gets no synthesized one.** The
  user's operation wins by construction rather than by a race.
- **It runs no interceptor.** A browser sends a preflight with no credentials
  and no `Authorization`; an auth interceptor short-circuiting it would break
  CORS for every operation on the path. `middleware.md` says an interceptor
  covers the *operations* in its subtree, and a preflight is not one. Observers
  still see it, which is right — a preflight is worth logging.

An `OPTIONS` that is *not* a preflight — no `Origin`, or no
`Access-Control-Request-Method` — is answered exactly as it was before CORS was
mounted: the same `method_not_allowed` policy, the same `Allow` value.

The methods a preflight advertises are the ones the covering scope declares, so
a `Cors` on a group owning `GET /x` advertises `GET` even where the router also
owns `POST /x`. `Cors::allow_methods` overrides that, for a deployment fronting
routes Kynos does not serve.

**A path can be covered by more than one `Cors`.** A group's interceptor stack
is checked against the router's and never against a sibling's, on the premise
that no request reaches two operations — and a preflight is the request that
does, since it is answered once per path. So the answer is assembled per scope:
`Access-Control-Request-Method` picks the configuration whose real response will
honour it, and a proposed method no scope covers falls back to the first, which
refuses it in the advertised list either way.

**One limit.** An endpoint-scoped `Cors` answers no preflight: an endpoint's own
interceptors stay inside the endpoint, which is what runs them, so preflight
registration cannot see them. Mount CORS at a router or group scope.
`a_cors_mounted_on_one_endpoint_answers_no_preflight` records the behaviour, so
closing the gap turns a test red rather than nothing.

## The one interceptor the router recognises by identity

Everything above is read from an interceptor's *types*. There is exactly one
exception, and it is bounded rather than general: while the router is built, it
downcasts each interceptor to `Cors` and asks the configuration two questions
the type system cannot answer.

Both are about a *value*. `allow_any_origin` and `allow_credentials` are
`mut self -> Self` builders on purpose — an allow-list read from the environment
at startup has to be applied conditionally — so whether they were both called is
not a fact a `const` can see. The two questions are whether the pair was
selected, which is refused (`Error::Middleware`), and what a preflight on the
covered paths should answer.

What stops this becoming a capability:

- The match is a closed list of two concrete types. `CorsDocumentation` is
  sealed, so there cannot be a third, and
  `every_cors_documentation_state_is_one_of_the_two_the_router_recognises`
  fails if one is added without teaching the probe about it.
- A third-party interceptor is never asked. `ErasedInterceptor::as_any` is
  `pub(crate)`, and there is no trait method an outside implementation could
  supply to be read this way.
- **Nothing read here reaches the description.** The refusal stops a document
  being produced at all; the preflight is registered after `describe` has
  finished. So the property this document opens with — that a declaration
  cannot disagree with behaviour, because it is the same text — is untouched.

## Vary is declared apart from the names

`Vary` is the one response header two interceptors may both contribute to, so it
has a channel of its own: `HeaderParams::VARIES` rather than `NAMES`.

RFC 9110 §12.5.5 defines it as an unordered set of field names. `Compression`
varies on `Accept-Encoding` and `Cors` varies on `Origin`, and both belong on the
same response — a browser-facing service wants that pairing. Naming `vary` in
`NAMES` would make it a compile error, and the conflict check is right about
every *other* header, so the fix is to stop calling this one a conflict rather
than to weaken the check.

Kynos merges what is declared into whatever `Vary` the response already carries:
case-insensitively, because a field name is; and never past `*`, which already
says the response depends on more than field names can express. The merge runs
in both places a group reaches the wire — `Continued::with_headers` and
`WithHeaders::into_response` — so the two cannot disagree.

`VARIES` is never described. A shared cache reads `Vary`; a client generator has
no use for it. Getting this wrong is not a missing nicety: a CORS response that
varies on `Origin` without saying so lets a cache hand one origin's
`Access-Control-Allow-Origin` to another, which defeats the check entirely.

## Opaque

The vocabulary is
[built](../crates/kynos-openapi/src/annotation/mod.rs) and
[checked](../crates/kynos-openapi/src/validate/rules/opaque.rs), and the router
side that produces it is
[in `router/service.rs`](../crates/kynos/src/router/service.rs)'s `mark_opaque`.

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
| [`Unchecked<T>`](../crates/kynos/src/schema/unchecked.rs) | `x-kynos-unchecked` on the schema | The schema only; the operation is not marked, because a hand-written schema is not an undeclared effect |
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

### Why CORS is written here rather than borrowed

`tower-http::cors` is the obvious thing to depend on, and it does not fit. Its
configuration types — `AllowOrigin`, `AllowMethods`, `AllowHeaders`,
`ExposeHeaders`, `MaxAge` — are opaque: the inner enums are private and the
readers (`to_header`, `is_wildcard`) are `pub(crate)`. Kynos has to *read a
configuration back*, twice: to refuse a combination the protocol cannot honour
while the router is built, and to synthesize a preflight from the operations a
path declares. Neither is possible through those types, so depending on them
would buy the constructors and none of the behaviour. No other crate in the
hyper or tower stack carries CORS types on their own.

What is here therefore matches `tower-http`'s surface where the difference
would be a missing capability, and departs from it where the difference is the
point:

| `tower-http` | Kynos |
| --- | --- |
| `allow_origin(Any / exact / list / predicate)` | `allow_any_origin`, `allow_origins`, `allow_origins_matching` |
| `allow_origin(mirror_request)` | any permitted origin is echoed already, except under `allow_any_origin` alone |
| `allow_headers(Any / list / mirror_request)` | `allow_any_header`, `allow_headers`; the wildcard echoes what was asked under credentials |
| `expose_headers(Any / list)` | `expose_any_header`, `expose_headers` |
| `max_age(Duration)` | `max_age` |
| `allow_credentials(bool)` | `allow_credentials` |
| `allow_methods(Any / list / mirror_request)` | derived from the operations the covering scope declares; `allow_methods` overrides |
| `vary(list)` | derived; a declared header name is a `const`, so it is not a builder's to set |
| `allow_credentials(predicate)`, `max_age(dynamic)` | absent |
| `allow_private_network` | absent |
| `CorsLayer::permissive()`, `very_permissive()` | absent |

The last four rows are decisions rather than gaps.

`allow_methods` is derived because the alternative is a second place to state
what the path already declares, and two statements of one fact drift. `vary`
is derived because `HeaderParams::VARIES` is a `const` the collision check
reads while the program is compiled; a value a builder set at run time is not
one the compiler can check two interceptors against.

A credentials predicate and a dynamic `max-age` are per-request decisions that
change what a *shared cache* may reuse, and neither varies on a field a cache
keys on. `allow_private_network` sends
`Access-Control-Allow-Private-Network`, a header from a draft that has since
been renamed; adding it is the same squatting the rate-limit headers above
refuse, and it stays out for the same reason.

`permissive()` is a one-line constructor for the configuration a service should
arrive at deliberately. `Cors::new()` permits nothing and every widening is a
call a reviewer can see.

### The cost of owning the common layers

Owning the common layers has an ergonomic dividend beyond correctness: it
removes `tower-http` version-skew pain, which is a real and recurring tax on
applications in this ecosystem.

The price is bus factor — on the order of fifteen crates of ongoing maintenance.
The mitigation is to keep each one small enough that a single contributor can
hold it in their head and own it.
