# kynos-openapi

The OpenAPI 3.1 and 3.2 document model, serialization and validation.

This crate is the data model [Kynos](https://github.com/getkono/kynos) emits
into, and is deliberately free of any runtime dependency: no `tokio`, no
`hyper`. It is usable on its own to build, serialize or validate an OpenAPI
description.

`openapi31` is the baseline and is enabled by default. `openapi32` adds the
fields introduced by OpenAPI 3.2.0 as a strict superset. Those fields are
`#[cfg]`-gated rather than runtime-optional, so a build without `openapi32`
cannot construct a document it would be unable to emit; the model types they
extend are `#[non_exhaustive]`, so a downstream `match` compiles under either
build.

Three further features are off by default:

- `yaml`: the `emit::yaml` module, holding `Document::to_yaml` beside the
  JSON emission every build has.
- `http`: conversions between `Method` and `http::Method`.
- `pattern`: an ECMA-262 `pattern` translated into the `regex` crate's dialect.

Both versions use the same JSON Schema dialect,
`https://spec.openapis.org/oas/3.1/dialect/base`. OpenAPI 3.0 and older are
permanently out of scope.

## Documentation

- [API documentation](https://docs.rs/kynos-openapi)
- [Design documentation](https://github.com/getkono/kynos/tree/master/docs)

## MSRV

Rust 1.85.

## License

MIT — see [LICENSE](LICENSE) for details.
