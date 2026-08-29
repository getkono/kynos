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
| `response::range` (`Served`, `ByteSource`) | RFC 9110 §14 (range requests), §13.1.2–13.1.5 (preconditions), §5.6.7 (HTTP dates), §9.3.2 (HEAD) | [`rfc9110.txt`](../references/rfc9110.txt) |
| `SetCookies`, and `response::cookie` | RFC 6265 §4.1 (`Set-Cookie`), `draft-ietf-httpbis-rfc6265bis-22` §4.1.3 (name prefixes), §5.6 (size) | [`rfc6265.txt`](../references/rfc6265.txt), [`draft-…-rfc6265bis-22.txt`](../references/draft-ietf-httpbis-rfc6265bis-22.txt) |
| `RequestId` | none. `X-Request-Id` is defined by no specification; RFC 6648 argues against the spelling and W3C Trace Context is the standardised alternative | [`rfc6648.txt`](../references/rfc6648.txt), [`w3c-trace-context-20211123.html`](../references/w3c-trace-context-20211123.html) |
| `Trace` | none | — |
| `Csrf` | W3C Fetch Metadata (`Sec-Fetch-Site`), WHATWG Fetch (`Origin`), RFC 9113 §8.3.1 (`:authority`) | [`w3c-fetch-metadata-20250401.html`](../references/w3c-fetch-metadata-20250401.html), [`whatwg-fetch-2026-06.html`](../references/whatwg-fetch-2026-06.html), [`rfc9113.txt`](../references/rfc9113.txt) |
| `Decompression` | RFC 9110 §8.4 (`Content-Encoding`), §15.5.16 (415), RFC 7932 (`br`), RFC 9659 (`zstd`) | [`rfc9110.txt`](../references/rfc9110.txt), [`rfc7932.txt`](../references/rfc7932.txt), [`rfc9659.txt`](../references/rfc9659.txt) |
| Forwarded addresses | RFC 7239 §5.2 (`for`), §6 (`nodename`), §8.1 (what it is worth) | [`rfc7239.txt`](../references/rfc7239.txt) |
| Message framing, wherever a body's length changes | RFC 9110 §8.6 (`Content-Length`), RFC 9112 §6 (HTTP/1.1), RFC 9113 §8.1.1 and §8.2.2 (HTTP/2) | [`rfc9112.txt`](../references/rfc9112.txt), [`rfc9113.txt`](../references/rfc9113.txt) |
| `response::language` | RFC 9110 §12.5.4 (`Accept-Language`), §8.5 (`Content-Language`), §8.5.1 (language tags), §12.4.2 (qvalues), §12.4.3 (wildcards), §12.5.5 (`Vary`), §12.1 and §15.5.7 (serving a default rather than a 406); RFC 4647 §2.1 (language ranges), §3.3.1 (Basic Filtering), §3.4 (Lookup); RFC 5646 §2.1 (the tag grammar), §2.1.1 (casing) | [`rfc9110.txt`](../references/rfc9110.txt), [`rfc4647.txt`](../references/rfc4647.txt), [`rfc5646.txt`](../references/rfc5646.txt) |
| Problem responses | RFC 9457 | [`rfc9457.txt`](../references/rfc9457.txt) |
| `response::disposition` | RFC 6266, RFC 8187 | [`rfc6266.txt`](../references/rfc6266.txt), [`rfc8187.txt`](../references/rfc8187.txt) |

Two entries in that table are worth reading as statements rather than
references. `RequestId` and `Trace` are governed by nothing, and saying so is the
point: a reader looking for the specification behind `X-Request-Id` should find
out here that there is none, rather than concluding the citation was forgotten.

**There is no `SecurityHeaders` row, and there was one.** It cited RFC 6797,
CSP Level 3, Referrer Policy, Permissions Policy and RFC 7034 for a middleware
`crates/kynos/src/middleware/` does not contain under any feature — so the
table asserted a mapping for a module that does not exist, which is the one
state the policy above says it must not have. The four texts stay vendored:
[`references/README.md`](../references/README.md) records that they are ahead
of the code rather than binding it.

## The document model

The middleware table is about HTTP. The OpenAPI specification governs
`crates/kynos-openapi/` in exactly the same way and had no entry here at all,
which left every rule in that crate — and every departure from one — with no
home under this document's own policy.

| Component | Governed by | Vendored as |
| --- | --- | --- |
| `model/` | OAS 3.1 §4 (Schema, and every object), OAS 3.2 §4 for the additions | [`3.1.2.md`](../references/3.1.2.md), [`3.2.0.md`](../references/3.2.0.md) |
| `validate/` | the MUSTs those texts state that a type cannot enforce: `operationId` uniqueness (3.1 §4.8.10), parameter uniqueness and path correspondence (§4.8.9), component key syntax (§4.8.7), style/location legality (§4.8.11) | same |
| `emit/downgrade` | the 3.1/3.2 delta itself: what 3.2 adds is what 3.1 cannot be handed | same |
| the meta-schemas | the OAI's own JSON Schema description of a document | [`oas-3.1-schema-2022-10-07.json`](../references/oas-3.1-schema-2022-10-07.json), [`oas-3.2-schema-2025-09-17.json`](../references/oas-3.2-schema-2025-09-17.json) |

