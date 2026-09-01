# Errors

## The rule

Every error Kynos puts on the wire is an [RFC 9457] problem detail. There is no
second envelope and no per-endpoint shape, so a client can handle failures
generically instead of learning one format per operation.

This covers the framework's own failures, not only the application's. A body
that will not parse and a path parameter that will not deserialize both produce
a problem document, and both appear in the operation's `responses` — because
[`FromRequestParts::Rejection`](../crates/kynos/src/extract/mod.rs) is bound by
`Responses` and therefore cannot decline to describe itself.

It covers what middleware refuses, too, and the description owes the same
account of it. A `ShortCircuit` answers with a problem document, so the response
it declares names `application/problem+json` and the `Problem` component — one
writer, `error::problem::problem_response`, rather than a spelling per site.
Eight of the ten short circuits Kynos ships once described a response with no
content while sending one; the sweep in
[`tests/interceptors.rs`](../crates/kynos/tests/interceptors.rs) is what now
holds every implementation to it.

[RFC 9457]: ../references/rfc9457.txt

## A problem is a representation, not a response

[`Problem`](../crates/kynos/src/error/problem.rs) does **not** implement
`Responses`, so it cannot be returned from a handler and `Result<T, Problem>`
does not compile.

This is [anti-pattern 4](../README.md#anti-patterns) applied to errors, and the
reasoning is the same one that keeps `IntoResponse` off `StatusCode`. `Problem`
carries its status in a field, so returning one would choose that status at run
time and `Responses` would have nothing honest to say about which. The type that
reaches the wire and the type a handler names are deliberately different: the
first carries a status, the second *is* a set of them.

It keeps `IntoResponse`, because being *written* is exactly what it is for —
that implementation is how every derived error reaches the wire. The handler
bound is the pair, so removing one half is enough, and removing the half that
cannot be answered honestly is the one that says something true. `Problem` also
implements `Schema` and is registered as a named component, so a hundred error
responses share one `$ref` rather than repeating the object.

An error type gets to the wire through
[`IntoProblem`](../crates/kynos/src/error/problem.rs) instead:

| Method | Answers |
| --- | --- |
| `into_problem(self)` | what this occurrence looks like |
| `statuses()` | which statuses the type can ever produce, as a `const` |

`#[derive(ApiError)]` emits both from one declaration, together with the
`IntoResponse` and `Responses` implementations that bridge them. **The derive is
the only supported bridge.** A hand-written `IntoProblem` compiles but yields a
type no handler can return, because a blanket
`impl<E: IntoProblem> IntoResponse for E` would overlap every concrete
`IntoResponse` implementation and Rust has no way to prove the disjointness.

## Declaring an error type

```rust
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("no user with id {id}")]
    #[problem(status = 404, title = "User not found")]
    NotFound {
        #[problem(extension)]
        id: UserId,
        trace: String,
    },

    #[error("that email is already registered")]
    #[problem(status = 409)]
    EmailTaken,
}
```

| Position | Key | Meaning |
| --- | --- | --- |
| type | `base` | URI prefix; `type` defaults to it plus the kebab-cased variant name |
| variant | `status` | required, 400–599 |
| variant | `title` | the problem's `title`, and the response's description. Absent, the wire carries the status's reason phrase and the description falls back to the doc comment |
| variant | `type` | an absolute URI, overriding `base` |
| field | `extension` | serialize this field as an extension member under its own name |

**`title` is read twice, and its absence is answered differently each time.**
On the wire it is the problem's `title`, and a variant that declares none
carries `StatusCode::canonical_reason` — RFC 9457 section 4.2.1's own
recommendation for a problem whose type says nothing. In the description it is
the response's `description`, and there the variant's doc comment is tried
before the reason phrase, on the same argument `detail` rests on: the sentence a
Rust reader already wrote is the sentence an API consumer should receive.

There is no de-camel-casing of variant names anywhere, and this document used to
say there was.

`detail` comes from `Display`, which is why the derive requires it and why
`thiserror` is the expected companion: the `#[error("...")]` a Rust reader
already writes is the sentence an API consumer receives, rather than a second
one that can drift from it.

**A field is not serialized unless it says so.** `#[problem(extension)]` is
opt-in because a variant carries whatever the error site had to hand, and the
default must not be to publish it. `trace` above stays internal.

**What the grammar refuses.** A missing or out-of-range `status`, a member the
grammar does not define, a `base` on a variant, an `extension` on a field with
no name, and a type with no `Display` are all compile errors, each with a case
in [`tests/ui/macros`](../crates/kynos/tests/ui/macros). What `base`, `title`,
`type` and `extension` *do* is checked at run time by the conformance harness,
which compares each problem document a service produced against what the
description declared for that operation and status.

## Rejections

One type per extractor, each naming only the statuses it can actually produce.
All live in [`error::rejection`](../crates/kynos/src/error/rejection.rs).

| Type | Statuses | Raised by |
| --- | --- | --- |
| `PathRejection` | 400 | `Path<T>` |
| `QueryRejection` | 400 | `Query<T>`, `QueryString<T, M>` |
| `HeaderRejection` | 400 | `Headers<T>` |
| `CookieRejection` | 400 | `Cookies<T>` |
| `BodyRejection` | 400, 415, 422 | every body extractor, and `OneOf<L, R>` |
| `NegotiationRejection` | 400, 406 | `Accept<T>` |
| `RangeRejection` | 416 | `Range<T>::apply` |
| `AuthRejection` | 401, 403 | `Auth<S>`, `Scoped<S, R>` |

The split is the whole point. A single shared rejection type would be *sound* —
it satisfies `emitted ⊇ observable` — but every operation would advertise every
status any extractor can produce, so a handler reading one path parameter would
claim it might answer 401. A document that says everything says nothing, and a
401 on an endpoint with no authentication is not a harmless over-approximation:
it is the kind of claim a client generator turns into dead retry logic.

`BodyRejection` keeps 400 and 422 apart deliberately. A client can retry
neither, but only one of them means its serializer is wrong.

`RangeRejection` is the one raised by a *method* rather than by extraction.
[`Range<T>`](../crates/kynos/src/response/range/mod.rs) is infallible — RFC 9110
§14.2 answers every unusable `Range` field by ignoring it, so a bad field is a
200 and not a 400 — and the 416 arises only once the field meets a
representation it cannot be applied to, in `Range::apply`.

That is why the argument declares nothing but the parameter. A `Range<T>`
contributes no rejection, so the 416 reaches the document through the handler's
return type and is declared on exactly the operations that can produce one; a
handler that reads the field and answers whole, which §14.2 allows outright,
advertises no 416 at all. Declaring it from the argument instead would be the
"413 no operation could produce" shape [`testing.md`](testing.md) records.

It is also the second rejection whose response is more than a problem document:
§15.5.17 asks a 416 to state the representation's length in `Content-Range`, so
the rejection carries that length and writes the field itself, as
`AuthRejection` does for `WWW-Authenticate`. Unlike `AuthRejection` it also
*describes* that field, because the `unsatisfied-range` grammar is fixed and
there is no per-operation string for a `Describe` to supply — which is what lets
the header travel with the status wherever the status is declared from.

An extractor that cannot fail says so with `Infallible`, whose `Responses`
implementation contributes nothing:
[`Inject<T>`](../crates/kynos/src/di/inject.rs),
[`MatchedPath` and `ConnectInfo`](../crates/kynos/src/extract/connection.rs).
For the last two this is a fact about ordering — a route has already matched by
the time a handler argument is built.

## What is not an error type

**Interceptor statuses.** 429, 503 and 408 belong to
[`RateLimit`, `Concurrency` and `Timeout`](../crates/kynos/src/middleware/limits.rs),
which return a response directly and declare it through
an interceptor's `Short`. They are not extractor rejections and have no rejection
variant; an interceptor builds a `Problem` itself. See
[`middleware.md`](middleware.md#declaring-is-not-describing).

**[`kynos::Error`](../crates/kynos/src/error/mod.rs).** The framework's own
failure, raised while a router is built or a server started — never while
serving. It is what `Router::openapi`, `Router::build` and `Server::serve`
return, and it never reaches a client.

**`kynos::Result`.** Its defaulted `E = Error` is for those build-time paths.
A handler writes a plain `Result<T, E>`; the alias in that position would
suggest a relationship to the framework error type that does not exist.

## The build failure reports through the application

Kynos ships no error reporter: no `Debug` renderer, no `.context()`, no
backtrace. `main` is where a build failure is rendered, and that is the
application's to choose — `anyhow`, `eyre`, `color-eyre`, or a plain
`Box<dyn Error>`.

What Kynos owes them is one property, asserted in
[`tests/reporting.rs`](../crates/kynos/tests/reporting.rs):

> `Error` implements `std::error::Error + Send + Sync + 'static`.

That is exactly the bound `impl From<E> for eyre::Report` requires, so it is
what decides whether an application can `?` a `kynos::Result` out of `main` at
all. It is also what makes the cause chain worth keeping: those crates render a
failure by walking `source()` recursively, and `anyhow` supplies a backtrace
when the underlying error provides none — which Kynos cannot do on stable, since
`Error::provide` is unstable and `thiserror`'s `#[backtrace]` needs nightly.

**Policy: a `From` into `Error` is a promise that the source alone is the whole
story.** `TlsError` earns one because every variant names both what was being
configured and what was wrong with it. A bare `std::io::Error` does not, because
it cannot say what was being opened — which is why `Error::Io` was removed and
why the io failures the framework actually raises go through `ServerError::Bind`
and friends, each naming its address. `serde_json::Error` and
`serde_yaml_ng::Error` convert into separate variants rather than one, so the
conversion is what records which emitter failed.

**Policy: a cause is kept as a `source()`, not formatted into a `String`.** Two
exceptions, and both are stated where they apply. A value rendered in a *list*
must have a self-contained `Display`, so it carries no cause that would
duplicate it — `Error::Invalid` names every violation in its message because a
chain holds one error and a validation run produces a set, and `Violation` does
the same one level down. And `SpecError::MalformedAnnotation` keeps text because
its cause is a `serde_json::Error`, which is neither `Clone` nor `PartialEq`, so
holding it would cost the derives the property tests rely on.

Where a cause is kept but its type should not escape, it is boxed:
`TlsError` holds `Box<dyn Error + Send + Sync>` so a rustls failure stays
walkable without rustls's semantic version becoming Kynos's, or its name
appearing outside `server/tls/`.

## Where the union happens

`Handler::describe` contributes each argument's `Describe`, then each argument's
`Rejection` as a `Responses`, then the return type's `Responses`.
`Result<T, E>` unions the two sides on the way out, which is where a handler's
success and failure descriptions meet with no restatement anywhere.

The scoping reason this lives in `Handler::describe` rather than in `Describe`
is recorded in [`handlers.md`](handlers.md#where-the-rejection-union-happens).

## Localization

RFC 9457 asks for it directly. Section 3.1.3 says a problem's `title` "SHOULD
NOT change from occurrence to occurrence of the problem, **except for
localization** (e.g., using proactive content negotiation)", and section 4.2.1
says the same of the reason phrase an `about:blank` problem carries.

**Kynos negotiates the language and ships no catalogue.**
[`response::language`](../crates/kynos/src/response/language/mod.rs) chooses the
tag and writes `Content-Language`; the strings are the application's. That split
is [`architecture.md`](architecture.md)'s third invariant applied to text, and
the refusal it rests on is recorded there.

### It is an interceptor, and it has to be

`IntoResponse::into_response(self)` takes no context, and an extraction
rejection short-circuits to a response before any handler runs. **There is no
point in the pipeline where a request and a `Problem` coexist.** So localizing
an error is something done *to* a finished response, and an interceptor is the
only thing that holds one.

That is a design constraint rather than a preference, and it has a payoff: an
interceptor reaches every problem the service can produce, because a rejection's
response travels back up through `Next::run` like any other. One that translates
an application's own `#[derive(ApiError)]` translates Kynos's 400 for a
malformed path parameter by the same code, without naming it.
[`examples/localized_errors.rs`](../crates/kynos/examples/localized_errors.rs)
is that, in about sixty lines of public API.

The cost is a JSON round trip on the error path — the shape `Compression`
already has, on a path that is not hot.

### What a catalogue is keyed by

By the problem *type*, which is what section 3.1.3 makes `title` a property of.
The status belongs in the key too: a variant with no `#[problem(base = ...)]`
is `about:blank`, so every rejection the framework raises shares one URI and is
told apart only by its code. An error type that wants a localized title of its
own therefore needs a `base`, which is the one place that key earns its keep
beyond tidiness.

A type the catalogue has no entry for keeps the title it already carried, rather
than losing one. That is what makes adding a language additive.

### Two things the framework will not do

**It ships no translation of its own reason phrases.** `Problem::new` writes
`StatusCode::canonical_reason`, in English. Translating those is forty-odd
phrases in however many languages, and no CI job could hold them correct — the
same objection [`architecture.md`](architecture.md) makes to a media-type
database, except that a mistranslation cannot be caught by sampling *or* by a
property test. The downside is asymmetric as well: a wrong title in a language
a reader trusts misleads them, where an English one merely fails to help. An
application that wants them replaces them from its own catalogue.

**It does not localize `detail`.** That member comes from `Display`, so
translating an interpolated sentence means owning argument reordering, plural
categories and gendered agreement — a message-format model, which is `fluent` or
`icu` and a row the dependency table does not have. RFC 9457 section 3.1.4
points the other way regardless: consumers "SHOULD NOT parse the `detail` member
for information", and the channel that is meant to be read by a machine is
`type` together with the extension members. An error that needs a localized
sentence should carry what the sentence is *about* as extensions and let the
client render it.

## Rules

| # | Rule | Enforced by |
| --- | --- | --- |
| 4 | No status is chosen at run time | `IntoResponse` is unimplemented for `StatusCode`, `String`, `&str` and tuples of them; `Responses` is unimplemented for `Problem` |

Anti-pattern 4's other half — that a handler's several statuses come from
`#[derive(Reply)]` — is homed in [`handlers.md`](handlers.md#status-is-a-type).
This document owns the error side of the same rule.

## Rationale

*Non-normative.*

### Why `statuses()` is a `const` and not derived from `into_problem`

Reading the status set off the conversion would require running it, and a
description you can only obtain by executing the program is one you cannot check
in CI — the same argument
[`middleware.md`](middleware.md#declaring-is-not-describing) makes for contributions
being inert data. Declaring the set separately admits the failure mode where it
disagrees with the conversion, which is why the derive computes both from the
same attributes rather than trusting an author to keep two lists aligned.

### Why the derive is the only bridge

Making `IntoProblem` implementable by hand and useless on its own is an
unsatisfying pair. The alternatives were worse: a blanket implementation does
not compile, a `macro_rules!` bridge adds a second exported spelling for one
concept, and sealing `IntoProblem` would remove the ability to name the trait in
a bound or read what the derive produced. Leaving it public and inert keeps the
derive's output inspectable, which is the property that makes the generated code
reviewable.

### Why problem details rather than a Kynos envelope

There is an obvious pull toward a richer error type — one carrying a code enum,
a retry hint and a field-error list as first-class members. RFC 9457 already
allows all three as extension members, and choosing the standard means generated
clients, API gateways and error-reporting tools understand the shape without
configuration. The cost is that `extensions` is a `BTreeMap<String, Value>`
rather than something typed, which is a real loss at the edges and the reason
`#[problem(extension)]` exists: the map is an implementation detail of the wire
form, not the way an author is expected to write an error.

### Why `kynos::Error` is not `anyhow::Error` or `eyre::Report`

eyre is a fork of anyhow, and the two share the representation that makes them
attractive here: a one-word narrow pointer, a blanket `From`, and a `Debug` that
prints the whole chain.

Neither implements `std::error::Error`, and that is structural rather than an
omission — a blanket `impl<E: Error + Send + Sync + 'static> From<E> for Self`
collides with core's reflexive `impl<T> From<T> for T` the moment the type
implements `Error`. A design picks one or the other.

That decides it. `impl From<E> for eyre::Report` requires
`E: StdError + Send + Sync + 'static`, so re-exporting `anyhow::Error` as
`kynos::Error` would stop an application wrapping its own `main` in eyre — the
case the type exists to serve. A newtype could implement `Error`, but hits the
same wall for its own blanket `From`, so every conversion is hand-written
anyway, while pattern matching is lost.

[`architecture.md`](architecture.md#dependencies) closes the question
independently: *"a crate absent from it is not a candidate, and 'can we add X?'
is answered by naming the row X displaces"*. The only `Errors` row is
`thiserror`, ambient because it *"is a derive that reaches no public
signature"*. An anyhow-shaped crate is the opposite — its type **is** the public
signature — so it displaces nothing.

The conclusion is not that Kynos should build its own reporter. It is that Kynos
should be a well-behaved `std::error::Error` and let the application's reporter
do the rendering, which is less code and a better result than either.
