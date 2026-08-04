# Specification References

This directory contains standards texts vendored under their respective
upstream terms for implementation reference and auditability. Product scope and
normative precedence remain defined in [`docs/prd.md`](../docs/prd.md).

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
