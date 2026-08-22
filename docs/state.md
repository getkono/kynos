# State

Kynos calls application state the *context*, and the word is not a synonym for
container. There is no container.

## Policy

**The context is a type, not a container.** The application's own struct *is*
the context. It is handed to [`Router::build`](../crates/kynos/src/router/mod.rs)
once, a handler's requirements are bounds on it, and resolution is a trait
selection the compiler performs. Nothing is registered, looked up or erased, so
there is no lookup left that could fail while a request is being served.

[`Provides<T>`](../crates/kynos/src/di/mod.rs) is the capability. A handler
names what it needs by taking [`Inject<T>`](../crates/kynos/src/di/inject.rs),
which is an ordinary `FromRequestParts<C>` bounded on `C: Provides<T> + Sync`.
The reflexive `impl<T: Clone> Provides<T> for T` means a one-dependency
application needs no wrapper at all: `Router::<Arc<Pool>>::new()` satisfies
`Inject<Arc<Pool>>` with no derive and no struct.
[`#[derive(Provider)]`](../crates/kynos-macros/src/derive/provider.rs) emits one
implementation per field, so the many-dependency case is the same mechanism with
more implementations rather than a second mechanism.

Two fields of the same type are rejected by the derive rather than left to
coherence, because a handler asking for that type could not say which field it
meant. `#[provide(skip)]` opts a field out.

**`Provides<T>` is synchronous and infallible.** The reason is not simplicity.
[`Describe::describe`](../crates/kynos/src/extract/describe.rs) is a static
method that cannot name the context type, so it can never see — and therefore
never document — a provider's error. A fallible provider would produce
responses no operation declares, which is the single invariant the framework
exists to hold. Blocking is refused for the same reason: a provider that awaits
a checkout can time out, and a timeout is a response.

Acquisition that can fail is therefore not injection. Inject the *handle* and
perform the acquisition in the handler body, where its failure lands in the
return type and so in the description:

| Acquisition | Injected | Performed | Failure appears in |
| --- | --- | --- | --- |
| Database transaction | the pool | handler body | the handler's return type |
| Outbound HTTP call | the client | handler body | the handler's return type |
| Queue publish | the sender | handler body | the handler's return type |

`Provides::provide` runs once per injected argument per request, so an
implementation is expected to be a clone of a handle and nothing more.

## Invariants

**1. A handler compiles if and only if its context provides every `Inject<T>`
it names.** There is no fallback, no default and no partial resolution.

**2. The mount site is where that becomes a compile error.** `routes![..]`
leaves the context type inferred; `Router::<App>::new().mount(routes![..])` is
where `C` first becomes concrete, so it is where a handler asking for something
`App` does not provide fails to typecheck. `Router::build(context)` supplies a
*value* of the type whose *capabilities* were already proven. Nothing new can
fail there. [`tests/pipeline.rs`](../crates/kynos/tests/pipeline.rs) asserts the
positive half of this end to end.

