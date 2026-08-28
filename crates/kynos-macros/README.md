# kynos-macros

Procedural macros for the [Kynos](https://github.com/getkono/kynos) REST API
framework.

Nothing here is meant to be depended on directly. Every macro is re-exported
from [`kynos`](https://crates.io/crates/kynos) behind its `macros` feature,
which is enabled by default, and each one is documented next to the trait it
implements. Depend on `kynos`.

The route attributes carry only what the types cannot — the method, the path and
prose. Parameters come from the handler's arguments and responses from its
return type, and neither is restated here.

## Documentation

- [API documentation](https://docs.rs/kynos-macros)
- [Design documentation](https://github.com/getkono/kynos/tree/master/docs)

## MSRV

Rust 1.85.

## License

MIT — see [LICENSE](LICENSE) for details.
