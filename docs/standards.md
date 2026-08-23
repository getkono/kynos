# Standards

Which specification governs each middleware, and where every departure from one
is argued.

This document does not restate a specification — [`references/`](../references/README.md)
holds the texts and [`middleware.md`](middleware.md) holds the reasoning. What
lives here is the mapping between them, so that "which RFC is this written
against" has an answer that is one lookup rather than a search, and so that a
departure cannot be introduced without a line appearing below.

It is a claim about conformance, which is not the same claim as
[`nfr.md`](nfr.md)'s. A row can name a governing section here and still owe a
test there.

## Policy

**Every middleware names its governing standard.** A module implementing a wire
format that some specification defines cites the document and the section, in
its own doc comment and in the table below.

**Every departure is argued before it ships, not after.** A deviation belongs in
[`middleware.md`](middleware.md) with its reason, and in the deviation column
below with a pointer. A deviation nobody wrote down is indistinguishable from a
defect, which is why the table has no "unknown" state.

**A known non-conformance is recorded, not hidden.** The last section is where
one lives until it is fixed. Removing a row from it requires the fix, not an
edit.

## What governs what

| Middleware | Governed by | Vendored as |
| --- | --- | --- |
| `Cors`, and the preflight | WHATWG Fetch §3.2 (CORS protocol) | [`whatwg-fetch-2026-06.html`](../references/whatwg-fetch-2026-06.html) |
| `Compression` | RFC 9110 §8.4 (`Content-Encoding`), §12.4.2 (qvalues), §12.5.3 (`Accept-Encoding`), §12.5.5 (`Vary`), §8.8.1 (validators), §14 (ranges) | [`rfc9110.txt`](../references/rfc9110.txt) |
| — its codings | RFC 9110 §8.4.1 (`gzip`, `deflate`, `compress`), RFC 7932 (`br`), RFC 9659 (`zstd`, and its 8 MB window) | [`rfc7932.txt`](../references/rfc7932.txt), [`rfc9659.txt`](../references/rfc9659.txt) |
| `Cache` | RFC 9111 §3 (storing), §4.1 (`Vary` matching), §4.2 (freshness), §4.4 (invalidation), §3.5 (`Authorization`) | [`rfc9111.txt`](../references/rfc9111.txt) |
| `Conditional` | RFC 9110 §13 (preconditions), §8.8.3 (comparison), §15.4.5 (304) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `BodySize` | RFC 9110 §15.5.14 (413), §10.1.1 (`Expect`) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `Timeout` | RFC 9110 §15.5.9 (408), §15.6.4 (503), §15.6.5 (504) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `Concurrency` | RFC 9110 §15.6.4 (503), §10.2.3 (`Retry-After`) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `RateLimit` | `draft-ietf-httpapi-ratelimit-headers-11` §3, RFC 9651 §3.3.3 (`sf-string`), RFC 6585 §4 (429), RFC 9110 §10.2.3 (`Retry-After`) | [`draft-…-ratelimit-headers-11.txt`](../references/draft-ietf-httpapi-ratelimit-headers-11.txt), [`rfc9651.txt`](../references/rfc9651.txt), [`rfc6585.txt`](../references/rfc6585.txt) |
| — its default spelling | RFC 6648 (deprecating `X-`), departed from deliberately | [`rfc6648.txt`](../references/rfc6648.txt) |
| `SetCookies`, and `response::cookie` | RFC 6265 §4.1 (`Set-Cookie`), `draft-ietf-httpbis-rfc6265bis-22` §4.1.3 (name prefixes), §5.6 (size) | [`rfc6265.txt`](../references/rfc6265.txt), [`draft-…-rfc6265bis-22.txt`](../references/draft-ietf-httpbis-rfc6265bis-22.txt) |
| `RequestId` | none. `X-Request-Id` is defined by no specification; RFC 6648 argues against the spelling and W3C Trace Context is the standardised alternative | [`rfc6648.txt`](../references/rfc6648.txt), [`w3c-trace-context-20211123.html`](../references/w3c-trace-context-20211123.html) |
| `Trace` | none | — |
| Message framing, wherever a body's length changes | RFC 9110 §8.6 (`Content-Length`), RFC 9112 §6 (HTTP/1.1), RFC 9113 §8.1.1 and §8.2.2 (HTTP/2) | [`rfc9112.txt`](../references/rfc9112.txt), [`rfc9113.txt`](../references/rfc9113.txt) |
| Problem responses | RFC 9457 | [`rfc9457.txt`](../references/rfc9457.txt) |
| `response::disposition` | RFC 6266, RFC 8187 | [`rfc6266.txt`](../references/rfc6266.txt), [`rfc8187.txt`](../references/rfc8187.txt) |

