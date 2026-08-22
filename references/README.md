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
complete document, including its copyright, authorship, license, and
pre-RFC-5378-material notices, and is distributed under those terms rather than
the repository's MIT license.

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
is distributed under those terms rather than the repository's MIT license. Unlike
RFC 9110 it carries no pre-RFC-5378-material notice.

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
repository's MIT license. Both postdate RFC 5378, so neither carries the
pre-RFC-5378-material notice RFC 9110's section records.

The same Revised BSD License treatment applies to Code Components extracted
from them — which includes the ABNF the encoder is written against.
