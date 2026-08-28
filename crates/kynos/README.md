# kynos

An idiomatic, performance-focused Rust framework for building REST APIs with
first-class OpenAPI 3.1 and 3.2 support.

Kynos only lets you build APIs it can fully describe. Every handler input
describes itself as a Parameter or Request Body, every handler output describes
itself as a Responses Object, and every interceptor declares what it
contributes. Anything undescribable does not compile.

The emitted document is therefore not documentation that drifts from the code.
It is a checked contract derived from the same types the server runs on.

## What it costs you

Several things other Rust frameworks offer are absent: arbitrary `tower`
middleware, raw request access, wildcard and catch-all routes, runtime-chosen
status codes, `serde_json::Value` bodies, erased state maps. Most are refused
because they would put a claim in the description that the running service does
not honour; a few are argued on other grounds, and one is advice rather than a
rule. The `unchecked` feature provides named escape hatches for three of them,
at the price of a description that is no longer authoritative for the route that
took one. All eleven, each with its own reasoning, are in the
[repository README](https://github.com/getkono/kynos#anti-patterns); WebSockets
and OpenAPI 3.0 support are refused outright and are listed just above it.

## Feature flags

`openapi31` is the baseline and is enabled by default; `openapi32` is an opt-in
strict superset. The rest gate the codecs, scalar formats, TLS, compression, the
response cache, static assets, the API reference page and the test client. The
full table is in the
[repository README](https://github.com/getkono/kynos#feature-flags), and the API
documentation on [docs.rs](https://docs.rs/kynos) is built with all of them
enabled.

## Documentation

- [API documentation](https://docs.rs/kynos)
- [Design documentation](https://github.com/getkono/kynos/tree/master/docs)
- [Examples](https://github.com/getkono/kynos/tree/master/crates/kynos/examples)

## MSRV

Rust 1.85.

## License

MIT — see [LICENSE](LICENSE) for details.
