# Kynos

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

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

## Rust

- Document every public item.
- Keep APIs idiomatic and avoid premature abstraction.
- Use `thiserror` for library error types when errors are introduced.
- Enable only the Tokio features required by the consuming crate.
- Keep unsafe code out of the framework unless a measured need and documented safety argument justify it.

## Commits

Commits must follow [Conventional Commits](https://www.conventionalcommits.org/). Convco enforces this through git hooks and CI; merge commits are exempt.
