# Security policy

## Reporting a vulnerability

Report privately through a
[security advisory](https://github.com/getkono/kynos/security/advisories/new).
Do not open a public issue, and do not raise it in Discussions.

Include what the bug form asks for — the version or commit, the exact feature
set, the toolchain, and a reproduction — plus what an attacker gets out of it.
A report naming a reachable code path is worth several describing a shape.

Expect acknowledgement within seven days. Disclosure is coordinated: a fix and
an advisory ship together, and you are credited unless you ask otherwise.

## Supported versions

The latest published release is supported, alongside `master`. Nothing older
is: Kynos is pre-1.0, and a fix is issued as a new patch release rather than
backported.

| Version | Supported |
| --- | --- |
| latest `0.x` release | yes |
| `master` | yes |
| any earlier release | no |

## Scope

In scope: anything reachable through the documented public API. Request
parsing and extraction, the emitted document diverging from what a handler
actually accepts or returns, TLS setup, security-scheme enforcement, panic
recovery, and resource exhaustion under adversarial input.

Out of scope:

- Anything reachable only through `layer_unchecked`, `route_unchecked` or
  `upgrade_unchecked`. The `unchecked` feature is a documented anti-pattern
  that stops the emitted document being authoritative and says so; something
  that requires it is a documentation defect, not a vulnerability.
- Advisories in upstream crates. Report those upstream. A Kynos advisory is
  for how Kynos uses a dependency, not for the dependency itself — if a pinned
  crate needs moving because of one, that is the dependency issue form.
- Denial of service that needs privileged network position or an already
  compromised host.
