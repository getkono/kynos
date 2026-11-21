"""Reports every crate the published packages depend on that is not on its
latest release, and fails only when it cannot say.

The graph is what crates.io users receive: every package reachable from a
publishable member (`publishable.py`) through a normal or build edge at
`--all-features`, transitive ones included. Dev-dependencies and what only an
unpublished member such as `kynos-profile` pulls in are left out. Cargo unifies
features across the workspace, so a feature only `kynos-profile` turns on can
still add an edge here. That can only add a dependency to the report, never hide
one.

The verdict is `cargo update --dry-run --verbose`, and with `resolver = "3"`
its "latest" already respects `rust-version`. Each line says one of four things
about a locked package:

- `compatible`: a semver- and MSRV-compatible release that `Cargo.lock` does
  not hold yet. A `cargo update` picks it up.
- `msrv`: the newer release needs a Rust above the MSRV.
- `incompatible`: the newer release is semver-incompatible, so it needs a
  requirement raised. The dependency's tier decides what that is.
- `held`: the newer release is semver-compatible and within the MSRV, but a
  requirement in the graph, such as an `=` pin, holds the lock below it.
- `downgrade`: the lockfile holds a version the MSRV-aware resolver would not
  pick.

Each one is a warning. The exit status is non-zero only when there is no
verdict: cargo failed, the published graph is empty, or a line has a verb this
parser knows but a shape it does not. Run as `mise run deps:latest`; the report
is written to `target/deps-latest-report.md`.
"""

import json
import os
import re
import subprocess
import sys
from collections import namedtuple
from pathlib import Path

# Before the sibling import, so a run writes no `.pyc` into the tree.
sys.dont_write_bytecode = True

from publishable import publishable  # noqa: E402

REPORT = Path("target/deps-latest-report.md")

Behind = namedtuple("Behind", "kind name current available requires_rust")

# What each kind means and how it is cleared, in report order.
KINDS = {
    "compatible": (
        "A compatible release is not locked",
        "Run `cargo update` on a branch from `master` before the release merges.",
    ),
    "msrv": (
        "The newer release needs a Rust above the MSRV",
        "Raising `rust-version` is a release decision of its own.",
    ),
    "incompatible": (
        "The newer release is semver-incompatible",
        "Raising the requirement follows the dependency's tier in "
        "docs/architecture.md: `chore(deps)` or a `!` commit.",
    ),
    "held": (
        "A requirement in the graph holds a compatible release back",
        "`cargo tree --invert` names the dependent; an `=` pin of ours is "
        "raised in the manifest, anyone else's waits for its release.",
    ),
    "downgrade": (
        "The lock holds a version the MSRV-aware resolver would not pick",
        "Run `cargo update` so the lock builds on the MSRV.",
    ),
}

VERSION = r"v(\S+)"
# A newer release the move does not reach, and the Rust it needs. `Unchanged`
# carries `available` when one exists; `Updating` carries it when a compatible
# step leaves one beyond. `requires Rust` alone is said of the locked version
# itself, which `msrv:check` holds, so it records nothing here.
AVAILABLE = (
    r"\((?:available: v([^,)\s]+)(?:, )?)?(?:requires Rust ([^)\s]+))?\)"
)
UPDATING = re.compile(rf"^Updating (\S+) {VERSION} -> {VERSION}(?: {AVAILABLE})?$")
DOWNGRADING = re.compile(
    rf"^Downgrading (\S+) {VERSION} -> {VERSION}(?: {AVAILABLE})?$"
)
UNCHANGED = re.compile(rf"^Unchanged (\S+) {VERSION} {AVAILABLE}$")
# `Updating crates.io index`, or `Updating git repository `…``: a source being
# fetched, not a package.
FETCHING = re.compile(r"^Updating (\S+ index|git repository .+)$")
IGNORED = ("Locking ", "Adding ", "Removing ", "note:", "warning:", "Blocking ")
VERBS = ("Updating ", "Unchanged ", "Downgrading ")


class Unparsed(ValueError):
    """A line with a verb this parser knows, in a shape it does not."""


def published_graph(metadata, roots):
    """The `(name, version)` of every registry package reachable from `roots`.

    Only normal and build edges are followed, never `dev` ones. `metadata` is
    `cargo metadata --format-version 1 --all-features`.
    """
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [
        package["id"] for package in metadata["packages"]
        if package["name"] in roots and package["source"] is None
    ]
    seen = set(pending)
    while pending:
        for dep in nodes[pending.pop()]["deps"]:
            if dep["pkg"] in seen:
                continue
            if all(kind["kind"] == "dev" for kind in dep["dep_kinds"]):
                continue
            seen.add(dep["pkg"])
            pending.append(dep["pkg"])
    return {
        (packages[id]["name"], packages[id]["version"])
        for id in seen
        if packages[id]["source"] is not None
    }


