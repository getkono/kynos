# Routing

## Path templates

[`PathTemplate`](../crates/kynos-openapi/src/model/paths/template.rs) is the
only parser for a path in the workspace. The route attributes call it rather
than reimplementing it, because two notions of "valid path template" that could
disagree is exactly the drift this framework exists to prevent.

What it enforces:

| Rule | Rejected as |
| --- | --- |
| The template begins with `/` | `MissingLeadingSlash` |
| No `?` and no `#` anywhere | `NotAPath` |
| Outside `{}`, only `pchar` and `/` | `IllegalLiteralCharacter` |
| A `%` introduces two hex digits | `MalformedPercentEncoding` |
| Braces balance; `{` inside a name is unbalanced | `UnbalancedBraces` |
| `{}` holds a name | `EmptyExpression` |
| No variable name repeats | `DuplicateVariable` |
| No segment is empty | `EmptySegment` |

`pchar` is RFC 3986 §3.3: ASCII alphanumerics, `-._~`, the sub-delimiters
`!$&'()*+,;=`, `:`, `@`, and percent-encoded triples. Every other character —
including every non-ASCII character — has to arrive percent-encoded. `/` is
permitted in a literal run because it is the segment separator rather than a
`pchar`, and segmentation is checked separately.

`?` and `#` get their own error rather than being reported as illegal
characters, because a template carrying one is more likely a whole URL pasted in
than a stray byte, and the diagnostic should say so.

Two properties are easy to get backwards:

**A trailing `/` is legal.** The grammar is
`path-template = "/" *( path-segment "/" ) [ path-segment ]`, so the final
segment is optional. `/users` and `/users/` are therefore *different paths* —
which is what makes trailing-slash handling an application-level policy rather
than a parse question.

**Variable names are barely checked at all.** Anything except a brace is
accepted, including a `/`. That is not laxity: the OpenAPI grammar admits it, and
`PathTemplate` is the document model's type, so it must be able to hold a
template that arrived in an externally authored description. Kynos's own
narrower contract is enforced elsewhere — see the next section.

[`normalized`](../crates/kynos-openapi/src/model/paths/template.rs) replaces
every name with `{}`. Two templates are the same path if and only if their
normalized forms are equal, which is what makes "two paths differing only in
variable name" a checkable violation rather than an opinion. `with_prefix`
concatenates, trimming a trailing `/` off the prefix, and re-parses — so a
prefix that collides with one of the template's variables is caught by
`DuplicateVariable` rather than by a separate rule.

## The routing contract is narrower than the model

Kynos does not route catch-alls. A path parameter value must not contain an
unescaped `/`, so `/assets/{*path}` has no OpenAPI equivalent: no path template
is true of it, and every `paths` key that could be minted would be a claim about
either the path or a parameter that the service does not honour.

That check does **not** belong inside `PathTemplate`, and its absence there is
deliberate. `{*path}` is a legal OpenAPI variable name, and the model has to
round-trip an externally authored description without rewriting it. Rejecting
the name at parse time would make the model lossy in order to enforce a routing
rule the model does not have.

