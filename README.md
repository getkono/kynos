# Kynos

> Status: Not for public use yet! Feel free to star. Also we have had enough AI-generated PRs in various repos but contributions with human-oversight would be welcomed.

Kynos is an idiomatic, performance-focused Rust framework for building REST APIs with first-class OpenAPI 3.1 and 3.2 support.

## Prerequisites

- [mise](https://mise.jdx.dev/) — tool and task manager

## Quick Start

```bash
mise install
mise exec -- hk install --mise
mise run check
```

## Development

| Command | Description |
| --- | --- |
| `mise run check` | Run formatting checks, linting, and tests |
| `mise run format` | Format the workspace |
| `mise run lint:fix` | Apply Clippy fixes |
| `mise run test` | Run tests |
| `mise run coverage` | Run tests with LLVM coverage |

## License

MIT — see [LICENSE](LICENSE) for details.
