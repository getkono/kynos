"""Public API checks: what each published library exposes, and whose types.

Lists `kynos-openapi` and `kynos` with `cargo public-api`, holds each listing to
the snapshot committed beside the crate, and holds every foreign crate the
listings name to the Public dependencies allowance in `docs/architecture.md`,
which is read rather than restated. The same pass holds the async-machinery and
runtime rules of that document's Public API surface over the listing, and the
*Fix reaches* column of its graph against what the listing shows.

`KYNOS_PUBLIC_API=overwrite` records the listings as the snapshots instead of
comparing them; every other rule still runs.
"""

import difflib
import os
import re
import subprocess
import sys
from pathlib import Path

# Relative to the script, so a run from any directory checks the same tree.
ROOT = Path(__file__).resolve().parent.parent
ARCHITECTURE = ROOT / "docs/architecture.md"

#: The libraries listed. `kynos-macros` is a proc-macro crate, whose public API
#: is macro names and names no type.
CRATES = ("kynos-openapi", "kynos")
SNAPSHOT = "public-api.txt"
#: One target, so a `cfg(unix)` item does not make the snapshot a function of
#: the host that recorded it.
TARGET = "x86_64-unknown-linux-gnu"
#: Blanket impls are omitted: they name whichever crate declares the blanket
#: trait (`equivalent::Equivalent` through `indexmap`), which is no item of
#: Kynos's. Auto-trait impls stay, since a lost `Send` is a break.
LISTING_ARGS = (
    "--all-features",
    "--omit",
    "blanket-impls",
    "--target",
    TARGET,
    "--color",
    "never",
)

NUMBERS = {
    "one": 1,
    "two": 2,
    "three": 3,
    "four": 4,
    "five": 5,
    "six": 6,
    "seven": 7,
    "eight": 8,
    "nine": 9,
    "ten": 10,
}

# --- Reading the allowance ---------------------------------------------------

ALLOWANCE_MARKER = "**Public dependencies.**"
PENDING = re.compile(
    r"^(\w+) (?:crates are|crate is) public API today without being in the allowance(.*?)(?:\n\n|\Z)",
    re.MULTILINE | re.DOTALL,
)
BACKTICKED = re.compile(r"`([^`]+)`")
CRATE = re.compile(r"[a-z][a-z0-9_-]*")
#: The crate whose allowance bullet names the only items that may name it.
RUNTIME = "tokio"


def crates_in(text):
    """The backticked tokens of `text` spelled as a crate name, in order."""
    return [token for token in BACKTICKED.findall(text) if CRATE.fullmatch(token)]


def allowance(architecture, failures):
    """The Public dependencies list, as `{crate: [items its reason names]}`.

    Each bullet is crates, an em dash, and the reason; an item is a backticked
    `Type::member` in the reason. An unreadable list is reported and empty.
    """
    start = architecture.find(ALLOWANCE_MARKER)
    if start < 0:
        failures.append(
            f"docs/architecture.md has no {ALLOWANCE_MARKER} list, so no foreign "
            "crate in a public item is allowed and nothing says which should be. "
            "Restore the list, or move this gate in the same commit"
        )
        return {}
    bullets = []
    for line in architecture[start:].splitlines()[1:]:
        if line.startswith("- "):
            bullets.append(line[2:])
        elif line.startswith("  ") and bullets:
            bullets[-1] += " " + line.strip()
        elif bullets:
            break
    if not bullets:
        failures.append(f"docs/architecture.md's {ALLOWANCE_MARKER} list has no bullet")
    allowed = {}
    for text in bullets:
        head, dash, reason = text.partition(" — ")
        crates = crates_in(head)
        if not dash or not crates:
            failures.append(
                "a Public dependencies bullet is not `crates — reason`, so the "
                f"crates it allows cannot be read:\n    {text}"
            )
        for crate in crates:
            allowed[crate] = [token for token in BACKTICKED.findall(reason) if "::" in token]
    return allowed


