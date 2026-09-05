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
import tomllib
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

# --- The off-path elements ---------------------------------------------------
# `performance.md` grades the document model, the emitters, the validators and
# `describe` as off-path elements, and an off-path element owes a proof that a
# request cannot reach it rather than a measurement. This is that proof's outer
# half, and `testing.md#the-off-path-proof` is where it is argued.
#
# Stated negatively, because a request path is not a set of files: the table
# names each element with the sites allowed to name it, and every other file
# under the scope is on the request path by default. So the rule needs no list
# of what serves a request -- which is the list nobody could keep true -- and a
# new site is a failing build until someone writes a row saying why a request
# cannot reach it.
#
# The declared side is read off disk, as it is everywhere else here: `naming()`
# computes the real set, and the only hand-written thing in a row is the reason,
# which is quoted back in the failure. Subset semantics, like the `ONLY_IN`
# rules above: a site that has stopped naming its element is stale rather than
# wrong, and only a stray fails.
#
# The inner half is not here and cannot be. `Dispatch` hands every request to a
# trait object, and what sits behind one is declared in a file this rule reads
# as an allowed site; `router/dispatch/tests.rs` closes that from the other side
# by destructuring the three types a request travels through.
TESTING = (ROOT / "docs/testing.md").read_text()
OFF_PATH_HEADER = "| Element | Named by | Named only in | Why a request cannot reach it |"
# The one scope a row's sites are resolved against. An element whose home is
# another crate means changing this, not the row: a site outside the scope would
# otherwise be compared against files the loop never looks at, and pass.
OFF_PATH_SCOPE = "crates/kynos/src/"


# What a *Named by* cell may hold: an identifier, or a path of them. The cell is
# prose that happens to be code, so the backticks around it are optional here
# rather than load-bearing.
NAMED_BY = re.compile(r"`?(\w+(?:\s*::\s*\w+)*)`?")


def token(cell):
    """A regex for what one *Named by* cell names, or `None` if unreadable.

    `None` loudly rather than a pattern that cannot match: a cell this function
    guesses at compiles to an escaped literal nothing in Rust source contains,
    and a rule that always passes reports that the elements are off the path
    when nobody has checked. A new kind of token belongs in `NAMED_BY` and here,
    not in a fallback.

    `Registry::new` is a path rather than an identifier, and the source may
    write it spaced or wrapped, so each `::` matches the whitespace a formatter
    is free to put around it.
    """
    readable = NAMED_BY.fullmatch(cell.strip())
    if readable is None:
        return None
    segments = [re.escape(part.strip()) for part in readable.group(1).split("::")]
    return re.compile(r"\b" + r"\s*::\s*".join(segments) + r"\b")


def sites(cell):
    """The files one *Named only in* cell allows, resolved against the scope.

    A comma-separated list of backticked paths, each of which may brace-expand:
    a row naming nine files is one cell, and `router/{describe,install}.rs` is
    the same shorthand the allowance table above uses.
    """
    entries = re.findall(r"`([^`]+)`", cell) or [part for part in cell.split(",") if part.strip()]
    return {
        OFF_PATH_SCOPE + site.strip().lstrip("/")
        for entry in entries
        for site in expand(entry.strip())
    }


halves = TESTING.split(OFF_PATH_HEADER)
if len(halves) != 2:
    failures.append(
        "testing.md no longer holds exactly one off-path table under the header "
        "this rule reads, so nothing states which elements a request may not "
        "reach"
    )

