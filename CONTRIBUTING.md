# Contributing

Contributions are welcome when a human stands behind them. Kynos has had enough
generated pull requests that nobody read; a change you cannot explain line by
line in review is not ready to open. Using tools to write it is fine, handing
review to them is not.

Vulnerabilities go to a [security advisory](.github/SECURITY.md); usage
questions, design arguments and everything else go to an issue first.
Participation is under the [code of conduct](CODE_OF_CONDUCT.md).

## Setup

Prerequisites: [rustup](https://rustup.rs/) and [mise](https://mise.jdx.dev/).
mise pins the toolchain and every tool the gates use.

```bash
mise install
mise exec -- hk install --mise   # commit-msg, pre-commit and pre-push hooks
mise run check                   # the gates a local change owes
```

CI runs those too, runs `coverage:ci` in place of `test`, and adds
`msrv:check`, `publish:check` and `commits:check`.

`mise tasks` lists the rest. The ones worth knowing by name:

| Task | Run it when |
| --- | --- |
| `mise run check` | before handing off any change |
| `mise run test` | iterating on behaviour |
| `mise run format` | before every commit; `format:check` is the gate |
| `mise run lint` | Clippy over every feature, warnings denied |
| `mise run features:check` | a feature flag or a `#[cfg]` changed; nothing else catches a broken combination |
| `mise run docs:check` | a public item changed; `missing_docs` is denied |
| `mise run containment:check` | a dependency or module moved; it enforces where each crate may be named |

## What binds a change

[`docs/`](docs/README.md) is normative: where a document there states a rule,
the change follows it or changes the document in the same pull request, with
the argument. [`AGENTS.md`](AGENTS.md) holds the code and workspace rules.
The README's [anti-patterns](README.md#anti-patterns) are refusals, not gaps;
a pull request implementing one will be closed.

[`docs/testing.md`](docs/testing.md) decides which kind of test a guarantee
owes. Tests are hermetic: nextest runs each in its own process, so never rely on
shared state or ordering, and never mask a flake with a retry.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/), checked by convco
in the `commit-msg` hook and in CI. Merge commits are exempt.

- Atomic: each commit builds, passes the gates, and does one thing. The one
  exception is a bug fix's red-test commit below, which fails by design.
- A breaking change is a `!` commit, and the changelog lists it. That applies
  to rows the README marks `frozen` too, while the API is 0.x.
- Pull requests are usually merged rather than squashed, so the series is what
  lands. Fix up history before review, not after.

## Bug fixes are red first

1. A commit adding a test that fails on the bug and asserts the correct
   behaviour. If the offending code cannot be tested, refactor adjacent code to
   expose it first.
2. A commit fixing it, after which that test passes.

Keep that order in the pushed series, so a reviewer can check out the first
commit and watch it fail.

## Features

Code-complete or not at all: no placeholder APIs, no `todo!()`. Prefer additive
changes, gate them behind a feature where one is justified, settle every
ambiguity the issue left open in the pull request, and add a minimal example to
[`crates/kynos/examples/`](crates/kynos/examples/README.md) for a new group of
features.

## Pull requests

The [template](.github/PULL_REQUEST_TEMPLATE.md) asks for what review needs.