def pending(architecture, failures):
    """The crates public today outside the allowance, each awaiting its issue.

    Read from the paragraph that counts them, whose count must agree.
    """
    match = PENDING.search(architecture)
    if match is None:
        failures.append(
            "docs/architecture.md no longer says which crates are public API "
            "without being in the allowance (`N crates are public API today "
            "without being in the allowance`). Say `No crates` rather than "
            "deleting the sentence, so the count stays checkable"
        )
        return []
    word, body = match.group(1).lower(), match.group(2)
    crates = crates_in(body)
    if word == "no":
        stated = 0
    elif word in NUMBERS:
        stated = NUMBERS[word]
    else:
        failures.append(f"`{match.group(1)}` is not a count this gate reads")
        return crates
    if stated != len(crates):
        failures.append(
            f"docs/architecture.md says {match.group(1).lower()} crates are public "
            f"API outside the allowance and names {len(crates)}: "
            + ", ".join(crates)
        )
    return crates


# --- Reading the graph's tiers -----------------------------------------------

GRAPH_HEADER = "| Layer | Crate | Named in | Fix reaches | Status |"
TIERS = ("auxiliary", "core", "public API")
TIER_OF = re.compile(r"(auxiliary|core|public API)\s*\(([^)]*)\)")


def cells(row):
    """The cells of a Markdown table row, `|` inside backticks kept."""
    out, cell, ticked = [], "", False
    for char in row.strip().strip("|"):
        if char == "`":
            ticked = not ticked
        if char == "|" and not ticked:
            out.append(cell.strip())
            cell = ""
        else:
            cell += char
    out.append(cell.strip())
    return out


def tiers(architecture, failures):
    """Each graph crate's tier, `{crate: tier}`, from the *Fix reaches* column.

    A cell is one tier for every crate in its row, or `tier (`crate`)` pairs. A
    row naming no crate (`—`) is skipped; a row with no tier is reported.
    """
    start = architecture.find(GRAPH_HEADER)
    if start < 0:
        failures.append(
            "docs/architecture.md has no graph table with a Fix reaches column, "
            "so no crate has a tier to check"
        )
        return {}
    tiered = {}
    for row in architecture[start:].splitlines()[2:]:
        if not row.startswith("|"):
            break
        _, crate_cell, _, reach, _ = cells(row)[:5]
        crates = crates_in(crate_cell)
        if not crates:
            continue
        pairs = TIER_OF.findall(reach)
        if pairs:
            assigned = {crate: tier for tier, names in pairs for crate in crates_in(names)}
        elif reach in TIERS:
            assigned = {crate: reach for crate in crates}
        else:
            failures.append(
                "a graph row's Fix reaches cell names no tier, so what an "
                f"upstream fix to {', '.join(crates)} may change is unstated: "
                f"`{reach}`. Give it one of {', '.join(TIERS)}"
            )
            continue
        for crate, tier in assigned.items():
            if tiered.setdefault(crate, tier) != tier:
                failures.append(
                    f"the graph tiers `{crate}` both {tiered[crate]} and {tier}"
                )
    return tiered


# --- Reading the async-machinery exception ----------------------------------

ERASED = re.compile(
    r"The rule is scoped to the checked surface(.*?)those (\w+) are the only ones",
    re.DOTALL,
)
LINKED_NAME = re.compile(r"\[`(\w+)`\]\(")


def erased(architecture, failures):
    """The types whose foreign-trait associated type may be a boxed future."""
    match = ERASED.search(architecture)
    if match is None:
        failures.append(
            "docs/architecture.md no longer names the types whose associated "
            "type may be a boxed future (`those N are the only ones`), so every "
            "`Pin` in an associated type is reported"
        )
        return set()
    names = set(LINKED_NAME.findall(match.group(1)))
    if NUMBERS.get(match.group(2)) != len(names):
        failures.append(
            f"docs/architecture.md says {match.group(2)} types name a boxed "
            f"future and links {len(names)}: " + ", ".join(sorted(names))
        )
    return names


# --- Reading a listing --------------------------------------------------------

ROOT_SEGMENT = re.compile(r"(?<![\w:])([a-z_][a-z0-9_]*)::")
#: Roots that are no foreign crate: the standard library, these crates, and
#: primitives and keywords a path can open with.
NOT_FOREIGN = frozenset(
    "core alloc std kynos kynos_openapi kynos_macros self super crate "
    "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize "
    "f16 f32 f64 f128".split()
)
#: Crates whose types the listing names by the crate that defines them, which
#: is not the one Kynos depends on: rustdoc resolves a re-export to its source.
DEFINED_IN = {"serde": ("serde_core",), "tracing": ("tracing_core",)}
ITEM = re.compile(
    r"^pub (?:(?:async|const|unsafe|extern \S+) )*"
    r"(?:fn|type|struct|enum|union|trait|const|static|macro|mod|use) (.*)$"
)
PINNED = re.compile(r"\bPin<|\bBoxFuture\b")


