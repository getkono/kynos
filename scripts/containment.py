"""Containment checks: a crate may be named only where its owner allows.

`docs/architecture.md` states the allowances and says twice that the count
is the check. This is that check. It reads the tables rather than restating
them, so the document and the gate cannot drift apart.

Source is stripped of comments and string literals before anything is
matched -- these rules are discussed in prose throughout `src`, and the
`b"h2"` ALPN identifier is a literal -- and `#[cfg(test)]` modules are
dropped, both inline and as sibling files.
"""

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

# --- Report -----------------------------------------------------------------
for failure in failures:
    print(f"containment: {failure}", file=sys.stderr)
if failures:
    sys.exit(1)
print(f"containment: {len(FILES)} source files, {len(rows)} allowance rows, every rule holds")
