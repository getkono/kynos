<!--
Title: a Conventional Commit header, e.g. `fix(router): ...`.
See CONTRIBUTING.md for what each box below means.
-->

## What and why

Closes #

<!-- The problem, the approach, and any decision a reviewer should argue with. -->

## Checklist

- [ ] A human wrote or read every line of this change and can explain it in review.
- [ ] `mise run check` passes locally.
- [ ] Every commit is an atomic Conventional Commit; a breaking change is a `!` commit.
- [ ] A bug fix lands as a failing test commit before the fix commit.
- [ ] A changed feature flag or `#[cfg]` was run through `mise run features:check`.
- [ ] Public items are documented, and any rule in `docs/` this changes is updated here.

## Not covered

<!-- What this does not test or does not do, or "nothing". -->