def roots(line):
    """The foreign crates `line` names a path through, by listing spelling."""
    return {root for root in ROOT_SEGMENT.findall(line) if root not in NOT_FOREIGN}


def spelled(crate):
    """The path roots a listing may name `crate`'s types through."""
    return (crate.replace("-", "_"),) + DEFINED_IN.get(crate, ())


def strip_generics(text):
    """`text` with every `<…>` group removed; the `>` of `->` closes none."""
    out, depth, previous = "", 0, ""
    for char in text:
        if char == "<":
            depth += 1
        elif char == ">" and depth and previous != "-":
            depth -= 1
        elif not depth:
            out += char
        previous = char
    return out


def item_path(line):
    """The path an item line declares, generics stripped, or `None`."""
    match = ITEM.match(line)
    if match is None:
        return None
    rest = strip_generics(match.group(1))
    return re.split(r"[(:= ]", rest.replace("::", "\0"), maxsplit=1)[0].replace("\0", "::")


def impl_header(line):
    """`(trait, self type)` of an `impl` line, generics stripped; trait `None`
    for an inherent impl."""
    # The impl's own parameters go with the other generics, leaving
    # `Trait for Type` or `Type`.
    body = strip_generics(line[len("impl") :]).split(" where ")[0].strip()
    trait, sep, self_type = body.rpartition(" for ")
    if not sep:
        return None, body
    return trait.lstrip("!"), self_type


def surface_failures(crate, listing, runtime_items, boxed):
    """What `crate`'s listing exposes that Public API surface refuses.

    A `Pin` or `BoxFuture` is allowed only in a member of a foreign trait's
    impl: in a method, whose signature the trait fixes, or in an associated
    type of one of `boxed`. A `tokio` path is allowed only in an item whose
    path ends with one of `runtime_items`.
    """
    problems, trait, self_type = [], None, None
    for line in listing.splitlines():
        if line.startswith("impl"):
            trait, self_type = impl_header(line)
            member = None
        else:
            path = item_path(line)
            member = (
                path
                if path and self_type and path.startswith(self_type + "::")
                else None
            )
        foreign = trait is not None and trait.split("::")[0] not in NOT_FOREIGN
        if PINNED.search(line):
            fixed = member is not None and foreign and (
                line.startswith("pub fn ")
                or (line.startswith("pub type ") and self_type.rsplit("::", 1)[-1] in boxed)
            )
            if not fixed:
                problems.append(
                    f"{crate} exposes a `Pin` or `BoxFuture` that no foreign trait "
                    "fixes and that architecture.md's Public API surface does not "
                    f"name:\n    {line}"
                )
        if RUNTIME in roots(line):
            path = item_path(line) or ""
            if not any(path.endswith("::" + item) for item in runtime_items):
                problems.append(
                    f"{crate} names a {RUNTIME} type outside "
                    + ", ".join(f"`{item}`" for item in runtime_items)
                    + f", the only items architecture.md lets name one:\n    {line}"
                )
    return problems


def dependency_failures(named, allowed, awaiting, tiered):
    """What the foreign crates the listings name break of the documented sets.

    `named` maps each listing root to a line naming it; `allowed` and
    `awaiting` are the allowance and the crates outside it pending an issue;
    `tiered` is the graph's `{crate: tier}`.
    """
    problems, owner = [], {}
    for crate in [*allowed, *awaiting]:
        for root in spelled(crate):
            owner[root] = crate
    for root in sorted(set(named) - set(owner)):
        problems.append(
            f"`{root}` reaches a public item and is not in architecture.md's "
            "Public dependencies list, so an incompatible bump of it is a `!` "
            "release nobody chose. Keep its type out of the signature, or add "
            f"the crate to the list in the same change:\n    {named[root]}"
        )
    reached = {owner[root] for root in named if root in owner}
    for crate in allowed:
        if crate not in reached:
            problems.append(
                f"architecture.md's Public dependencies list allows `{crate}` and "
                "no public item names it. Drop it from the list, and move its "
                "graph row down a tier"
            )
    for crate in awaiting:
        if crate not in reached:
            problems.append(
                f"`{crate}` is listed as public API outside the allowance and no "
                "public item names it any more. Drop it from that paragraph, and "
                "move its graph row down a tier"
            )
    for crate, tier in sorted(tiered.items()):
        if tier == "public API" and crate not in owner.values():
            problems.append(
                f"the graph tiers `{crate}` public API and architecture.md lists "
                "it neither as a public dependency nor as one awaiting its issue"
            )
        if tier != "public API" and crate in reached:
            problems.append(
                f"the graph tiers `{crate}` {tier}, and a public item names it, "
                "so an upstream fix reaches a signature. Tier it public API"
            )
    return problems


