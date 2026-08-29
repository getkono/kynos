# Kynos

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

## Code Guidelines

- OpenAPI compliance: The official OpenAPI specs in Markdown is vendored in `references/`. Strictly distinguish features behind `openapi31` (default enabled and baseline) and `openapi32` feature flags.
- RFC9110 compliance: See `references/rfc9110.txt` when HTTP semantic correctness is involved.
- Idiomatic and strict Rust API: API is pre-v1. All changes are on the table. API must be idiomatic Rust, and structurally strict to leverage compiler hints.
- Production-ready: Our contract guarantees made it easy to make it production-grade since day-one. Not future optimizations should not break API (hence we are not v0.y.z).
- Be opinionated where it counts: We scope features that are strictly required for performance and where there should only be one recommended approach. For example, IO-related primitives is vertically integrated and coupled with core dependencies like `tokio` and dependency injection is fully-featured. However, we would not prescribe dependencies such as ORMs and logging backends.
- Runtime: tokio-only and never abstracted over; direct tokio use stays inside `crates/kynos/src/server/`. See `docs/architecture.md`.
- Document all public API surface idiomatically and just tersely for internal logic.

## Development Guidelines

- Pushing code: Atomic, semantic commits are strictly required. All PRs are *usually* merged rather than being squashed/rebased.
- New features: Code implementation must be code-complete. Feature flag(s) must be modified when justified. When feasible, new features must be additive. Requirements are often missing context about the repository so all features must identify and finalize all technical ambiguity. Assert all decisions and standards-compliant behavior via the testing method `docs/testing.md` allocates to that kind of code, and write nothing that section lists as redundant. When appropriate, each new group of feature should be demonstrated in some minimal example(s) in the appropriate crate's examples directory.
- Bug fixes: Correctness is strict. The offending code must be testable so refactor adjacent code to expose the internals for unit testing if required. Push strictly in order: red tests targetting failure case and asserting the correct invariants; implementation that addresses the red case with evidence the tests turned green.
- Framework documentation is intentionally curated and minimal. The API should be mostly self-documenting so documentation serve to fill in the gaps such as design decisions and highlighting the important concepts and design patterns for beginners.
- Tests: hermetic by construction. Nextest isolates each test in its own process, so never rely on shared state or test ordering, and never mask a flake with retries.
- Documentation: All documentation must be placed in `docs/` which intentionally tersely formatted in Markdown. It complements and provides high-level context complementing the API docs published with code.

## Workspace

- The root is a Cargo and mise monorepo.
- The crates are `kynos-openapi` (the OpenAPI document model, runtime-free), `kynos-macros` (procedural macros) and `kynos` (the framework facade, which re-exports both). Application code depends only on `kynos`.
- Declare shared dependency versions under `[workspace.dependencies]`.
- Add a dependency to a member crate with `workspace = true` only when the crate consumes it.
- A module becomes a directory once it holds two independently-changing concerns; tests move to a sibling `tests.rs`. Passing ~400 lines excluding tests is when to ask that question, not an answer to it: a module holding one concern stays a file, because splitting it would only lengthen the paths of the items it declares. `containment:check` counts the ones that stay.
- Submodules are `pub` with no parent re-exports, so every item has one canonical path; the crate root and `kynos::prelude` are the only curated shortcuts, and macro-support items live in `kynos::__private`. A module that declares no item of its own — only trait implementations — is private instead, since it has nothing for a path to point at.
- A feature gate belongs on the `pub mod` line, not repeated on each item inside it.
- Do not introduce public framework APIs as placeholders, with one exception: the pre-v1 API-skeleton milestone, during which the surface is designed ahead of its implementation so it can be reviewed and frozen as a whole. A placeholder body must be `todo!()`, must be fully documented, and must appear in a `no_run` doc example proving the surface is usable. Once the skeleton is frozen this exception lapses.
- A proc macro is exempt from the `no_run` example rule only while its expansion cannot compile in its own crate; a derive must expand to a well-formed implementation with `todo!()` bodies rather than `todo!()`-ing during expansion, since an expansion that aborts can appear in no test at all.

## Tooling

Use mise as the task interface:

```bash
mise run test          # correctness
mise run format:check  # formatting
mise run lint          # Clippy with warnings denied
mise run coverage      # LLVM coverage
mise run features:check # every reachable feature combination
mise run docs:check    # rustdoc with warnings denied
mise run msrv:check    # builds on the declared MSRV
mise run containment:check # each crate is named only where it is allowed
mise run publish:check # every crate packages and rebuilds from its tarball
```

A framework this feature-gated breaks silently without `features:check`; run it
whenever a feature flag or a `#[cfg]` changes.

Run `mise run check` before handing off changes.

Commits must follow Conventional Commits (i.e. semantic commits). Convco enforces this through git hooks and CI; merge commits are exempt.
