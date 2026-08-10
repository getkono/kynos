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
| variant | `title` | defaults to the variant name, de-camel-cased |
| variant | `type` | an absolute URI, overriding `base` |
| field | `extension` | serialize this field as an extension member under its own name |

`detail` comes from `Display`, which is why the derive requires it and why
`thiserror` is the expected companion: the `#[error("...")]` a Rust reader
already writes is the sentence an API consumer receives, rather than a second
one that can drift from it.

**A field is not serialized unless it says so.** `#[problem(extension)]` is
opt-in because a variant carries whatever the error site had to hand, and the
default must not be to publish it. `trace` above stays internal.

**What is built.** The grammar is parsed and enforced: a missing or
out-of-range `status`, a member the grammar does not define, a `base` on a
variant, an `extension` on a field with no name, and a type with no `Display`
are all compile errors, each with a case in
[`tests/ui/macros`](../crates/kynos/tests/ui/macros). `statuses()` is real.
What `base`, `title`, `type` and `extension` *do* is designed rather than built,
because `into_problem` and `Problem`'s builders are still `todo!()`; the
members are checked for shape and position so the grammar cannot quietly change
meaning once those land.

## Rejections

One type per extractor, each naming only the statuses it can actually produce.
All live in [`error::rejection`](../crates/kynos/src/error/rejection.rs).

| Type | Statuses | Raised by |
| --- | --- | --- |
| `PathRejection` | 400 | `Path<T>` |
| `QueryRejection` | 400 | `Query<T>`, `QueryString<T, M>` |
| `HeaderRejection` | 400 | `Headers<T>` |
| `CookieRejection` | 400 | `Cookies<T>` |
| `BodyRejection` | 400, 413, 415, 422 | every body extractor, and `OneOf<L, R>` |
| `NegotiationRejection` | 400, 406 | `Accept<T>` |
| `AuthRejection` | 401, 403 | `Auth<S>`, `Scoped<S, R>` |

The split is the whole point. A single shared rejection type would be *sound* —
it satisfies `emitted ⊇ observable` — but every operation would advertise every
status any extractor can produce, so a handler reading one path parameter would
claim it might answer 401. A document that says everything says nothing, and a
401 on an endpoint with no authentication is not a harmless over-approximation:
it is the kind of claim a client generator turns into dead retry logic.

`BodyRejection` keeps 400 and 422 apart deliberately. A client can retry
neither, but only one of them means its serializer is wrong.

An extractor that cannot fail says so with `Infallible`, whose `Responses`
implementation contributes nothing:
[`Inject<T>`](../crates/kynos/src/di/inject.rs),
[`MatchedPath` and `ConnectInfo`](../crates/kynos/src/extract/connection.rs).
For the last two this is a fact about ordering — a route has already matched by
the time a handler argument is built.

## What is not an error type

**Interceptor statuses.** 429, 503 and 504 belong to
[`RateLimit`, `Concurrency` and `Timeout`](../crates/kynos/src/middleware/limits.rs),
which return a response directly and declare it through
`OperationContribution`. They are not extractor rejections and have no rejection
variant; an interceptor builds a `Problem` itself. See
[`middleware.md`](middleware.md#operationcontribution).

**[`kynos::Error`](../crates/kynos/src/error/mod.rs).** The framework's own
failure, raised while a router is built or a server started — never while
serving. It is what `Router::openapi`, `Router::build` and `Server::serve`
return, and it never reaches a client.

**`kynos::Result`.** Its defaulted `E = Error` is for those build-time paths.
A handler writes a plain `Result<T, E>`; the alias in that position would
suggest a relationship to the framework error type that does not exist.

## Where the union happens

`Handler::describe` contributes each argument's `Describe`, then each argument's
`Rejection` as a `Responses`, then the return type's `Responses`.
`Result<T, E>` unions the two sides on the way out, which is where a handler's
success and failure descriptions meet with no restatement anywhere.

The scoping reason this lives in `Handler::describe` rather than in `Describe`
is recorded in [`handlers.md`](handlers.md#where-the-rejection-union-happens).

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
[`middleware.md`](middleware.md#operationcontribution) makes for contributions
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