The enforcement point is `Router::mount` and the composition methods that feed
it: a template whose variable name begins with `*` is refused there, before
`matchit` is asked to insert it, and the router reports it rather than serving
it. The same check refuses a segment carrying two variables — `/{a}-{b}` is a
legal OpenAPI template that `matchit` cannot take apart, so it too is a path the
router declines rather than one the model rejects.
[`architecture.md`](architecture.md#what-does-not-move-and-why) records the same
boundary from the dependency's side.

The escape hatch is
[`route_unchecked`](../crates/kynos/src/unchecked.rs), behind the `unchecked`
feature. It serves the route and gets no `paths` entry, recording it under
`OPAQUE_ROUTES_ANNOTATION` and stamping the document non-authoritative instead.

## Router, Group, and document scope

| Construct | Code scope | Document scope |
| --- | --- | --- |
| [`Router::mount`](../crates/kynos/src/router/mod.rs) | operations at the root | those operations' `paths` entries |
| [`Group`](../crates/kynos/src/router/group.rs) | one prefix, one tag, shared interceptors | prefix joins each path; the tag lands on each operation; each contribution merges into each operation |
| [`Router::nest`](../crates/kynos/src/router/mod.rs) | another router under a prefix | the prefix joins every path beneath |
| [`Router::merge`](../crates/kynos/src/router/mod.rs) | another router at the same level | the two operation sets union |
| [`Router::intercept`](../crates/kynos/src/router/mod.rs) | every operation in the router | every one of their descriptions |
| [`Router::observe`](../crates/kynos/src/router/mod.rs) | every operation in the router | nothing — observers change nothing |
| [`Router::docs`](../crates/kynos/src/router/docs/) | two operations, at the paths the `Docs` names | two `paths` entries, rendered from the very document that describes them |

A route attribute also emits `relative_uri`, taking exactly the path and query
types that route extracts — so a link that no longer matches its target is a
compile error. It is *relative* because the attribute knows only its own path
template: a `Group` or `nest` prefix is applied here, while the router is built,
which is after the expansion and out of its reach. A route mounted at the router
root needs no join; one under a prefix does.

A tag is applied at four scopes, and they add rather than override:
`Router::tag`, `Group::tag`, `EndpointBuilder::tag`, and `tag = T` on the route
attribute itself. Only the last is a fact about the operation rather than about
what encloses it, which is why it is the only one readable without building a
router — it becomes
[`EndpointMeta::TAGS`](../crates/kynos/src/router/endpoint/meta.rs). The
attribute names one tag; the enclosing scopes are how an operation acquires the
rest.

Scope in the document matches scope in the router, exactly. A group is the
recommended unit of API structure — one per resource — because attaching
authentication to a group documents it on every operation underneath, correctly,
without anyone maintaining that by hand.

What reaches a router is an
[`Endpoints<C>`](../crates/kynos/src/router/endpoint/), produced by
`routes![..]`, or anything implementing `IntoEndpoints<C>`: an
[`EndpointBuilder`](../crates/kynos/src/router/endpoint/), or a tuple, array
or vector of those. There is deliberately no blanket `IntoEndpoints` over
`Endpoint`, because it would conflict with every container implementation —
a downstream crate may implement `Endpoint` for a tuple of its own types, and
coherence has to assume it will. A hand-written endpoint is mounted with one
line, `sink.push(self)`.

`Endpoints` is opaque and append-only. The prefix, the panic policy and the
interceptors belong to whatever is mounting, not to the endpoints, so there is
nothing there for a caller to reach into.

### The description a router serves is the one it emits

`Router::docs` is the only mount whose *payload* is the document. The bytes
cannot exist before `build`, because the two operations it registers are in the
document they serve — and the page's pointer cannot be settled earlier either,
since a `nest` prefix is applied while the outer router is built, after the
`Docs` value was constructed. Both halves are therefore rendered at `build`,
which is what makes the served description byte-identical to `openapi`'s and the
page's link true of the path the route actually got.

It follows rule 11 rather than bending it: what is served is `openapi`'s own
output, not a document patched on the way past.

## `validate`, `openapi`, `build`

| Method | Consumes | Returns | Fails when |
| --- | --- | --- | --- |
| `validate(&self)` | nothing | every `Violation`, warnings included | the router cannot be described at all |
| `openapi(&self)` | nothing | the `Document`, at the lowest version expressing this API without loss | any violation is error-level |
| `openapi_as(&self, v)` | nothing | the `Document` at version `v` | as above, or the API uses a construct `v` cannot express |
| `build(self, context)` | the router | a servable `Service<C>` | any violation is error-level |

`validate` is the one to put in an integration test: it catches the mistakes
that only appear across a whole API — a duplicated `operationId`, two paths that
differ only in variable name, a security requirement naming a scheme nobody
declared. `build` runs the same structural checks, so an API that cannot be
described correctly fails at startup rather than at documentation time.

### Which version `openapi` emits

The lowest one that expresses the API without loss: 3.1 for an API using no
3.2-only construct, and 3.2 for one that uses a `QUERY` operation, a streamed
response or an `in: querystring` parameter. Lowest rather than highest, because
a description a consumer's toolchain can read is worth more than one
advertising a version number, and nothing is lost by saying 3.1 when 3.1 is
enough.

**Not the `openapi32` feature.** Cargo unifies features across a dependency
graph, so which version a document claims cannot follow a flag any crate in the
build might turn on — a program whose own API never changed would otherwise
start emitting `openapi: 3.2.0` because something unrelated enabled it. The
feature gates which constructs a program can *write*; `openapi` reads which it
actually *used*.

`openapi_as` targets a version and never degrades to one. A 3.2-only construct
requested as 3.1 is a hard error listing what blocks it: a Server-Sent Events
response has no `itemSchema` in 3.1 to describe it with, and Kynos would rather
not emit at all than emit a description with the stream quietly missing. Reach
for it when a consumer pins a version, and let `openapi` decide otherwise.

## Typed URIs

A route attribute emits an inherent `uri` constructor beside the endpoint type
it generates ([`route/uri.rs`](../crates/kynos-macros/src/route/uri.rs)). Its
signature is derived from the handler's own extractors:

| Handler has | `uri` signature |
| --- | --- |
| neither | `uri() -> Uri` |
| `Path<P>` | `uri(path: P) -> Uri` |
| `Query<Q>` | `uri(query: Q) -> Uri` |
| both | `uri(path: P, query: Q) -> Uri` |

The values are percent-encoded on the way in, so a path parameter holding a `/`
survives as `%2F` rather than silently becoming two segments —
[`tests/typed_uri.rs`](../crates/kynos/tests/typed_uri.rs) asserts exactly that.

Three mismatches are compile errors rather than runtime 404s: a handler with
`Path<T>` on a route with no variables, a route with variables and no `Path<T>`,
and a `PathParams` field set that does not match the template's variables in
declaration order — the last through a `const` assertion the attribute emits
against `PathParams::NAMES`. Extracting `Path<T>` or `Query<T>` twice in one
handler is also rejected, since neither the URI nor the description could say
which one won.

`operationId` defaults to the handler's function name and is overridden with
`operation_id = "..."` on the attribute. Override it only to keep a generated
client's method name stable across a refactor.

## Application-level policies

[`FallbackPolicy`](../crates/kynos/src/router/policy.rs) and
[`TrailingSlashPolicy`](../crates/kynos/src/router/policy.rs) are set once, on
the router, and have no per-route form.

| Policy | Applies to | Values |
| --- | --- | --- |
| `not_found` | no route matched | `Problem` (default), `Empty` |
| `method_not_allowed` | path matched, method did not | `Problem` (default), `Empty` |
| `trailing_slashes` | a request differing only by a final `/` | `Strict` (default), `Redirect`, `Lenient` |

None of them adds a `paths` entry. An unmatched path, a wrong method and a
trailing-slash variant are all outside the description, and settling them once
at the application level is what keeps the paths *in* the description exact.

`Redirect` and `Lenient` both accept a second spelling of a path and differ in
whether the client is told. `Redirect` answers 308, so a client that follows it
learns the declared form. `Lenient` serves both spellings from the operation the
declared one names and never mentions the difference — reach for it where the
extra round trip is not available, such as a client that will not replay a
non-idempotent method, or a directory-style path genuinely reachable both ways.

Neither adds or removes anything but the final slash. Neither changes casing,
and neither normalizes an individual route.

`Lenient` registers the flipped spelling in the match table and nowhere else.
The consequences are all of one piece: `paths` still carries exactly the key
that was declared, `MatchedPath` still reports the declared template rather than
becoming one label per spelling, `Allow` is still computed from the declared
operations, and the synthesized `OPTIONS` still covers both. A route declared
both ways keeps both entries — the declared spellings are registered first, and
a flipped spelling that collides with one is discarded.

The `Allow` header on a 405 is derived from the operations actually declared on
that path, so it cannot disagree with the document.

## The `matchit` contract

`matchit` is the router, declared by `crates/kynos` and named only under
[`router/`](../crates/kynos/src/router/). What routing depends on it for:

- A `{param}` matches exactly one path segment and never crosses a `/`. That is
  the same rule OpenAPI's path templating has, which is why the two can share a
  syntax without a translation step.
- A capture is a borrowed slice of the request path, not an owned `String`.
  This is the property that makes the zero-allocation requirement in
  [`nfr.md`](nfr.md#routing) reachable at all.
- Catch-all patterns are understood by `matchit` and rejected by Kynos above it,
  which is why the anti-pattern and the crate can coexist.

**The allocation caveat.** The zero-allocation requirement is scoped rather than
absolute, and the scope comes from the router's actual guarantees. `matchit`
captures parameters into a fixed inline buffer of three and spills to the heap
beyond it; backtracking out of a static segment into a parameter sibling
allocates once as well. Both are reachable through ordinary REST shapes, so the
requirement is written against routes of at most three parameters with no
static/dynamic sibling overlap. Widening it later is a measurement, not a
rewrite.

## Rules

The normative home for four of the [anti-patterns](../README.md#anti-patterns).

| # | Rule | Enforced by |
| --- | --- | --- |
| 3 | No wildcard or catch-all routes | `Router::mount`, above `PathTemplate`. [`router::assets`](../crates/kynos/src/router/assets/) is how static files are served *without* one: a fixed set is enumerable, so every path is a literal |
| 9 | No header-based API versioning | nothing mechanical |
| 10 | One application-level slash policy, or none | `TrailingSlashPolicy` has no per-route form |
| 11 | The emitted document is never hand-patched | `Router::openapi` returns an owned `Document`; nothing exposes the built service's document mutably |

**#9 has no enforcement point, and that is worth stating rather than hiding.**
OpenAPI expresses paths; a version carried in a header is invisible to `paths`,
so two versions of an operation collapse into one entry that describes neither
faithfully. Nothing in the type system can see a convention a service applies to
a header it never declared — and Kynos will not let that header be declared
either ([anti-pattern #5](handlers.md#rules)). Put the version in the path.

**#11.** `Router::openapi()` is the only supported path from code to
description. If it cannot express something, that is a bug worth reporting
rather than routing around: a document patched after emission is a document that
no longer follows from the types the server runs on, which is the one property
the whole design buys.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why the model is more permissive than the router

It would be simpler to reject `{*path}` in `PathTemplate::parse` and have one
rule in one place. The cost is that `kynos-openapi` stops being able to hold a
document it did not produce — and consuming an external specification in order
to *verify* code against it is a stated goal, not a hypothetical one
([`architecture.md`](architecture.md#invariants), invariant 3).

The general principle: the model's job is to be true of OpenAPI, and the
router's job is to be true of what Kynos can serve. Where those differ, the
narrower rule goes in the narrower layer. A parser that enforces a consumer's
policy is a parser that cannot be reused by a second consumer.

### Why fallbacks are not routes

Modelling a 404 handler as a route would put an entry in `paths` for a path that
does not exist, or a wildcard entry that claims every path exists. Both are
worse than the honest absence, and the honest absence costs a client nothing: a
consumer that meets an undocumented 404 has met the HTTP default, not a
surprise. The same reasoning is why `FallbackPolicy` chooses only the *shape* of
the body — an RFC 9457 problem document or nothing — and not its status.