Two entries in that table are worth reading as statements rather than
references. `RequestId` and `Trace` are governed by nothing, and saying so is the
point: a reader looking for the specification behind `X-Request-Id` should find
out here that there is none, rather than concluding the citation was forgotten.

## Deviations, argued

Each of these departs from the document above it, on purpose, with the reasoning
in [`middleware.md`](middleware.md).

| Deviation | Why |
| --- | --- |
| `Compression` never sends 406, even when identity is refused | 406 is a status its `Short` does not declare, and the soundness invariant forbids sending one that is undeclared |
| `Compression` never encodes a ranged or range-advertising response | RFC 9110 §14.1.2 computes ranges over encoded octets and §8.8.1 forbids one strong validator naming two representations |
| `Compression` ships no `deflate` or `compress` | RFC 9110 §8.4.1.2 records that `deflate` is widely mis-implemented; `compress` is obsolete |
| `Cache` has no heuristic freshness | RFC 9111 §4.2.2 permits one; every heuristic is a guess that turns a correct origin into an incorrect cache |
| `Cache` never stores a response setting a cookie | `Vary` cannot protect against it — the cookie is in the response, and nothing in the request selects it |
| `Conditional` implements `If-None-Match` alone | `If-Match` and `If-Unmodified-Since` must be evaluated before the change, which only a handler can do |
| No `Last-Modified` / `If-Modified-Since` | Needs an HTTP-date implementation, which the dependency policy in [`architecture.md`](architecture.md) does not admit. Sending neither half is consistent; sending one is not |
| `RateLimit` keeps the `X-` prefix by default | The unprefixed names belong to a draft that has already replaced them once; squatting them would reach generated clients |
| No signed or encrypted cookies, no sessions | Arrives with a crypto stack the dependency table has no row for, in a default build no feature gate could contain |
| No default body cap | Would add 413 to every operation of every application that never asked for one |

## Known non-conformances

Departures that are **not** argued — defects, recorded here until they are
fixed. A row leaves this table only with its fix.

| # | Where | Requirement | Today |
| --- | --- | --- | --- |
| 1 | `middleware/cache/mod.rs` | RFC 9111 §4.4 — a cache **MUST** invalidate the target URI on a non-error response to an unsafe method | `CacheStore::invalidate` exists and is called from nowhere in `src/`. A `PUT` leaves the prior `GET` served until its freshness lapses |
| 2 | `middleware/limits.rs` | RFC 9110 §15.6.5 — 504 is for a server *acting as a gateway or proxy* awaiting an upstream response | `Timeout` answers 504 from an origin server with no upstream. It also covers the slow-body case, where §15.5.9's 408 is the accurate status |
| 3 | `response/cookie.rs` | `rfc6265bis-22` §4.1.3 — `__Secure-` and `__Host-` names carry attribute requirements a conformant user agent enforces by discarding the cookie | Neither prefix is checked. `__Host-x` without `Secure` renders, reaches the wire, and is dropped by the browser in silence |
| 4 | `middleware/rate_limit/headers.rs` | `draft-…-ratelimit-headers-11` §3.1.2 — `qu` **MUST** be a String | Rendered as a bare token. `content-bytes` parses, then mis-types against a strict client's schema |
| 5 | `middleware/cors/preflight.rs` | WHATWG Fetch — `*` in `Access-Control-Allow-Headers` does not cover `Authorization` | `allow_any_header()` answers `*` for a non-credentialed request, so every bearer-token preflight fails |
| 6 | `middleware/compression.rs` | RFC 9110 §8.6 — `Content-Length` counts the octets actually transferred | Not updated when the body is re-encoded. Unreachable for Kynos-authored responses, since hyper derives it; reachable through a hand-built response or a layer beneath `layer_unchecked` |
| 7 | `middleware/conditional/mod.rs` | RFC 9110 §15.4.5 — 304 indicates a request that *would have resulted in a 200* | The guard is `is_success()`, which admits 201, 202, 203, 204 and 206. A ranged 206 becomes a 304 replaying `ETag` and `Vary` but no `Content-Range`, and the client cannot tell which representation was current |

Numbers 2 and 6 reach a generated client; the rest are wire-level. All seven
were found by reading each module against its governing text rather than by a
failing test, which is the argument for this document existing.
