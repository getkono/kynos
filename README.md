# Kynos

> Status: Not for public use yet! Feel free to star. Also we have had enough AI-generated PRs in various repos but contributions with human-oversight would be welcomed.

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

## Development

- Prerequisites: rustup, [mise](https://mise.jdx.dev/)

- Install dependencies:
```bash
mise install
mise exec -- hk install --mise
mise run check
```

*See <mise.toml> for scripts.*

## FAQ

**Is kynos monolithic?**
No, it is minimal but strict and opinionated where correctness is in question.

**Is kynos really faster than the competition?**
A better way to frame it is we are as fast if not faster in most ways (open an issue if not) and spend effort on optimizations possible due to our API strictness since day-one.
We plan to release our benchmarks in this repo: <https://github.com/getkono/kynos-bench>
<!-- TODO: establish this claim. -->

**Why code-first and not contract-first?**
There may be more than one answer but most consumers already expect contracts for REST APIs (usually in the form of OpenAPI specs). Building and extending code that define OpenAPI specs has less friction and enables performance optimizations not possible with an OpenAPI spec that constrains code.

**How is kynos related to spargen?**
[spargen](https://github.com/getkono/spargen) is a related project by the same people but completely separate internals. kynos is for the server and spargen is for the client.

## MSRV

We conservatively bump as new language features come about.

Rust 1.85

## License

MIT — see [LICENSE](LICENSE) for details.