Two departures, both argued in the code that makes them:

| Deviation | Why |
| --- | --- |
| `encoding` ⊕ `prefixEncoding`/`itemEncoding` is a validation rule, not a sum type | Every other stated exclusion in the model is a type. These two fields are 3.2-only, so a sum type would give one field a different shape in a 3.1 build than a 3.2 one — and a type that changes with a feature is the non-additivity `openapi32` exists to avoid. `validate/rules/content.rs` carries the argument |
| A requirement's name may be a URI under 3.2 but a bare undeclared name is still refused | 3.2 §4.8.30 admits both spellings. A single-segment relative reference is a legal URI, so reading one as a URI would accept every misspelling; 3.2 also says such a reference is spelled `./foo`. `validate/rules/operations.rs` carries the argument |

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
| `RateLimit` keeps the `X-` prefix by default | The unprefixed names belong to a draft that has already replaced them once; squatting them would reach generated clients |
| No signed or encrypted cookies, no sessions | Arrives with a crypto stack the dependency table has no row for, in a default build no feature gate could contain |
| No default body cap | Would add 413 to every operation of every application that never asked for one |
| Language negotiation runs Lookup, and falls back to Basic Filtering | RFC 9110 §12.5.4 declines to choose — "implementations can offer the most appropriate matching scheme" — and neither scheme is right alone. Basic Filtering serves `en-GB` nothing when only `en` is offered, which §12.5.4's own closing note complains about; Lookup serves a client asking for `en` nothing when the catalogue is keyed `en-US`. Lookup wins wherever it has an answer, so the fallback only decides what §3.4 would have abandoned |
| `Accept-Language: *` is honoured rather than discarded | RFC 4647 §3.4 says the wildcard "does not convey enough information by itself" to run Lookup. RFC 9110 §12.4.3 admits it in the field and gives it a meaning — it "selects unspecified values" — so it scores every tag no other range named, and refusing to honour a client that explicitly said "anything" would default it away for nothing |
| A language nobody offers is answered with the default, never a 406 | RFC 9110 §12.1 lets an origin decide a non-conforming response beats a 406, and §15.5.7 defines that status as the server being *unwilling* to supply a default. The exposure is asymmetric: a browser sends `*/*` and reaches `Accept`'s 406 almost never, but sends a narrow `Accept-Language` on every request. `Content-Language` is what keeps the fallback honest, which is why it is `required` |
| `Content-Language` states one tag, never the list §8.5 permits | §8.5 allows several for content "intended for multiple audiences". A negotiated response has one, and stating one is what lets the emitted schema enumerate the offer truthfully. A representation genuinely aimed at several audiences is not something this negotiation produces |

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

A second such reading has since happened, against the OpenAPI texts rather than
the HTTP ones. It found nine, and each is closed the same way:

| Was | Closed by |
| --- | --- |
| A 3.2 `Server.name` below the document root emitted under `openapi: 3.1.2`, which the 3.1 meta-schema rejects | a Server Object read wherever one hangs — the root, a Path Item, an Operation, a Link |
| Six enum variants gained a 3.2 field without being `#[non_exhaustive]`, so a downstream pattern broke when any crate enabled `openapi32` | the attribute on each variant, with the constructor surface completed so sealing them left the type buildable |
| Four objects the specification says MAY be extended could not be; two of them failed to *parse* a legal document | `Extensions` on `Paths`, `Callback`, `Discriminator` and `Xml` |
| A JSON `null` `example`, `default` or `const` was read back as absent | a deserializer that tells an absent field from a present `null`, at all eight sites |
| A legal 3.2 response stating only a `summary` would not parse | `description` optional, with 3.1's requirement moved to `validate` |
| `operationId` uniqueness — and every other operation rule — stopped at `paths`, though the MUST says "all operations described in the API" | webhooks, reusable path items, callbacks and `additionalOperations` walked too |
| A 3.2 requirement naming a scheme by URI was reported as an error | the second spelling 3.2 admits, gated on the version |
| A Responses Object holding only an extension satisfied "at least one response code" | a predicate that asks about responses rather than about emptiness |
| Component keys were checked in five of eleven sections, against a MUST that says "**All**" | every section |

Two more were found in the same reading and are *not* in that table, because
neither is a conformance failure: `#[derive(Tag)]` dropped 3.2-only members
silently under `openapi31`, and a `Concurrency` limit of zero built a service
that refused everything. Both are recorded where they live.
