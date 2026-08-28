"""Containment checks: a crate may be named only where its owner allows.

`docs/architecture.md` states the allowances and says twice that the count
is the check. This is that check. It reads the tables rather than restating
them, so the document and the gate cannot drift apart.

Source is stripped of comments and string literals before anything is
matched -- these rules are discussed in prose throughout `src`, and the
`b"h2"` ALPN identifier is a literal -- and `#[cfg(test)]` modules are
dropped, both inline and as sibling files.
"""

import os
import re
import sys
from pathlib import Path

# From the script's own location rather than the working directory, so that
# running it by hand from a crate directory checks the same tree mise does.
ROOT = Path(__file__).resolve().parent.parent
ARCHITECTURE = (ROOT / "docs/architecture.md").read_text()
NUMBERS = {"Three": 3, "Four": 4, "Five": 5, "Six": 6, "Seven": 7, "Eight": 8}
failures = []


CHAR_LITERAL = re.compile(r"'(\\.|[^\\'])'")


def strip(source):
    """Drop comments, literals and `#[cfg(test)]` modules.

    A hand-rolled scanner rather than a set of regexes, because the three
    constructs nest: `"//"` is not a comment, `'"'` is not a string, and
    Rust's block comments nest inside each other. Getting any of those
    wrong desynchronises the scan and silently inverts what survives.
    """
    out, i, n = [], 0, len(source)
    while i < n:
        pair = source[i : i + 2]
        if pair == "//":
            j = source.find("\n", i)
            i = n if j < 0 else j
        elif pair == "/*":
            depth, i = 0, i
            while i < n:
                if source[i : i + 2] == "/*":
                    depth += 1
                    i += 2
                elif source[i : i + 2] == "*/":
                    depth -= 1
                    i += 2
                    if depth == 0:
                        break
                else:
                    i += 1
        elif source[i] == "r" and (m := re.match(r'r(#*)"', source[i:])):
            close = '"' + m.group(1)
            j = source.find(close, i + m.end())
            i = n if j < 0 else j + len(close)
        elif source[i] == '"':
            i += 1
            while i < n and source[i] != '"':
                i += 2 if source[i] == "\\" else 1
            i += 1
        elif source[i] == "'" and CHAR_LITERAL.match(source, i):
            # A char literal. A bare `'` that does not close is a lifetime,
            # which is ordinary code and falls through to the branch below.
            i = CHAR_LITERAL.match(source, i).end()
        else:
            out.append(source[i])
            i += 1
    text = "".join(out)

    while (m := re.search(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*", text)) is not None:
        rest = text[m.end() :]
        if not rest.startswith("{"):
            text = text[: m.start()] + rest.removeprefix(";")
            continue
        depth = 0
        for k, char in enumerate(rest):
            depth += (char == "{") - (char == "}")
            if depth == 0:
                break
        text = text[: m.start()] + rest[k + 1 :]
    return text


FILES = [
    (path.relative_to(ROOT).as_posix(), strip(path.read_text()))
    for crate in sorted((ROOT / "crates").iterdir())
    if (crate / "src").is_dir()
    for path in sorted((crate / "src").rglob("*.rs"))
    if path.name != "tests.rs" and not path.name.endswith("_tests.rs")
]


def naming(*crates):
    """The files naming any of `crates` as an identifier."""
    pattern = re.compile(r"\b(" + "|".join(crates) + r")\b")
    return {path for path, text in FILES if pattern.search(text)}


def claimed(sentence):
    """The number `architecture.md` writes into one of its count claims."""
    found = re.search(sentence, ARCHITECTURE)
    if found is None:
        failures.append(
            f"architecture.md no longer states a count matching /{sentence}/, so "
            "the claim this gate exists to hold is gone"
        )
        return None
    word = found.group(1)
    number = NUMBERS.get(word.capitalize())
    if number is None:
        # Loudly, rather than skipping the check: a count written in a word
        # this gate cannot read is a count nothing is holding.
        failures.append(f"architecture.md writes an unreadable count: {word!r}")
    return number


def expand(entry):
    """`server/{accept,mod}.rs` -> `server/accept.rs`, `server/mod.rs`."""
    brace = re.search(r"\{([^}]*)\}", entry)
    if brace is None:
        return [entry]
    return [
        entry[: brace.start()] + part.strip() + entry[brace.end() :]
        for part in brace.group(1).split(",")
    ]


# --- The runtime allowance table -------------------------------------------
table = ARCHITECTURE[ARCHITECTURE.index("| Site | Names | Why it is not in `server/` |") :]
rows = []
for line in table.split("\n")[2:]:
    if not line.startswith("|"):
        break
    rows.append(re.findall(r"`([^`]+)`", line.split("|")[1]))

stated = claimed(r"\*\*(\w+) rows, and the count is the check\.\*\*")
if stated is not None and stated != len(rows):
    failures.append(
        f"architecture.md's allowance table claims {stated} rows and has {len(rows)}"
    )

allowed = {f"crates/kynos/src/{site.lstrip('/')}" for row in rows for entry in row for site in expand(entry)}


def permitted(path):
    if path.startswith("crates/kynos/src/server/"):
        return True
    return any(path == site or path.startswith(site.rstrip("/") + "/") for site in allowed)


if offenders := sorted(p for p in naming("tokio") if not permitted(p)):
    failures.append(
        "`tokio` is named outside `server/` at a site the allowance table does "
        "not list:\n    " + "\n    ".join(offenders)
    )

# --- The dependency graph ---------------------------------------------------
UNDER = "under"
ONLY_IN = "only in"
for crates, rule, where, description in [
    (("hyper", "hyper_util"), ONLY_IN,
     {"crates/kynos/src/server/connection.rs", "crates/kynos/src/http/body.rs"},
     "`hyper` and `hyper-util` are named only in `server/connection.rs` and `http/body.rs`"),
    (("rustls", "tokio_rustls"), UNDER, "crates/kynos/src/server/tls/",
     "`tokio-rustls` and `rustls` are named only under `server/tls/`"),
    (("matchit",), UNDER, "crates/kynos/src/router/",
     "`matchit` may be named only under `router/`"),
    (("h2", "httparse"), ONLY_IN, set(), "`h2` and `httparse` are never named"),
    (("tower", "tower_service"), ONLY_IN, {"crates/kynos/src/unchecked.rs"},
     "`tower` and `tower-service` are named only in `unchecked.rs`"),
]:
    found = naming(*crates)
    stray = sorted(f for f in found if not f.startswith(where)) if rule == UNDER else sorted(found - where)
    if stray:
        failures.append(f"{description}, but it is also named in:\n    " + "\n    ".join(stray))

# --- Hand-rolled `Stream` implementations -----------------------------------
# Only the section that enumerates them. Collecting every link in the
# document would let an unrelated mention anywhere else silently authorise a
# new hand-rolled `Stream`.
surface = ARCHITECTURE[ARCHITECTURE.index("### Public API surface") :]
surface = surface[: surface.index("\n## ")]
declared = set(re.findall(r"\]\(\.\./(crates/[^)]+\.rs)\)", surface))
hand_rolled = {path for path, text in FILES if re.search(r"\bStream\s+for\b", text)}

sites = claimed(r"\*\*One public row, (\w+) sites, and the count is the check\*\*")
if sites is not None and sites != len(hand_rolled):
    failures.append(
        f"architecture.md claims {sites} hand-rolled `Stream` sites and there are "
        f"{len(hand_rolled)}:\n    " + "\n    ".join(sorted(hand_rolled))
    )
if undeclared := sorted(hand_rolled - declared):
    failures.append(
        "a hand-rolled `Stream` sits where architecture.md names no site:\n    "
        + "\n    ".join(undeclared)
    )

# --- The module-size budget -------------------------------------------------
# AGENTS.md: a module becomes a directory once it exceeds ~400 lines excluding
# tests. Files still over that line are a debt, and `nfr.md` writes down how
# many there are so the number can only move deliberately -- down as one is
# split, and never up without someone editing the ledger and saying why.
NFR = (ROOT / "docs/nfr.md").read_text()
oversized = sorted(
    path
    for path, _ in FILES
    if len((ROOT / path).read_text().split("\n")) > 400
)

budget = re.search(r"a module-size budget of (\d+) files", NFR)
if budget is None:
    failures.append("nfr.md no longer states the module-size budget")
elif int(budget.group(1)) != len(oversized):
    failures.append(
        f"nfr.md budgets {budget.group(1)} files over ~400 lines and there are "
        f"{len(oversized)}. Splitting one means lowering the budget in the same "
        "commit; adding one means arguing for it there.\n    "
        + "\n    ".join(oversized)
    )

# --- Nothing a package compiles reaches outside the package ------------------
# `cargo package` copies a package directory and nothing above it, so a path
# literal that climbs out of one names a file the archive cannot carry. Two
# distinct faults of exactly that shape had already reached the tree, and no
# gate could see either: every other task builds the working tree, where both
# paths resolve, and `cargo package`'s own verify step does not build test
# targets.
#
# Read from the raw source rather than from `FILES`, whose text has had its
# string literals stripped, and over every `.rs` file in each package rather
# than `src/` alone -- a test target is published too.
# Resolved against the reading file: `include_bytes!` and `include_str!` take a
# path relative to the source that names them.
INCLUDED = re.compile(r'include_(?:bytes|str)!\s*\(\s*"([^"]*)"')
# Resolved against the package: the two spellings of building a path from the
# manifest directory. Only literals attached directly to it are read -- a path
# assembled through a variable is beyond a source scan, and every site in this
# workspace is one expression.
CONCATENATED = re.compile(r'CARGO_MANIFEST_DIR"\s*\)\s*,\s*"([^"]*)"')
JOINED = re.compile(r'CARGO_MANIFEST_DIR"\s*\)\s*\)?((?:\s*\.join\(\s*"[^"]*"\s*\))+)')
JOIN = re.compile(r'\.join\(\s*"([^"]*)"\s*\)')
# The manifest's own `exclude`, which is what says a file never reaches an
# archive. A target excluded there is exempt by construction: the rule is that
# nothing *published* reaches out, and the exemption is the manifest's to grant
# and to explain.
EXCLUDED = re.compile(r"^exclude\s*=\s*\[([^\]]*)\]", re.M)


def published(package):
    """Every `.rs` file in `package` that a published archive would carry."""
    manifest = EXCLUDED.search((package / "Cargo.toml").read_text())
    exempt = re.findall(r'"([^"]*)"', manifest.group(1)) if manifest else []
    for source in sorted(package.rglob("*.rs")):
        relative = source.relative_to(package).as_posix()
        if "target" in source.relative_to(package).parts:
            continue
        if any(relative == entry or relative.startswith(entry.rstrip("/") + "/") for entry in exempt):
            continue
        yield source


for package in sorted((ROOT / "crates").iterdir()):
    if not (package / "Cargo.toml").is_file():
        continue
    for source in published(package):
        text = source.read_text()
        reached = (
            [(source.parent, literal) for literal in INCLUDED.findall(text)]
            + [(package, literal.lstrip("/")) for literal in CONCATENATED.findall(text)]
            + [(package, "/".join(JOIN.findall(chain))) for chain in JOINED.findall(text)]
        )
        for base, literal in reached:
            if Path(os.path.normpath(base / literal)).is_relative_to(package):
                continue
            failures.append(
                f"{source.relative_to(ROOT).as_posix()} reads {literal!r}, which "
                f"resolves outside {package.relative_to(ROOT).as_posix()}. A "
                "published archive carries the package directory and nothing "
                "above it, so this names a file the archive cannot hold: either "
                "keep what it reads inside the package, or `exclude` the target "
                "and say in the manifest why the assertion is the repository's"
            )

# --- Report -----------------------------------------------------------------
for failure in failures:
    print(f"containment: {failure}", file=sys.stderr)
if failures:
    sys.exit(1)
print(f"containment: {len(FILES)} source files, {len(rows)} allowance rows, every rule holds")