**3. A request-derived value is never a dependency.** A `CurrentUser` read from
an `Authorization` header is not application state. It reaches a handler through
[`Auth<S>`](../crates/kynos/src/security/auth.rs), so that requiring the
credential, adding the scheme to the operation's `security`, and adding 401 and
403 to its `responses` are one act. Injecting it instead would make the
requirement invisible in the description — [anti-pattern
#8](../README.md#anti-patterns).

**4. Injection contributes nothing to the description, and says so.**
`Inject<T>` implements `Describe` with an empty body and declares
`Rejection = Infallible`. See [`handlers.md`](handlers.md) for why an empty body
is a claim rather than an omission.

## Scope

Every provider is a singleton for the life of the process. One context exists;
`provide` hands out a value from it per request.

Per-request memoization — one transaction shared by two injected repositories —
is deliberately absent rather than pending. The common case needs nothing from
Kynos: inject the pool, open the transaction where it is used.

> **Not yet implemented, and not scheduled.** The paragraph below records the
> additive path so the decision is not re-litigated, not a plan.

A first-class version costs no signature change: a `ProvidesScoped<T>`
capability plus a new extractor, with the memo living in the request's own
extensions, which `FromRequestParts::from_request_parts` already hands every
extractor mutably. A miss in that memo is a cold cache rather than a missing
dependency, so it still cannot panic, and the compile-time guarantee still comes
from the bound on the context.

## Locality

*Normative about what the signatures commit to, not about anything the runtime
does today. Kynos serves every request from one context on the runtime's own
worker pool; nothing is pinned, and no core owns anything.*

The reason to settle this with the surface rather than after it is that one of
the three properties below cannot be added later.

**1. The context is borrowed, never cloned.** `Handler::call` takes `&C`, and so
does every `FromRequestParts` and `FromRequest`. Nothing in the request path
asks for `C: Clone`, and nothing holds the context at an address it assumes is
the only one. A `&C` is a `&C` whether it points at one context or at one of
sixty-four.

**2. An injected value is owned, and that is where the borrow stops.**
`Provides<T>::provide(&self) -> T` hands out a value and cannot hand out a
`&T`, because `FromRequestParts::from_request_parts` returns `Self` with no
lifetime tying it to `context`. This is the one thing the freeze forecloses:
lending state to a handler means a lifetime parameter on `FromRequestParts` and
`FromRequest`, and therefore on every extractor and all thirty-three `Handler`
implementations.

It is closed deliberately. What it costs is one clone of a handle per injected
argument per request — for the shape this document already prescribes, an `Arc`,
one atomic increment. What lending would buy back is that increment, and the
increment is only worth anything because the refcount sits on a line shared
between cores. A per-core context removes it by construction: each core's `C`
owns its own handles, the increment becomes core-local, and the win that
borrowing was for arrives without borrowing.

**3. Nothing in a built service is a singleton by type.** `Router::build`
takes the context by value and returns a `Service<C>` that owns it. Two
services built from two contexts are two independent values: there is no
registry, no `static` and no `OnceLock` anywhere in the request path. "One
context per process" describes how a `Server` is assembled today, not something
the types impose.

### What a per-core design would add

Recorded so the decision is not re-litigated. All of it is additive:

- `build` consumes the `Router`, so a router cannot be built twice. A per-core
  server needs either `Router: Clone` or a second constructor —
  `build_per_core(self, impl Fn(usize) -> C)` — returning one service per core.
- `Server` holds one `Arc<Service<C>>` and spawns every connection onto the
  ambient runtime. Per-core serving means one runtime per core, each with its
  own listener and its own service.

Neither touches `Handler`, `Provides`, `Inject`, or any handler signature. That
is the property worth having: a handler written today compiles unchanged
against a per-core server, and an application that never wants one pays nothing
for the possibility.

What it would not add is a way for two cores to share mutable state without
saying so. `Provides<T>` takes `&self` and the context is reached through a
shared reference, so anything mutable in it is already something the
application chose to make interior-mutable — and whether that is sharded is the
application's decision rather than the framework's.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### How the ecosystem resolves state

| Framework | Mechanism | Resolved | Failure mode |
| --- | --- | --- | --- |
| axum | `State<S>` projected by `FromRef` | compile time | compile error |
| axum | `Extension<T>`, a `TypeId`-keyed map | run time | 500, per request |
| actix-web | `Data<T>`, a `TypeId`-keyed map | run time | 500, per request |
| poem | `Data<&T>` from request extensions | run time | 500, per request |
| salvo | `Depot`, a string-keyed `Any` map | run time | `Result` the handler unwraps |
| Kynos | `Provides<T>` on the context type | compile time | compile error |

The first row and the last are the same shape: a capability trait the state type
implements, selected by the compiler. axum spells it `FromRef`; Kynos spells it
`Provides`. That Kynos has only that shape — no second, erased path beside it —
is the whole of the difference, and it is a subtraction rather than an
invention.

The middle four rows share one failure mode: a dependency that was never wired
becomes a runtime error on the first request that needs it, which is to say in
production if that route is not covered by a test. salvo's is worse only in that
the key is a string, so a typo cannot be caught by any tool at all.

The rows describe the mechanism's shape, which is stable, rather than any
particular version's exact status code.

### Why `FromContext` was deleted

`FromContext` existed to keep request-derived arguments and state-derived
arguments in separate categories. It was removed because the separation was
enforced by nothing — a type could implement both traits — and because it is not
the property that matters.

The property that matters is that **every** argument implements `Describe`,
with an empty body being a claim of contract-neutrality rather than an
omission. Once that holds, a second trait sorting arguments into kinds buys
nothing: an argument that describes nothing has said so, and an argument that
cannot describe itself has no way into a signature regardless of which category
it would have claimed.

So `Inject<T>` became an ordinary `FromRequestParts` with
`Rejection = Infallible`, alongside `MatchedPath` and `ConnectInfo`, which
already worked that way. One kind of argument, one trait to check, no category
whose boundary nothing defends.

The same commit removed `ContextBuilder`. Its `build(self) -> C` had no bound
on `C` and no relation between the values put in and the value that came out,
so the only way to implement it was a `TypeId`-keyed map of `Box<dyn Any>` —
the erased state map [anti-pattern #7](../README.md#anti-patterns) rejects,
reintroduced behind the type that was supposed to replace it.
