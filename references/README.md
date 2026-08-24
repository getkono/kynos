# Specification References

This directory contains standards texts vendored under their respective
upstream terms for implementation reference and auditability. Product scope
lives in the [README](../README.md), and design decisions that bind
implementation work are recorded in [`docs/`](../docs/README.md).

## OpenAPI Specifications

These OpenAPI Specification texts are vendored from the Apache-2.0 licensed
[`OAI/OpenAPI-Specification`](https://github.com/OAI/OpenAPI-Specification)
repository. This section defines which vendored spec texts to consult.

### Patch-Version Policy

The OAS 3.x specification text defines a `major.minor.patch` versioning scheme:
`major.minor` designates the OAS feature set, while patch versions address errors
or clarifications and do not change the feature set. The vendored revision
histories also identify every `3.0.x` and `3.1.x` patch after `.0` as a patch
release.

Upstream release notes are consistent with that policy:

- [OAS 3.0.4](https://github.com/OAI/OpenAPI-Specification/releases/tag/3.0.4)
  makes no changes to requirements from 3.0.3.
- [OAS 3.1.1](https://github.com/OAI/OpenAPI-Specification/releases/tag/3.1.1)
  makes no changes to requirements from 3.1.0.
- [OAS 3.1.2](https://github.com/OAI/OpenAPI-Specification/releases/tag/3.1.2)
  has no material changes.

Therefore, implementation work should use only the latest vendored patch for each
minor line. Older patch files stay in this directory only as historical audit
material when checking how wording changed.

### Active References

Use these as the sole active references for their minor lines:

| Minor line | Active reference | Use |
| --- | --- | --- |
| 3.0 | [`3.0.4.md`](3.0.4.md) | Historical comparison and 3.0 rejection/dialect-difference checks |
| 3.1 | [`3.1.2.md`](3.1.2.md) | OpenAPI 3.1 feature-set reference |
| 3.2 | [`3.2.0.md`](3.2.0.md) | Future-minor comparison |

Do not use `3.0.0.md`, `3.0.1.md`, `3.0.2.md`, `3.0.3.md`, `3.1.0.md`, or
`3.1.1.md` for implementation decisions unless the task is explicitly auditing
historical wording between patch releases.

## HTTP Semantics

[`rfc9110.txt`](rfc9110.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 9110, HTTP
Semantics](https://www.rfc-editor.org/rfc/rfc9110.txt), retrieved on 2026-08-03.
Its SHA-256 digest is
`21c1cdce6ab0e5509b04d84a28000836c7a087cf786efe6f04877ebfff47232a`.
Consult the [RFC Editor information page](https://www.rfc-editor.org/info/rfc9110)
for the document's current status and reported errata.

The plain-text publication is vendored because it is an official RFC Editor
artifact and is substantially less markup-heavy than the official HTML and XML
formats. Do not reformat or otherwise modify it; apply errata in implementation
work rather than altering the published reference.

### Licensing

RFC 9110 was published under the IETF Trust's [Legal Provisions Relating to
IETF Documents](https://trustee.ietf.org/documents/trust-legal-provisions/tlp-5/)
(TLP 5.0). Section 3.c.i grants permission to copy and distribute an IETF
document in full and without modification. The vendored file preserves the
complete document, including its copyright, authorship and license notices, and
is distributed under those terms rather than the repository's MIT license. It
postdates RFC 5378, so it carries no pre-RFC-5378-material notice.

The document's embedded notice requires separate Revised BSD License treatment
for Code Components extracted from it. Consult the TLP before copying RFC text
or code into source files.

## Problem Details

[`rfc9457.txt`](rfc9457.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 9457, Problem Details for HTTP
APIs](https://www.rfc-editor.org/rfc/rfc9457.txt), retrieved on 2026-08-10. Its
SHA-256 digest is
`f2b3db92fb0bf3489cb3841a0da0c0d88dff40797b64d40b6123085183886c7b`.
Consult the [RFC Editor information page](https://www.rfc-editor.org/info/rfc9457)
for the document's current status and reported errata.

Consult it whenever the wire form of an error is involved. Problem details are
the only error representation Kynos emits, so the registered members, the
`application/problem+json` media type and the extension-member rules bind
[`error::problem`](../crates/kynos/src/error/problem.rs) and everything
`#[derive(ApiError)]` produces; [`docs/errors.md`](../docs/errors.md) records how
that maps onto the framework's types.

RFC 9457 obsoletes RFC 7807, which is not vendored. Treat 7807 field names and
the older `application/problem+json` registration as historical: where a client
library still speaks 7807, the difference is additive and 9457 is the reference.

### Licensing

RFC 9457 was published under the IETF Trust's [Legal Provisions Relating to
IETF Documents](https://trustee.ietf.org/documents/trust-legal-provisions/tlp-5/)
(TLP 5.0), on the same terms as RFC 9110 above. The vendored file preserves the
complete document, including its copyright, authorship and license notices, and
is distributed under those terms rather than the repository's MIT license. Like RFC 9110
it postdates RFC 5378 and so carries no pre-RFC-5378-material notice.

The same Revised BSD License treatment applies to Code Components extracted from
it — which includes the JSON and schema fragments in its examples.

## Content Disposition

[`rfc6266.txt`](rfc6266.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 6266, Use of the
Content-Disposition Header Field in the Hypertext Transfer Protocol
(HTTP)](https://www.rfc-editor.org/rfc/rfc6266.txt), retrieved on 2026-08-22.
Its SHA-256 digest is
`2887d464e7a2a15877aba5d54b2e8e0c06c32294c44c9d95eb8940069ab43d33`.
Consult the [RFC Editor information page](https://www.rfc-editor.org/info/rfc6266)
for the document's current status and reported errata.

[`rfc8187.txt`](rfc8187.txt) is the same for [RFC 8187, Indicating Character
Encoding and Language for HTTP Header Field
Parameters](https://www.rfc-editor.org/rfc/rfc8187.txt), retrieved on the same
date. Its SHA-256 digest is
`af50e65257dc16a27fa85e25ef68d49c8fa2a4abc21094a00469462c8cb89b53`.
Consult its [information page](https://www.rfc-editor.org/info/rfc8187) on the
same terms.

The pair is vendored together because neither is usable alone here: RFC 6266
defines `filename` and defers the non-ASCII spelling to RFC 8187's `ext-value`,
so a `filename*` parameter is written against §4.1 of the first and §3.2.1 of
the second at once. They bind
[`response::disposition`](../crates/kynos/src/response/disposition.rs), which is
the only place Kynos writes the field.

RFC 9110 does not define `Content-Disposition` and does not mention it; it
names RFC 8187 as a normative reference and nothing more. Neither of these
documents is reachable from [`rfc9110.txt`](rfc9110.txt), which is why both are
here rather than left to it.

RFC 8187 obsoletes RFC 5987, which is not vendored. Treat 5987 as historical:
8187 narrows the charset to UTF-8 and is the reference.

### Licensing

Both were published under the IETF Trust's [Legal Provisions Relating to IETF
Documents](https://trustee.ietf.org/documents/trust-legal-provisions/tlp-5/)
(TLP 5.0), on the same terms as RFC 9110 and RFC 9457 above. The vendored files
preserve the complete documents, including their copyright, authorship and
license notices, and are distributed under those terms rather than the
repository's MIT license. Both postdate RFC 5378, so neither carries a
pre-RFC-5378-material notice.

The same Revised BSD License treatment applies to Code Components extracted
from them — which includes the ABNF the encoder is written against.

## Verifying this directory

[`SHA256SUMS`](SHA256SUMS) lists every vendored file. `mise run references:verify`
checks it three ways: that no file was edited in place, that no file was added
without being listed, and that every digest quoted in this README still names
something. The prose below is what a reader consults and `SHA256SUMS` is what CI
checks, so the third check is what stops the two from becoming two statements of
one fact that drift apart.

Do not reformat a vendored file. Retrieve it again and update its digest instead.

## HTTP Caching

[`rfc9111.txt`](rfc9111.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 9111, HTTP
Caching](https://www.rfc-editor.org/rfc/rfc9111.txt), retrieved on 2026-08-23.
Its SHA-256 digest is
`aeb52adb3279d5f23dae34f68af11bd5cef0a0aff7ffcd014c9ca93c5302cf3e`.
Consult the [RFC Editor information page](https://www.rfc-editor.org/info/rfc9111)
for the document's current status and reported errata.

It binds [`middleware::cache`](../crates/kynos/src/middleware/cache/mod.rs) and
the freshness rules beside it. Section 4.4's invalidation requirement, section
4.1's `Vary` matching, section 4.2.1's ordered freshness calculation and section
3.5's `Authorization` rules are the four a change to that module most often
turns on. RFC 9110 defines the conditional-request machinery the cache leans on
but not the storage model, which is why both are here.

## HTTP/1.1 and HTTP/2

[`rfc9112.txt`](rfc9112.txt) is the same for [RFC 9112, HTTP/1.1
](https://www.rfc-editor.org/rfc/rfc9112.txt), retrieved on 2026-08-23, with
SHA-256 digest
`e4f426bac6206b67fdf9e0da826154f70588db2133a0a86b15cde4ff725d8937`.
[`rfc9113.txt`](rfc9113.txt) is the same for [RFC 9113,
HTTP/2](https://www.rfc-editor.org/rfc/rfc9113.txt), retrieved on the same date,
with SHA-256 digest
`a00ef91b64e111a282e77ec66980f5242e77c0bb5e33e0927e3b6757080506de`.

The pair is vendored together because message framing is the one place the two
versions genuinely disagree and a response has to satisfy whichever carried it.
RFC 9112 section 6.3 terminates a HEAD, 1xx, 204 or 304 response at the end of
the header section *regardless of the fields present*; RFC 9113 section 8.2.2
makes `Transfer-Encoding` malformed outright, so the chunked framing RFC 9112
permits has no HTTP/2 equivalent. Anything that changes a body's length after
the head is built — compression above all — is written against both.

RFC 9113 obsoletes RFC 7540 and RFC 8740, neither of which is vendored.

## Cookies

[`rfc6265.txt`](rfc6265.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 6265, HTTP State Management
Mechanism](https://www.rfc-editor.org/rfc/rfc6265.txt), retrieved on 2026-08-23.
Its SHA-256 digest is
`dc4177094813e85a44a7897b892b76bc09160b3a03aea44f0f511f3a98f91933`.

[`draft-ietf-httpbis-rfc6265bis-22.txt`](draft-ietf-httpbis-rfc6265bis-22.txt) is
the same for [draft-ietf-httpbis-rfc6265bis-22, Cookies: HTTP State Management
Mechanism](https://www.ietf.org/archive/id/draft-ietf-httpbis-rfc6265bis-22.txt),
retrieved on the same date, with SHA-256 digest
`762b37b40b2286da2e9250bb24a0f9dfca790b11fe20bcde0c67fcbbad28979d`.

**The second is an Internet-Draft, and an expired one.** Revision 22 carried an
expiry of 2026-06-04 and no revision has superseded it; no RFC obsoletes RFC 6265.
Cite it as a snapshot of work in progress, never as a published standard, and
re-check the datatracker before relying on any section that a later revision
might have moved.

The pair is vendored together because the half that matters most here exists only
in the draft. `__Secure-` and `__Host-` name prefixes (section 4.1.3) are
implemented by every current browser and appear nowhere in RFC 6265, and the
4096-byte size limit is stated as a user-agent requirement in the draft's section
5.6 where RFC 6265 gives it only as a minimum a user agent must support. Both are
requirements on the *user agent*: a server that ignores them emits a
`Set-Cookie` a conformant browser discards in silence, which is why they bind
[`response::cookie`](../crates/kynos/src/response/cookie.rs) even though the
server-side wording is a SHOULD.

## Rate Limiting

[`draft-ietf-httpapi-ratelimit-headers-11.txt`](draft-ietf-httpapi-ratelimit-headers-11.txt)
is an unmodified, byte-for-byte copy of [draft-ietf-httpapi-ratelimit-headers-11,
RateLimit header fields for
HTTP](https://www.ietf.org/archive/id/draft-ietf-httpapi-ratelimit-headers-11.txt),
retrieved on 2026-08-23. Its SHA-256 digest is
`d6016dd0db5a33ba3f1e6b6f4c84e9b5ecc00986822d5212bab831f6c2d1d412`.

**This is an Internet-Draft**, expiring 2026-11-24. It is the reason
[`middleware::rate_limit`](../crates/kynos/src/middleware/rate_limit/mod.rs)
keeps the `X-` prefixed spelling as its default and offers the structured
spelling as a type-state rather than adopting it outright;
[`docs/middleware.md`](../docs/middleware.md) carries that argument. Sections
3.1.1 to 3.1.4 give the parameter types — `q` an Integer, `qu` a **String**, `w`
an Integer, `pk` a **Byte Sequence** — which are what the rendered field is
checked against.

[`rfc6585.txt`](rfc6585.txt) is the same for [RFC 6585, Additional HTTP Status
Codes](https://www.rfc-editor.org/rfc/rfc6585.txt), retrieved on the same date,
with SHA-256 digest
`f6d55d1b491cd515c35827cf9181753b23b2a68c4df14e56d83dc445b3876e58`. It defines
429, which RFC 9110 references but does not restate.

## Structured Field Values

[`rfc9651.txt`](rfc9651.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 9651, Structured Field Values
for HTTP](https://www.rfc-editor.org/rfc/rfc9651.txt), retrieved on 2026-08-23.
Its SHA-256 digest is
`fe27f2ec8819911afbe4bd11f6fcb947580da4c49e5423a1fff960e252ced26d`.

It is the serialization the RateLimit draft's fields are written in, so the two
are read together: section 3.3.3's `sf-string` is what a String-typed parameter
must be rendered as, and a value that happens to be a valid token is *not* the
same thing.

RFC 9651 obsoletes RFC 8941, which is not vendored. Treat 8941 as historical.

## Deprecating "X-" Prefixes

[`rfc6648.txt`](rfc6648.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 6648, Deprecating the "X-"
Prefix and Similar Constructs in Application
Protocols](https://www.rfc-editor.org/rfc/rfc6648.txt), retrieved on 2026-08-23.
Its SHA-256 digest is
`f826d22153c972b27df0045e95266173953eb10e9274cb296b7bac364185ef88`.

Kynos ships two `X-` prefixed field names — `X-RateLimit-*` and `X-Request-Id` —
and this is the document that says it should not. Both are deliberate; consult it
before adding a third.

## Forwarded

[`rfc7239.txt`](rfc7239.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 7239, Forwarded HTTP
Extension](https://www.rfc-editor.org/rfc/rfc7239.txt), retrieved on 2026-08-23.
Its SHA-256 digest is
`f15c45c9e079684bc4284d041be63285923962448cb0d8540d54106c68c2a40d`.

It defines the `Forwarded` field and the `for`, `by`, `proto` and `host`
parameters, and — more usefully here — section 8 is explicit that the field is
attacker-controlled unless every hop that wrote it is trusted. That is the whole
reason a client address resolved from it needs a stated trust policy rather than
a default. `X-Forwarded-For` and `X-Forwarded-Proto` are de-facto and appear in
no specification; RFC 7239 section 7.1 is the closest thing to a description of
them.

## Strict Transport Security

[`rfc6797.txt`](rfc6797.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 6797, HTTP Strict Transport
Security (HSTS)](https://www.rfc-editor.org/rfc/rfc6797.txt), retrieved on
2026-08-23. Its SHA-256 digest is
`7cefa03f77547532580b7f877b4e6388bb66c20631b624667a0065c14ae3d364`.

Section 7.2 is the one that constrains an implementation rather than a policy: an
HSTS host **must not** include the field in a response conveyed over
non-secure transport. Emitting it unconditionally is therefore not a harmless
default, and knowing whether the transport was secure is a prerequisite rather
than a refinement.

## Content Codings

[`rfc7932.txt`](rfc7932.txt) is an unmodified, byte-for-byte copy of the RFC
Editor's official plain-text publication of [RFC 7932, Brotli Compressed Data
Format](https://www.rfc-editor.org/rfc/rfc7932.txt), retrieved on 2026-08-23,
with SHA-256 digest
`394abd1879e016f4eac89512c2acaf92f7b2d67feb8ab58b6b118c213be07146`.

[`rfc8878.txt`](rfc8878.txt) is the same for [RFC 8878, Zstandard Compression and
the "application/zstd" Media Type](https://www.rfc-editor.org/rfc/rfc8878.txt),
with SHA-256 digest
`8ee6be03534113f5689cda75b9539a02e0704a2506d420814223e506420aeea4`.

[`rfc9659.txt`](rfc9659.txt) is the same for [RFC 9659, Window Sizing for
Zstandard Content Encoding](https://www.rfc-editor.org/rfc/rfc9659.txt), with
SHA-256 digest
`a43584f250506db54df8bc9ff90652888135369fbc331453f67a71829b0827a2`.

The third is not optional reading beside the second. RFC 8878 registered the
`zstd` content coding with a recommended window; RFC 9659 updates it so that an
encoder producing frames for HTTP must not require a window larger than 8 MB, and
current browsers enforce that. Citing 8878 alone for the coding is out of date.

`gzip`, `deflate`, `compress` and `identity` are defined in RFC 9110 section
8.4.1 and section 12.5.3 and need nothing further. These three are here because
`br` and `zstd` are not in RFC 9110 at all.

### Licensing

RFC 6265, 6585, 6648, 6797, 7239, 7932, 8878, 9111, 9112, 9113, 9651 and 9659,
and both vendored Internet-Drafts, were published under the IETF Trust's [Legal
Provisions Relating to IETF
Documents](https://trustee.ietf.org/documents/trust-legal-provisions/tlp-5/)
(TLP 5.0), on the same terms as RFC 9110 above. Each vendored file preserves the
complete document, including its copyright, authorship and license notices, and
is distributed under those terms rather than the repository's MIT license.

All of them postdate RFC 5378, so none carries a pre-RFC-5378-material notice —
which is true of every text in this directory. The same Revised BSD License treatment applies
to Code Components extracted from any of them, which includes every ABNF rule an
implementation is written against.

## Living standards

The six documents below are not RFCs and are not immutable. Each is vendored as
a **dated snapshot** chosen because its publisher offers one: WHATWG cuts
periodic review drafts, and every W3C technical report has a dated version URL
beside its undated "latest version" pointer. The undated URL is what a reader
should follow to see what changed; the vendored file is what an implementation
was written against, and it is the only form worth checksumming.

They are HTML because neither publisher issues a plain-text form. That is the one
respect in which they differ from every other file here.

### CORS

[`whatwg-fetch-2026-06.html`](whatwg-fetch-2026-06.html) is an unmodified,
byte-for-byte copy of the [Fetch Standard Review Draft, June
2026](https://fetch.spec.whatwg.org/review-drafts/2026-06/), retrieved on
2026-08-23. Its SHA-256 digest is
`9353f1399c2191979c44b43fd2753b69135229130c8f3f2bbbb57c01737a5033`.

Fetch, not RFC 9110, is what defines CORS: the preflight, the
`Access-Control-*` fields, and the rule that a wildcard never satisfies a
credentialed request. It binds
[`middleware::cors`](../crates/kynos/src/middleware/cors/mod.rs). Worth knowing
before reading that module: `*` in `Access-Control-Allow-Headers` is a literal
field name for a credentialed request, and even without credentials it does not
cover `Authorization`, which always has to be named.

### Fetch Metadata

[`w3c-fetch-metadata-20250401.html`](w3c-fetch-metadata-20250401.html) is an
unmodified, byte-for-byte copy of the W3C Working Draft of [Fetch Metadata
Request Headers, 1 April
2025](https://www.w3.org/TR/2025/WD-fetch-metadata-20250401/), retrieved on
2026-08-23. Its SHA-256 digest is
`a16ceda347ab6421c450a3c9c402446204073fc395cf8dc36c7c2425f5ff4b77`.

It defines `Sec-Fetch-Site` and the rest of the `Sec-Fetch-*` family as headers
the browser sets and script cannot forge. That property is what makes a
tokenless, sessionless CSRF defence possible at all, which is why this is here
rather than a CSRF token scheme.

### Trace Context

[`w3c-trace-context-20211123.html`](w3c-trace-context-20211123.html) is an
unmodified, byte-for-byte copy of the W3C Recommendation [Trace Context, 23
November 2021](https://www.w3.org/TR/2021/REC-trace-context-1-20211123/),
retrieved on 2026-08-23. Its SHA-256 digest is
`9e7228d2a91c5aa4bef6e7f610366a2000274ee51699053e10c8ac3f0b8965be`.

It defines `traceparent` and `tracestate`. Kynos ships no tracing backend, so
this binds nothing today; it is vendored because the correlation-identifier
surface is deliberately generic over its header group and this is the
standardised group a reader is most likely to reach for.

Unlike the other three this is a Recommendation rather than a draft, so the
dated snapshot and the latest version are the same document.

### Security response headers

Three snapshots, vendored together because they are configured together and
separately from anything else here:

| File | Document | SHA-256 |
| --- | --- | --- |
| [`w3c-csp3-20260813.html`](w3c-csp3-20260813.html) | [Content Security Policy Level 3, 13 August 2026](https://www.w3.org/TR/2026/WD-CSP3-20260813/) | `a28120328f5265dbf2028d8a7272bece363016915b3d6e53641643c270ac65b2` |
| [`w3c-referrer-policy-20170126.html`](w3c-referrer-policy-20170126.html) | [Referrer Policy, 26 January 2017](https://www.w3.org/TR/2017/CR-referrer-policy-20170126/) | `4e60be213bbe74353fd4be29e519c9dd046f3dc6f012cc1149a7d50234a81e81` |
| [`w3c-permissions-policy-20260618.html`](w3c-permissions-policy-20260618.html) | [Permissions Policy, 18 June 2026](https://www.w3.org/TR/2026/WD-permissions-policy-1-20260618/) | `6b3c9df3a8d6fb06214569ab5548a3cd48906a99b04c14af57f54506ce01436e` |

All three were retrieved on 2026-08-23.

`X-Content-Type-Options` and `X-Frame-Options` are in none of them: the first is
defined by WHATWG's Fetch and HTML standards, and the second by RFC 7034, which
is Informational and largely superseded by CSP's `frame-ancestors`. Both are
`X-` prefixed names that predate the deprecation in
[`rfc6648.txt`](rfc6648.txt) and are kept by every browser regardless, which is
the reason they are emitted and the reason no specification here defines them.

### Licensing

The Fetch Standard is © WHATWG (Apple, Google, Mozilla, Microsoft) and licensed
under [Creative Commons Attribution 4.0
International](https://creativecommons.org/licenses/by/4.0/), which permits
redistribution with attribution. The vendored file preserves the complete
document including its attribution and license notice.

The four W3C documents are © W3C and distributed under the [W3C Software and
Document License](https://www.w3.org/copyright/software-license/), except
Referrer Policy, which predates it and carries the older [W3C Document
License](https://www.w3.org/copyright/document-license/). Both permit
redistribution of an unmodified copy that keeps its notices, which each vendored
file does.

None of these five is distributed under the repository's MIT license.
