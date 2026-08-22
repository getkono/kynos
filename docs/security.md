# Security

## The rule

**Requiring a credential and declaring it are one act.**

There is no way to guard an operation without describing the guard, and no way
to describe one without enforcing it. `Auth<S>` is the only door, and taking one
as a handler argument adds the scheme to `security`, registers it under
`components.securitySchemes`, adds 401 and 403 to `responses`, and declares the
`WWW-Authenticate` the 401 carries. Four things, one argument, no way to do a
subset.

## Policy

### A scheme says where its credential travels, once

`#[derive(SecurityScheme)]` writes two implementations from one attribute.
`SecurityScheme::describe` says where a client should put the credential;
`Carries::present` is where Kynos reads it from. They are emitted from the same
`in`, the same `name`, the same kind, so a description advertising `X-Api-Key`
cannot sit next to a verifier reading `X-API-Token`.

This is why [`Authenticator::authenticate`](../crates/kynos/src/security/mod.rs)
receives `S::Presented` and not a `&Parts`. An authenticator is not given the
request, so it *cannot* reach for a field the scheme did not declare. The
constraint is the feature: every framework that configures a credential finder
beside its documentation has two statements that agree until someone edits one.

### The three states of a presented credential

`Carries::present` returns `Result<Option<T>, AuthRejection>`, and all three
mean different things:

| Answer | Meaning | `Auth<S>` | `MaybeAuth<S>` |
| --- | --- | --- | --- |
| `Ok(None)` | no credential was presented | 401 | anonymous |
| `Err` | one was presented and is not a credential of this scheme | 401 | **401** |
| `Ok(Some)` | one was presented | the authenticator decides | the authenticator decides |

The second row is the one worth stating. A request carrying a malformed token,
or a credential for a *different* scheme, is not an anonymous request. Treating
it as one would let `MaybeAuth` wave through exactly the traffic worth refusing,
so a carrier refuses rather than reporting absence when the request presented
something it cannot read.

### Optional authentication is a description

`MaybeAuth<S>` emits `security: [{}, {S: []}]` — the empty requirement first,
which is OpenAPI's spelling for "anonymous access is also permitted". A reader
of the description learns that the credential is *honoured* rather than
*demanded*.

That is a different thing from a middleware configured not to reject, which
appears in no document at all.

### What Kynos does not verify

No JWT verifier, no session store, no password hasher. Each is application
policy: the algorithm, the claim set, the key rotation, the cost parameters. A
framework that chose them would be wrong for most callers and unremovable for
the rest. [`examples/jwt.rs`](../crates/kynos/examples/jwt.rs) shows the shape
with `jsonwebtoken` as a dev-dependency, named nowhere under `src/`.

What Kynos *does* ship is the part that is the same for everyone:

| Supplied | Because |
| --- | --- |
| The carrier for every scheme kind | It follows from the description, and hand-rolling it is where the bugs are |
| RFC 7617 basic decoding | base64, the *first* colon, and UTF-8 — a fixed wire form |
| [`constant_time_eq`](../crates/kynos/src/security/mod.rs) | `==` on a shared secret says how much of a guess was right |
| The `WWW-Authenticate` challenge | It is part of what the 401 *is*, and it has to match the description |
| 401 and 403 on the operation | A guard that did not declare them would make the document wrong |

### Two carriers are refused, deliberately

RFC 6750 defines three ways to present a bearer token. Kynos reads one.

- **Section 2.2, the form-encoded body.** Not reachable from a request *head*,
  and making a credential a body field would put it in the operation's schema.
- **Section 2.3, the URI query parameter.** `SHOULD NOT` in the RFC itself. A
  token in a query string reaches access logs, `Referer` headers and browser
  history.

Neither is a gap to be closed later. An *API key* in a query parameter is a
different matter and is supported, because that is what `api_key(in = "query")`
describes and the specification permits it.

### Where mutual TLS fits

A client certificate is presented during the handshake, so no request field
reveals it — which is exactly why the scheme has to be declared. It reaches an
authenticator as `PeerCertificates`, read back from the
[`Connection`](../crates/kynos/src/extract/connection.rs) the server recorded.

`peer_certificates` is **not** gated on the `tls` feature, and that is a
decision rather than an oversight. The feature would key on the wrong thing: a
service behind a TLS-terminating proxy sees no certificates with the feature on.
What answers "can this deployment authenticate a client certificate" is the
deployment, so the reader reports none in every case that produces none.

### A scheme may be published without being required

`Router::security_scheme::<S>()` registers a scheme that no operation yet
demands. That is what lets a description advertise a credential before the
operations requiring it exist.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### Why `Presented` is owned

A borrowing form — `type Presented<'r>` — would keep the common path
allocation-free, and it was tried. It puts a lifetime parameter into every
application's `impl Authenticator`, which
[`architecture.md`](architecture.md#public-api-surface) rules out for the public
surface: generics that exist for performance stay private. RPITIT would not
unify the elided form with `async fn` in an implementation either, so the
ergonomic cost was not even buying compilation.

The price is one allocation per authenticated request, against a verifier that
is about to check a signature or read a session store.

### Why base64 is not a dependency

`base64` would be a fine dependency and is not taken, for the reason
[`architecture.md`](architecture.md#dependencies) gives: a new dependency
arrives feature-gated and additive. Basic authentication is in the default
build, so it could not have been gated. What is here is decode-only, refuses
non-canonical encodings, and has RFC 4648's own vectors plus an independently
written encoder as its oracle.

`subtle` is declined the same way. It reaches the tree today only under `tls`,
and making it direct would put it in every build for fifteen lines.

### Comparison with Salvo's `jwt-auth`

Salvo couples JWT as a framework feature. Kynos does not, and the difference is
not only that one crate is absent from the graph.

| Salvo | Kynos | Where the difference bites |
| --- | --- | --- |
| A `Finder` is configured on the hoop | `Carries::present` is emitted from the `#[security(...)]` attribute | The finder and the document are one text, so they cannot disagree |
| The hoop emits no security requirement; `#[endpoint(security(..))]` is written separately | `Auth<S>` adds the requirement, the scheme, 401, 403 and the challenge in one act | Guarded-and-undocumented and documented-and-unguarded are both unrepresentable |
| `depot.jwt_auth_state()` returns `Authorized`/`Unauthorized`/`Forbidden`; the handler must match | Guarding is the argument's type | There is no state to forget to read |
| `force_passed(false)` lets an unauthenticated request through, invisibly | `MaybeAuth<S>` emits `[{}, {S: []}]` | "Optional" is a fact in the description rather than a flag in the wiring |
| Scopes are strings compared in handler code | `Scoped<S, R>` writes `R::SCOPES` into the requirement *and* the check | A misspelled scope is a compile error |
| One `JwtAuth` per router; the verifier is ambient | `Authenticates<S>` is per scheme on the context | A router guarding `S` cannot be built on a context that cannot verify `S` |
| `jsonwebtoken` is in every user's dependency graph | Named by one example and nothing under `src/` | JWT policy stays the application's |
| Basic auth is a separate feature with its own validator | One `Carries` seam covers basic, bearer, arbitrary HTTP schemes, API keys and mTLS | One concept, five carriers |
| Peer certificates are not surfaced | `Carries for MutualTls` yields `PeerCertificates` | mTLS is an authenticable scheme rather than a transport detail |
| The verifier reads the whole request | `authenticate` receives `S::Presented` only | An authenticator cannot read what the scheme did not declare |

The row that matters most is the first. Everything else follows from it: once
the carrier and the description are the same text, there is no drift left for
the other rows to describe.
