# Kynos

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

## Code Guidelines

- OpenAPI compliance: The official OpenAPI specs in Markdown is vendored in `references/`. Strictly distinguish features behind `openapi31` (default enabled and baseline) and `openapi32` feature flags.
- Idiomatic and strict Rust API: API is pre-v1. All changes are on the table. API must be idiomatic Rust, and structurally strict to leverage compiler hints.
- Production-ready: Our contract guarantees made it easy to make it production-grade since day-one. Not future optimizations should not break API (hence we are not v0.y.z).
- Be opinionated where it counts: We scope features that are strictly required for performance and where there should only be one recommended approach. For example, IO-related primitives is vertically integrated and coupled with core dependencies like `tokio` and dependency injection is fully-featured. However, we would not prescribe dependencies such as ORMs and logging backends.
- Document all public API surface.

## Development Guidelines

- Pushing code: Atomic, semantic commits are strictly required. All PRs are *usually* merged rather than being squashed/rebased.
- New features: Code implementation must be code-complete. Feature flag(s) must be modified when justified. When feasible, new features must be additive.
- Bug fixes: Correctness is strict. The offending code must be testable so refactor adjacent code to expose the internals for unit testing if required. Push strictly in order: red tests targetting failure case and asserting the correct invariants; implementation that addresses the red case with evidence the tests turned green.

## Workspace

- The root is a Cargo and mise monorepo.
- The initial library crate is `crates/kynos`.
- Declare shared dependency versions under `[workspace.dependencies]`.
- Add a dependency to a member crate with `workspace = true` only when the crate consumes it.
- Do not introduce public framework APIs as placeholders.

## Tooling

Use mise as the task interface:

```bash
mise run test          # correctness
mise run format:check  # formatting
mise run lint          # Clippy with warnings denied
mise run coverage      # LLVM coverage
```

Run `mise run check` before handing off changes.

Commits must follow Conventional Commits (i.e. semantic commits). Convco enforces this through git hooks and CI; merge commits are exempt.
