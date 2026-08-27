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
| `Timeout` | RFC 9110 §15.5.9 (408) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `Concurrency` | RFC 9110 §15.6.4 (503), §10.2.3 (`Retry-After`) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `RateLimit` | `draft-ietf-httpapi-ratelimit-headers-11` §3, RFC 9651 §3.3.3 (`sf-string`), RFC 6585 §4 (429), RFC 9110 §10.2.3 (`Retry-After`) | [`draft-…-ratelimit-headers-11.txt`](../references/draft-ietf-httpapi-ratelimit-headers-11.txt), [`rfc9651.txt`](../references/rfc9651.txt), [`rfc6585.txt`](../references/rfc6585.txt) |
| — its default spelling | RFC 6648 (deprecating `X-`), departed from deliberately | [`rfc6648.txt`](../references/rfc6648.txt) |
| `SetCookies`, and `response::cookie` | RFC 6265 §4.1 (`Set-Cookie`), `draft-ietf-httpbis-rfc6265bis-22` §4.1.3 (name prefixes), §5.6 (size) | [`rfc6265.txt`](../references/rfc6265.txt), [`draft-…-rfc6265bis-22.txt`](../references/draft-ietf-httpbis-rfc6265bis-22.txt) |
| `RequestId` | none. `X-Request-Id` is defined by no specification; RFC 6648 argues against the spelling and W3C Trace Context is the standardised alternative | [`rfc6648.txt`](../references/rfc6648.txt), [`w3c-trace-context-20211123.html`](../references/w3c-trace-context-20211123.html) |
| `Trace` | none | — |
| `Csrf` | W3C Fetch Metadata (`Sec-Fetch-Site`), WHATWG Fetch (`Origin`), RFC 9113 §8.3.1 (`:authority`) | [`w3c-fetch-metadata-20250401.html`](../references/w3c-fetch-metadata-20250401.html), [`whatwg-fetch-2026-06.html`](../references/whatwg-fetch-2026-06.html), [`rfc9113.txt`](../references/rfc9113.txt) |
| `SecurityHeaders` | RFC 6797 §6.1 and §7.2 (HSTS), W3C CSP Level 3, Referrer Policy, Permissions Policy, RFC 7034 (`X-Frame-Options`) | [`rfc6797.txt`](../references/rfc6797.txt), [`w3c-csp3-20260813.html`](../references/w3c-csp3-20260813.html), [`w3c-referrer-policy-20170126.html`](../references/w3c-referrer-policy-20170126.html), [`w3c-permissions-policy-20260618.html`](../references/w3c-permissions-policy-20260618.html) |
| `Decompression` | RFC 9110 §8.4 (`Content-Encoding`), §15.5.16 (415), RFC 7932 (`br`), RFC 9659 (`zstd`) | [`rfc9110.txt`](../references/rfc9110.txt), [`rfc7932.txt`](../references/rfc7932.txt), [`rfc9659.txt`](../references/rfc9659.txt) |
| Forwarded addresses | RFC 7239 §5.2 (`for`), §6 (`nodename`), §8.1 (what it is worth) | [`rfc7239.txt`](../references/rfc7239.txt) |
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
| `Compression` never encodes beneath a strong validator | RFC 9110 §8.8.1: a validator shared by a coded and an uncoded representation *is* weak, and the encoder cannot restate one from where it sits |
| A streamed encode sends no `Content-Length` | RFC 9110 §8.6 forbids forwarding one known to be incorrect, and the encoded length is not known until after the head has gone |
| `Compression` never encodes a ranged or range-advertising response | RFC 9110 §14.1.2 computes ranges over encoded octets and §8.8.1 forbids one strong validator naming two representations. An asset set answers `Accept-Encoding` itself instead, minting a validator per stored coding, which is the same sections satisfied rather than traded against |
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

**There are none today.** The seven this document was written to record are all
closed, each by a change that names the requirement it was failing:

| Was | Closed by |
| --- | --- |
| `CacheStore::invalidate` called from nowhere, so a `PUT` left the prior `GET` served | RFC 9111 §4.4 honoured on any non-error answer to an unsafe method |
| `Timeout` answering 504 from an origin server with no upstream | 408, and `middleware.md` records that neither status is exact |
| `__Secure-` and `__Host-` names carrying none of what their prefix promises | both prefixes completed where that narrows nothing, and refused where it would widen |
| `qu` rendered as a bare token against a MUST for a String | rendered through the same `sf_string` as the two names beside it |
| `*` in `Access-Control-Allow-Headers` not covering `Authorization` | the field names `authorization` outright beside the wildcard |
| `Content-Length` not restated when the body is re-encoded | the encoded length stated, and a streamed encode stating none at all |
| A 304 minted from any `is_success()`, 206 included | minted only from a 200 |

Keeping the table rather than deleting it is the point: an empty one is a claim
that somebody looked, and it is where the next such reading belongs.

All seven were found by reading each module against its governing text rather
than by a failing test, which is the argument for this document existing. Each
now has one.