# --- The listings -------------------------------------------------------------


def listed(crate, toolchain):
    """`cargo public-api`'s listing of `crate`, or the failure it ended in."""
    command = ["cargo", f"+{toolchain}", "public-api", "-p", crate, *LISTING_ARGS]
    run = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if run.returncode != 0:
        sys.stderr.write(run.stderr)
        return None, f"`{' '.join(command)}` exited {run.returncode}"
    return run.stdout, None


def snapshot_failures(crate, recorded, listing):
    """The difference between a crate's committed snapshot and its listing."""
    if recorded == listing:
        return []
    where = f"crates/{crate}/{SNAPSHOT}"
    if recorded is None:
        return [f"{where} is not there. Record it with `mise run api:record`"]
    diff = difflib.unified_diff(
        recorded.splitlines(), listing.splitlines(), where, "listed", lineterm="", n=0
    )
    return [
        f"{crate}'s public API is not the one {where} records. Review the "
        "difference below, then record it with `mise run api:record` in the "
        "same change:\n" + "\n".join(diff)
    ]


def main(architecture=None, listings=None, recorded=None, overwrite=None, root=ROOT):
    """Run every rule, and report what does not hold.

    Each argument defaults to this repository's own, read or listed when
    `main` runs: `architecture` as text, `listings` and `recorded` as
    `{crate: text}` (`None` for a missing snapshot). Snapshots are read from
    and overwritten under `root`.
    """
    failures = []
    if architecture is None:
        architecture = ARCHITECTURE.read_text()
    if overwrite is None:
        overwrite = os.environ.get("KYNOS_PUBLIC_API") == "overwrite"
    if listings is None:
        toolchain = os.environ.get("KYNOS_NIGHTLY")
        if not toolchain:
            print("public-api: KYNOS_NIGHTLY is unset; run this through mise", file=sys.stderr)
            return 1
        listings = {}
        # Idempotent; the task installed the toolchain, this adds the target.
        subprocess.run(
            ["rustup", "target", "add", "--toolchain", toolchain, TARGET],
            cwd=ROOT,
            check=True,
            capture_output=True,
        )
        for crate in CRATES:
            listing, failure = listed(crate, toolchain)
            if failure:
                failures.append(failure)
            else:
                listings[crate] = listing
    if recorded is None:
        recorded = {}
        for crate in listings:
            path = root / "crates" / crate / SNAPSHOT
            recorded[crate] = path.read_text() if path.is_file() else None

    allowed = allowance(architecture, failures)
    awaiting = pending(architecture, failures)
    tiered = tiers(architecture, failures)
    boxed = erased(architecture, failures)
    runtime_items = allowed.get(RUNTIME, [])

    named = {}
    for crate, listing in listings.items():
        if overwrite:
            (root / "crates" / crate / SNAPSHOT).write_text(listing)
        else:
            failures += snapshot_failures(crate, recorded.get(crate), listing)
        failures += surface_failures(crate, listing, runtime_items, boxed)
        for line in listing.splitlines():
            for foreign in roots(line):
                named.setdefault(foreign, line)
    if listings:
        failures += dependency_failures(named, allowed, awaiting, tiered)

    for failure in failures:
        print(f"public-api: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(
        f"public-api: {len(listings)} crates, {len(named)} foreign crates named, "
        "every rule holds"
    )
    return 0


# Importing runs no rule; `public_api_test.py` relies on that.
if __name__ == "__main__":
    sys.exit(main())
