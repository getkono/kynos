# Pending UI cases

Cases that were scoped for this suite and did not land, with the blocker that
stopped each one. A case is only worth adding once its blocker is gone, because
`compile_fail` passes for any reason at all — a negative that cannot be paired
with a control asserts nothing.

## README anti-patterns

Covered: 1, 2, 4, 5, 6, 7, 10, 11. Pending: 3, 8, 9.

| # | Anti-pattern | Blocker |
| --- | --- | --- |
| 3 | Wildcard and catch-all routes | `PathTemplate::parse` accepts `{*path}` on purpose: a variable name is unconstrained in OpenAPI, and an externally authored description holding one has to round-trip. The narrower rule is enforced where routes are registered, which is a run-time check in a `todo!()`-bodied body rather than a type-level one. Landing this needs the router's registration path. |
| 8 | Request-derived values as dependencies | `Inject<CurrentUser>` typechecks whenever the context provides a `CurrentUser`, and nothing in the type system distinguishes a value read from the request from application state. The rule is a review convention today. |
| 9 | Header-based API versioning | The README says so itself: a version header declared with `#[derive(HeaderParams)]` compiles, and no mechanism rejects it. Advice rather than a rule. |

Anti-pattern 1 landed as the part of it that a single build can check: a
`tower::Layer` is not accepted where an `Interceptor` is required. That
`layer_unchecked` itself is absent without the `unchecked` feature is *not*
checked here — the negative needs the feature off, and this suite's snapshots
are recorded with it on. See below.

## Resolved since this ledger was written

`#[kynos::operation]` was listed here as having no possible control, because no
program using it compiled: it read `method` itself and then handed its whole
argument list to a parser that rejected `method` as unknown. That was a defect
rather than a limit of the suite. Both cases have landed
(`macros/operation_missing_method.rs`, `pass/operation_with_method.rs`), along
with `macros/route_method_argument.rs`, which pins the other half — that a
per-method attribute must keep rejecting `method`.

## Blocked on the suite's one feature set

Five snapshots in `antipattern/` embed rustc's "the following other types
implement" list, which enumerates implementations that feature flags add and
remove. That pins every snapshot in the suite to one feature set, and
`--all-features` is the set `mise run test` uses. A case that can only fail with
a feature *off* therefore has nowhere to live: `[[test]]` targets cannot select
feature sets, only require them.

| Case | Asserts |
| --- | --- |
| `Group::layer_unchecked` absent | the escape hatch for README anti-pattern 1 is behind `unchecked` |
| `#[kynos::operation(method = "PROPFIND")]` rejected | a method outside the eight OpenAPI 3.1 names needs `openapi32` for `additionalOperations` |