def compatible(current, newer):
    """Whether `newer` is semver-compatible with `current`, as Cargo's caret.

    The leftmost non-zero of major, minor and patch must match.
    """
    def key(version):
        core = re.split(r"[-+]", version, maxsplit=1)[0].split(".")
        major, minor, patch = (int(part) for part in core)
        if major:
            return (major,)
        if minor:
            return (0, minor)
        return (0, 0, patch)

    return key(current) == key(newer)


def parse_update(text):
    """Each package `cargo update --dry-run --verbose` reports as behind.

    Raises `Unparsed` for a line whose verb is known but whose shape is not,
    so a change in cargo's wording fails the check rather than emptying it.
    """
    records = []
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith(IGNORED) or FETCHING.match(line):
            continue
        if match := UPDATING.match(line) or DOWNGRADING.match(line):
            name, current, target, beyond, rust = match.groups()
            kind = "compatible" if line.startswith("Updating") else "downgrade"
            records.append(Behind(kind, name, current, target, None))
        elif match := UNCHANGED.match(line):
            name, current, beyond, rust = match.groups()
        elif line.startswith(VERBS):
            raise Unparsed(f"unrecognised `cargo update` line: {raw!r}")
        else:
            continue
        if beyond is not None:
            if not compatible(current, beyond):
                kind = "incompatible"
            elif rust:
                kind = "msrv"
            else:
                kind = "held"
            records.append(Behind(kind, name, current, beyond, rust))
    return records


def report(records):
    """The Markdown job summary for `records`."""
    lines = ["## Published dependencies behind their latest release", ""]
    if not records:
        return "\n".join([*lines, "None.", ""])
    lines += [
        "Warnings only. Each table says what clearing it takes.",
        "",
    ]
    for kind, (title, remedy) in KINDS.items():
        rows = sorted(r for r in records if r.kind == kind)
        if not rows:
            continue
        lines += [f"### {title}", "", remedy, "", "| Crate | Locked | Latest |"]
        lines.append("| --- | --- | --- |")
        for row in rows:
            latest = row.available
            if row.requires_rust:
                latest += f" (Rust {row.requires_rust})"
            lines.append(f"| `{row.name}` | {row.current} | {latest} |")
        lines.append("")
    return "\n".join(lines)


def describe(record):
    """`record` as one phrase: the crate, its locked version and the latest."""
    text = f"{record.name} {record.current} -> {record.available}"
    if record.requires_rust:
        text += f" (Rust {record.requires_rust})"
    return text


def annotations(records):
    """One GitHub warning per kind, naming each of its crates.

    One per kind rather than per crate: GitHub shows ten warnings a step and
    drops the rest, so per-crate lines would hide all but the first ten.
    """
    lines = []
    for kind, (title, _) in KINDS.items():
        rows = sorted(r for r in records if r.kind == kind)
        if rows:
            listed = ", ".join(describe(r) for r in rows)
            lines.append(f"::warning title={title} ({len(rows)})::{listed}")
    return lines


def run(args, stderr=subprocess.PIPE):
    """`args`'s stdout, or `None` if it failed.

    Stderr is left out, since a cold registry fills it with fetches; pass
    `stderr=subprocess.STDOUT` where it carries the result.
    """
    done = subprocess.run(args, stdout=subprocess.PIPE, stderr=stderr, text=True)
    if done.returncode != 0:
        print(done.stdout, end="", file=sys.stderr)
        print(done.stderr or "", end="", file=sys.stderr)
        print(f"{args[0]} {args[1]} exited {done.returncode}", file=sys.stderr)
        return None
    return done.stdout


def main():
    names = run(["cargo", "metadata", "--no-deps", "--format-version", "1"])
    metadata = run(
        ["cargo", "metadata", "--all-features", "--format-version", "1"]
    )
    # Cargo prints the verbs to stderr: they are the result.
    update = run(
        ["cargo", "update", "--dry-run", "--verbose", "--color", "never"],
        stderr=subprocess.STDOUT,
    )
    if names is None or metadata is None or update is None:
        return 1
    roots = set(publishable(json.loads(names)))
    graph = published_graph(json.loads(metadata), roots)
    if not graph:
        print("the published packages have no registry dependency", file=sys.stderr)
        return 1
    try:
        records = parse_update(update)
    except Unparsed as error:
        print(error, file=sys.stderr)
        return 1
    behind = [r for r in records if (r.name, r.current) in graph]

    if os.environ.get("GITHUB_ACTIONS") == "true":
        print("\n".join(annotations(behind)))
    else:
        for record in sorted(behind, key=lambda r: (list(KINDS).index(r.kind), r)):
            print(f"warning: {KINDS[record.kind][0]}: {describe(record)}")
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(report(behind))
    print(f"{len(behind)} of {len(graph)} published dependencies are behind")
    return 0


if __name__ == "__main__":
    sys.exit(main())
