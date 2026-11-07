# Migrating

What each release makes unnecessary downstream. An upgrade fixes what stops
compiling, because the compiler points at it; the
[changelog](../crates/kynos/CHANGELOG.md) lists those breaks. Nothing points at
a workaround that still compiles, so it survives the upgrade beside the API
that replaced it. This guide lists those.

Each row reads: if you wrote the left column, write the middle one instead.
The [issue](https://github.com/getkono/kynos/issues) in the right column
carries the motivating example. Paths are
relative to `kynos`, and an item `kynos::prelude` exports is named bare.

This is a guide, not a contract: nothing here is normative.

## Keeping it current

A feature request's optional **Replaces** field names the downstream code the
feature makes unnecessary. When that issue ships, its answer becomes a row in
the section for the release that ships it.

## 0.5 to 0.6

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| A coverage record of your own, with its own template matcher, for responses no `test::TestClient` received | `test::Coverage::record` | #1152 |
| A loopback socket, a hand-written request head and a status-line parser, to test what a configured server's timeouts answer | `test::TestClient::through_server`, which sends each request over `server::Server::serve_io` | #1152 |

## 0.4 to 0.5

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| Presenting, authenticating and authorizing a credential by hand, then patching the 403's challenge | `security::requirement::ScopeCheckedBy::check_scoped`, which authorizes the scheme's default scopes and the demanded ones in one call | #1000 |
| `extract::body::binary::Binary<M>` to carry a text body under a `text/*` media type | `extract::body::text_as::TextAs<M>`, which checks the body is UTF-8 | #987 |
| A hand-built hyper accept loop around your own transport, buffering each body before routing | `server::Server::serve_io`, which serves any `AsyncRead + AsyncWrite` connection, streams bodies and speaks HTTP/2 | #580 |

## 0.3 to 0.4

| If you wrote | Write instead | Issue |
| --- | --- | --- |
| Patching `externalDocs` into the document after the router emits it | `Router::external_docs` | #472 |
| A mount prefix of your own joined into a `Location` with `format!` | the `extract::link::Link<E>` extractor: `Created::at(link.uri(params), body)` renders the target with every `Group` and `nest` prefix applied | #477 |
| The same literal in every `#[problem(base = "https://…")]` | a shared `const`: `#[problem(base = crate::PROBLEM_BASE)]` | #458 |
| `FromStr`, `Display` and `schema::ParamValue` written by hand for a field-less enum | `#[derive(kynos::ParamValue)]`, reading and writing the names serde does | #464 |
| A macro re-declaring shared failure variants in every `ApiError` enum | one variant, `#[problem(transparent)] Shared(#[from] SharedError)` | #459 |
| `WithHeaders<E, H>` with an `Option` header that only one failure sets | `#[problem(headers)]` on that failure's one header-group field | #460 |
| A random request-id source of your own | `middleware::request_id::Random`, given to `middleware::request_id::RequestId::source` | #476 |
| An interceptor sending `X-Content-Type-Options: nosniff` and framing denial | `middleware::security_headers::SecurityHeaders`, with `.deny_framing()` | #475 |
| A hand-built schema for a base64 string field | `#[schema(content_encoding = "base64", content_media_type = "…")]` on the field | #463 |
| Response coverage and template matching merged across several test clients by hand | one `test::Coverage`, shared through `test::TestClient::with_coverage(&coverage)` | #479 |
| A raw TCP harness to send a stalled, partial or oversized body | `test::TestRequest::body_stream` with `content_length` | #479 |
| Matching `server::error::ServerError::ShutdownTimeout` to tell a requested shutdown from a failure | `server::error::ServerError::is_requested_shutdown` | #478 |