off_path_rows = 0
for line in (halves[1] if len(halves) == 2 else "").split("\n")[2:]:
    if not line.startswith("|"):
        break
    # `strip("|")` before the split, so the outer pipes do not yield two empty
    # cells and shift every column by one.
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    if len(cells) != 4:
        failures.append(f"testing.md's off-path table has a malformed row: {line.strip()}")
        continue

    element, named_by, where, reason = cells
    off_path_rows += 1
    allowance = sites(where)
    pattern = token(named_by)
    if pattern is None:
        failures.append(
            f"testing.md's off-path table names {element} with {named_by}, "
            "which this rule cannot read as an identifier or a path of them. "
            "The row holds nothing until it can: teach the rule the token, or "
            "write one it already knows"
        )
        continue

    offenders = sorted(
        path
        for path, text in FILES
        if path.startswith(OFF_PATH_SCOPE) and path not in allowance and pattern.search(text)
    )
    if offenders:
        failures.append(
            f"{element} is off the request path, and {named_by} is named at a "
            "site testing.md's off-path table does not allow. The row says a "
            f"request cannot reach it because {reason}. Either that reason "
            "covers the site below and the row should say so, or a request can "
            "now reach it:\n    " + "\n    ".join(offenders)
        )

if len(halves) == 2 and not off_path_rows:
    failures.append(
        "testing.md's off-path table has no rows, so it holds nothing. An "
        "element that stopped being off-path is retired by arguing it in "
        "performance.md's allocation, not by emptying the table"
    )

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
#
# "Excluding tests" is satisfied by the layout rule rather than by parsing:
# `FILES` already drops every `tests.rs`, and the convention puts a module's
# tests in one. A module keeping an inline `mod tests` would have those lines
# counted, which is the right pressure -- the same rule says to move them out.
#
# Counted with `count("\n")` rather than `len(split("\n"))`: every file here
# ends in a newline, so splitting yields one empty trailing element and a file
# of exactly 400 lines would be read as 401 and reported as past a line it has
# not passed.
NFR = (ROOT / "docs/nfr.md").read_text()
oversized = sorted(
    path
    for path, _ in FILES
    if (ROOT / path).read_text().count("\n") > 400
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

# --- The feature grading -----------------------------------------------------
# `docs/performance.md` grades every flag `crates/kynos` declares, and the grade
# decides what the flag owes: a full battery, an off-path proof, or nothing of
# its own because it is an aggregate. A flag nobody graded is the exact failure
# the table exists to make visible, and until this rule it failed nothing.
#
# Both sides are read off disk -- the flags from the table, the keys from the
# manifest -- so neither is transcribed here and no count is stated in either.
# `testing.md#cross-cutting` is the argument: a count reports that two numbers
# differ where a set names the flag nothing accounts for, and a count puts two
# branches each adding a flag on the same line where the table puts them on
# different ones.
#
# The manifest is parsed rather than scanned. The failure `strip()`'s docstring
# names applies here too: a regex over `^([\w-]+)\s*=` drops a quoted key, and a
# dropped key is a flag falling silently out of the compared set. A rule whose
# whole purpose is catching the flag nobody noticed cannot rest on a parser that
# can lose one.
PERFORMANCE = (ROOT / "docs/performance.md").read_text()
grading = PERFORMANCE[PERFORMANCE.index("| Grade | Owes | Flags |") :]
graded = []
for line in grading.split("\n")[2:]:
    if not line.startswith("|"):
        break
    graded += re.findall(r"`([^`]+)`", line.split("|")[3])

manifest = tomllib.loads((ROOT / "crates/kynos/Cargo.toml").read_text())
flags = set(manifest["features"])
depended = {
    member[len("dep:") :]
    for members in manifest["features"].values()
    for member in members
    if member.startswith("dep:")
}

if ungraded := sorted(flags - set(graded)):
    failures.append(
        "crates/kynos declares a feature that performance.md's grading table "
        "does not grade. Grading it is the argument the table exists to force: "
        "a full battery, an off-path proof, or an aggregate that owes nothing "
        "of its own:\n    " + "\n    ".join(ungraded)
    )

if undeclared := sorted(set(graded) - flags):
    failures.append(
        "performance.md grades a flag that crates/kynos does not declare, so "
        "the row names a battery nothing can be enabled to owe. Either the flag "
        "was renamed and the row was not, or the row outlived the feature:\n    "
        + "\n    ".join(undeclared)
    )

if regraded := sorted({flag for flag in graded if graded.count(flag) > 1}):
    failures.append(
        "performance.md grades a flag in more than one row, where the table "
        "says every flag appears in exactly one column. Two grades are two "
        "different batteries owed and nothing decides between them:\n    "
        + "\n    ".join(regraded)
    )

# An optional dependency no feature names with `dep:` makes Cargo synthesise an
# implicit feature for it: a flag the crate declares, absent from `[features]`,
# and so invisible to the three comparisons above. Its own failure rather than a
# fourth entry in `ungraded`, because the remedy differs -- write the `dep:`, do
# not add a table row.
if implicit := sorted(
    name
    for name, spec in manifest["dependencies"].items()
    if isinstance(spec, dict) and spec.get("optional") and name not in depended
):
    failures.append(
        "an optional dependency of crates/kynos is named by no `dep:`, so Cargo "
        "synthesises a feature for it that `[features]` does not list and this "
        "rule cannot count against the grading. Name it from the feature that "
        "needs it as `dep:`:\n    " + "\n    ".join(implicit)
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

# --- Parent re-exports ------------------------------------------------------
# AGENTS.md: submodules are `pub` with no parent re-exports, so every item has
# one canonical path, and the crate root and `kynos::prelude` are the only
# curated shortcuts. Both of those live in `lib.rs`, which is why it is the one
# file exempt rather than a list of names.
#
# What is refused is a *second* path to one of our own items: a `pub use` naming
# `crate`, `self`, `super`, or a module the same file declares. Re-exporting a
# foreign crate is a facade rather than a second path -- `http/mod.rs`
# republishes `http::HeaderMap`, which does not thereby acquire a Kynos path at
# all -- so the rule is written against where the path leads, not against an
# allowlist of files that would need editing every time one moved.
#
# `pub(crate) use` is left alone. The rule is about the paths a *user* can write,
# and a crate-visible alias is not one of them.
DECLARED_MODULE = re.compile(r"\bmod\s+(\w+)\s*[;{]")
REEXPORT = re.compile(r"^[ \t]*pub\s+use\s+(\w+)", re.MULTILINE)

reexports = []
for path, text in FILES:
    if path.endswith("/lib.rs"):
        continue
    own = set(DECLARED_MODULE.findall(text)) | {"crate", "self", "super"}
    reexports += [
        f"{path}: pub use {head}::..."
        for head in REEXPORT.findall(text)
        if head in own
    ]

if reexports:
    failures.append(
        "a `pub use` re-publishes one of our own items, giving it a second path "
        "where the layout rule allows exactly one:\n    " + "\n    ".join(sorted(reexports))
    )

# --- Placeholder bodies -----------------------------------------------------
# AGENTS.md permits a `todo!()` body only during the pre-v1 API-skeleton
# milestone, where the surface is designed ahead of its implementation so it can
# be reviewed and frozen as a whole, and says the exception lapses once the
# skeleton is frozen. `docs/testing.md` records that it has -- "the API-skeleton
# milestone is over, the bodies landed, and what it deferred has been paid" --
# and until now nothing held the lapse. That is the failure a spent exception
# has: it stops being argued for and quietly stays available.
#
# No allowlist is needed, which is the whole reason this rule is cheap. Every
# `todo!()` in the tree is inside a doc example, standing in for an application's
# own code, and `strip()` has already removed doc comments by the time this runs.
# The word boundary keeps the rule off a macro that merely ends in `todo!`.
PLACEHOLDER = re.compile(r"\btodo!")

if placeholders := sorted(path for path, text in FILES if PLACEHOLDER.search(text)):
    failures.append(
        "a `todo!()` stands in for a body, and the exception that allowed one "
        "lapsed when the API-skeleton milestone ended:\n    " + "\n    ".join(placeholders)
    )

# --- Report -----------------------------------------------------------------
for failure in failures:
    print(f"containment: {failure}", file=sys.stderr)
if failures:
    sys.exit(1)
print(
    f"containment: {len(FILES)} source files, {len(rows)} allowance rows, "
    f"{off_path_rows} off-path rows, {len(graded)} graded features, every rule holds"
)
