# Handlers

## The rule

Every handler argument implements
[`Describe`](../crates/kynos/src/extract/describe.rs). There is no second kind
of argument and no exemption.

An argument that contributes nothing to the contract says so, by implementing
`Describe` with an empty body:

| Argument | Describes | Because |
| --- | --- | --- |
| [`Inject<T>`](../crates/kynos/src/di/inject.rs) | nothing | application state has no wire form |
| [`MatchedPath`](../crates/kynos/src/extract/connection.rs) | nothing | it is the `paths` key, already in the document |
| [`ConnectInfo`](../crates/kynos/src/extract/connection.rs) | nothing | it is a property of the connection, not the API |

An empty body is a **claim**, not a skipped step: it asserts that a consumer
cannot observe this argument. That distinction is the entire difference from
tools that infer a description from axum handlers, where an extractor need not
describe itself at all and a document therefore acquires silent holes.

The consequence worth stating plainly: **there is no extractor that yields the
whole request.** No `Request`, no `Body`, no `HeaderMap`. Those are the holes.

## `Handler<C, A>`

[`Handler`](../crates/kynos/src/handler/mod.rs) is implemented for `async fn`s
of up to sixteen arguments. `C` is the application context;
`A` is `(Marker, T1, .., Tn)`.

| Slot | Bound | Notes |
| --- | --- | --- |
| Marker | [`ViaRequest`] or [`ViaParts`] | never written by hand; `()` when the handler takes no arguments |
| `T1..Tn-1` | `FromRequestParts<C> + Describe` | read the request head |
| `Tn` | `FromRequest<C> + Describe`, or `FromRequestParts<C> + Describe` | the last argument alone may consume the body |
| return | `IntoResponse + Responses` | one says what goes on the wire, the other what the document claims |

[`ViaRequest`]: ../crates/kynos/src/handler/mod.rs
[`ViaParts`]: ../crates/kynos/src/handler/mod.rs

The marker carries no information a caller supplies — it is inferred at every
call site — and exists only so the two implementations per arity are disjoint.
A function of `n` arguments matches both the body-consuming and the head-only
shape, and coherence has no other way to see the difference. Arities 0 through
16 are emitted by
[`handler/impls.rs`](../crates/kynos/src/handler/impls.rs), which is private
precisely so the arity macro does not leak.

The bounds are the whole enforcement mechanism. An argument that cannot
implement `Describe` has no way into a signature, and a return type that cannot
implement `Responses` has no way out.

**A consequence of the marker scheme:** a type implementing both `FromRequest`
and `FromRequestParts` makes `A` ambiguous at the call site, because the handler
would satisfy both implementations for the same arity. No Kynos type does. A
downstream type that implemented both would be unusable as a last argument until
one of the two implementations was removed.

## Where the rejection union happens

`Handler::describe` contributes, in order:

1. each argument's `Describe`;
2. each argument's `Rejection`, as a `Responses`;
3. the return type's `Responses`.

**The rejection half lives in `Handler::describe`, not in `Describe`.** The
reason is a scoping fact rather than a preference: `Rejection` is an associated
type of `FromRequestParts<C>`, chosen per context type, and `Describe::describe`
is a static method that cannot name `C`. `Handler::describe` is the only place
where the argument type and the context type are both in scope, so it is the
only place the union can be taken at all.

That is what makes *emitted ⊇ observable* — the invariant
[`middleware.md`](middleware.md#soundness-not-exactness) states — mechanical
rather than a convention every extractor author has to remember. An extractor
cannot forget to document its own failures, because it was never asked to.

`Result<T, E>` unions the two sides on the way out, which is where a handler's
success and failure descriptions come together with no restatement anywhere.

## Status is a type

There is no way to choose a status at run time.

| Type | Status | Notes |
| --- | --- | --- |
| [`NoContent`](../crates/kynos/src/response/status.rs) | 204 | |
| [`Created<T>`](../crates/kynos/src/response/status.rs) | 201 | `location` is a required field, not an option |
| [`Accepted<T>`](../crates/kynos/src/response/status.rs) | 202 | |
| [`Redirect<CODE>`](../crates/kynos/src/response/status.rs) | `CODE` | 301, 302, 303, 307 or 308 only |
| [`WithHeaders<T, H>`](../crates/kynos/src/response/headers.rs) | `T`'s | `H` implements `HeaderParams`, usually by deriving it, so `Response.headers` is complete |

`Created` and `Redirect` both carry a
[`Location`](../crates/kynos/src/response/status.rs), which exists because a
route attribute's `relative_uri` returns an `http::Uri` and neither that type
nor `String` is Kynos's to write a conversion between. Naming the concept is
what lets a typed URI and a string literal both arrive without a call-site
conversion. It validates nothing: a `Location` field value is a URI reference,
and relative forms are legal.

`Redirect<CODE>` is bounded on `(): ValidRedirectCode<CODE>`, a witness
implemented for exactly those five codes. Both the trait and `()` are foreign to
a downstream crate, so the set cannot be widened from outside, and
`Redirect::<304>` fails to compile. That rules out the common redirect bug of
writing 302 where 307 was meant and silently changing the method on replay.

An operation with several statuses returns an enum deriving
[`Reply`](../crates/kynos-macros/src/derive/reply.rs), one variant per status.
The derive rejects a struct, with a diagnostic pointing at `Created`, `Accepted`
and `NoContent`, because the point of `Reply` is a *closed set* and a struct has
one shape.

Each variant declares its own status, `#[reply(status = N)]`, between 200 and
599 — a 1xx is an interim response and a handler returns the final one. Two
variants may not share a status, which is where `Reply` is stricter than
[`ApiError`](errors.md): a problem carries a `detail` telling two occurrences of
one status apart, and a reply's variants are keyed by status alone. A variant's
fields are its response body, so it holds exactly one described type, or none
for the empty body.

## Negotiation, on both sides

| Direction | Selected by | Type | Distinctness proved by |
| --- | --- | --- | --- |
| request | `Content-Type` | [`OneOf<L, R>`](../crates/kynos/src/extract/body/mod.rs) | [`Alternative<Rhs>`](../crates/kynos/src/extract/body/alternative.rs) |
| response | `Accept` | [`Negotiated<T>`](../crates/kynos/src/response/negotiate/mod.rs) | `Representations`, sealed |
| response | `Accept-Language` | [`Localized<T, L>`](../crates/kynos/src/response/language/mod.rs) | `Languages`, open |

`Alternative` is not a blanket trait. It is implemented only for pairs of body
wrappers whose media types are known to be distinct, so `OneOf<Json<A>,
Json<B>>` fails to compile instead of making dispatch order observable — no
description can express "whichever the router tried first". The implementations
are enumerated per codec pair and each is `#[cfg]`-gated at item level, because
a cross-codec pair needs both features and no single gate covers a group.

The third row is the counter-case to the second, and the difference is the
specification's rather than a preference.
[`AcceptLanguage<L>`](../crates/kynos/src/response/language/mod.rs) *does*
contribute a parameter: OpenAPI names exactly three fields whose definition
shall be ignored — `Accept`, `Content-Type`, `Authorization` — and this is not
one of them. Nor is its trait sealed. The offerable representations are exactly
the codecs Kynos can describe, so an outside implementation would be one the
`content` map could not state; a catalogue is the opposite, and only the
application knows it.

What the two share is that the offer is a type, so the description cannot miss
an arm. `Localized` has no public constructor, which is what makes the emitted
`Content-Language` enumeration true: the only tag that reaches the wire is one
the negotiation chose from `Languages::TAGS`. Neither adds a status — a client
whose language is missing is served the default with `Content-Language` saying
so, never a 406.

[`Accept<T>`](../crates/kynos/src/response/negotiate/mod.rs) contributes **no**
`Accept` parameter, because the specification says a parameter definition for
that field shall be ignored. What describes the negotiation is the operation's
`content` map, contributed by the representation tuple; `Accept`'s own
`Describe` contributes only rejection responses, the 406 among them. The `Representation` and `Representations` traits are public and sealed by a
private supertrait: the set of offerable representations is exactly the set of
codecs Kynos can describe, and the bound is still nameable by a program generic
over what it offers.

## Rules

The normative home for four of the [anti-patterns](../README.md#anti-patterns).
Each is a rule with a mechanical enforcement point.

| # | Rule | Enforced by |
| --- | --- | --- |
| 2 | No handler receives the raw request, its body, or its whole header map | `Describe` has no blanket implementation, and Kynos ships none for `Request`, `Body` or `HeaderMap` |
| 4 | No status is chosen at run time | `IntoResponse` is unimplemented for `StatusCode`, `String`, `&str` and tuples of them; `Responses` is unimplemented for `Problem`, whose status is a field. See [`errors.md`](errors.md#a-problem-is-a-representation-not-a-response) |
| 5 | `Accept`, `Content-Type` and `Authorization` are never header parameters | `#[derive(HeaderParams)]` rejects them by folded name, and names the right tool for each |
| 6 | No unconstrained body type | `serde_json::Value` and friends have no [`Schema`](../crates/kynos/src/schema/mod.rs) implementation |

**#2.** A handler that wants an arbitrary header declares it with
`Headers<T>`; a handler whose body genuinely is arbitrary says
`Unchecked<T>`, which annotates the schema and makes `Router::validate` warn.
Weakness is allowed; silent weakness is not.

**#4.** Status belongs to the return type. `#[derive(Reply)]` covers an
operation with several.

**#5.** The three field names are reserved because a parameter definition for
them *shall be ignored*, so declaring one is a claim no consumer will honour.
The remedies the derive names are: content negotiation for `Accept`, the
content map for `Content-Type`, and `#[derive(SecurityScheme)]` for
`Authorization` — so that enforcing a credential and describing it are one act.

**#6.** The rule also removes `usize` and `isize` (their width depends on the
build target, and a wire contract must not), `SystemTime` (serde emits a
seconds/nanos struct nobody wants as a contract), and `Box<dyn Trait>`. The
accepted and rejected sets, and the `format` each describable type carries, are
normative in [`schema.md`](schema.md).

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why sixteen arguments

The arity ceiling is a macro expansion cost, not a design statement. Sixteen is
where the cost of two implementations per arity stops being free and where no
real handler has been observed. A handler approaching it is usually one that
should have grouped its parameters into a derived type, which is cheaper for the
description too: sixteen loose query parameters and one `Query<Filters>` emit
different documents, and the second is the one a client generator handles well.

### Why the marker is a type rather than a const

`ViaRequest` and `ViaParts` are uninhabited enums, so they cost nothing at run
time and cannot be constructed by accident. A const generic would have worked
equally well for disjointness and would have appeared in every diagnostic as a
bare integer. An error mentioning `ViaParts` at least names what it means: this
handler reads the head only.

### The empty `Describe` body as a design device

The alternative to `impl Describe for Inject<T> {}` is exempting some arguments
from the trait entirely — which is what a second argument category would be.
That trade looks like tidiness and is actually a hole: an exemption is invisible
at the call site, whereas an empty body is a line of code someone had to write
and a reviewer can see. The same reasoning removed `FromContext`; see
[`state.md`](state.md#why-fromcontext-was-deleted).
