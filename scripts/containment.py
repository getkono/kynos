"""Containment checks: a crate may be named only where its owner allows.

Reads the allowance tables in `docs/` rather than restating them. Source is
matched with comments, string literals and `#[cfg(test)]` modules (inline and
sibling files) removed; feature-gate rules use a second corpus that keeps
literals, since a `#[cfg(feature = "x")]` flag name is a string.
"""

import ast
import os
import re
import sys
import tomllib
from pathlib import Path

# Relative to the script, so a run from any directory checks the same tree.
ROOT = Path(__file__).resolve().parent.parent
ARCHITECTURE = (ROOT / "docs/architecture.md").read_text()
# Counts a document may state in words; an unlisted word fails loudly.
NUMBERS = {
    "Three": 3,
    "Four": 4,
    "Five": 5,
    "Six": 6,
    "Seven": 7,
    "Eight": 8,
    "Nine": 9,
    "Ten": 10,
    "Eleven": 11,
    "Twelve": 12,
    "Thirteen": 13,
    "Fourteen": 14,
    "Fifteen": 15,
    "Sixteen": 16,
}


CHAR_LITERAL = re.compile(r"'(\\.|[^\\'])'")
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*")


def blank(text):
    """`text` with every character but its newlines replaced by a space.

    Keeps both corpora the same length, so one span applies to both.
    """
    return "".join("\n" if char == "\n" else " " for char in text)


def literal_end(source, i):
    """Where the literal starting at `i` ends, or `None` if none starts there.

    Raw strings, ordinary strings and char literals; an unclosed `'` is a
    lifetime, not a literal.
    """
    if source[i] == "r" and (m := re.match(r'r(#*)"', source[i:])):
        close = '"' + m.group(1)
        j = source.find(close, i + m.end())
        return len(source) if j < 0 else j + len(close)
    if source[i] == '"':
        j, n = i + 1, len(source)
        while j < n and source[j] != '"':
            j += 2 if source[j] == "\\" else 1
        return j + 1
    if m := CHAR_LITERAL.match(source, i):
        return m.end()
    return None


def test_module_spans(text):
    """Every inline `#[cfg(test)] mod` in `text`, as `(start, end)` offsets.

    `text` must have its literals blanked, so a brace in a string is not
    counted. Outermost spans only.
    """
    spans = []
    for match in TEST_MODULE.finditer(text):
        if spans and match.start() < spans[-1][1]:
            continue
        rest = text[match.end() :]
        if not rest.startswith("{"):
            # `mod tests;`: `under_test` drops the sibling file.
            spans.append((match.start(), match.end() + rest.startswith(";")))
            continue
        depth, end = 0, len(text)
        for k, char in enumerate(rest):
            depth += (char == "{") - (char == "}")
            if depth == 0:
                end = match.end() + k + 1
                break
        # Unbalanced braces: the module runs to the end of the file.
        spans.append((match.start(), end))
    return spans


def strip(source, literals=True):
    """Drop comments and `#[cfg(test)]` modules, and literals unless asked.

    A scanner rather than regexes, since comments, strings and nested block
    comments interleave. `literals=False` keeps literals: the corpus a
    `#[cfg(feature = "x")]` gate is read against.
    """
    masked, kept, i, n = [], [], 0, len(source)
    while i < n:
        pair = source[i : i + 2]
        if pair == "//":
            j = source.find("\n", i)
            i = n if j < 0 else j
        elif pair == "/*":
            depth = 0
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
        elif (end := literal_end(source, i)) is not None:
            literal = source[i:end]
            masked.append(blank(literal))
            kept.append(literal)
            i = end
        else:
            masked.append(source[i])
            kept.append(source[i])
            i += 1
    masked, kept = "".join(masked), "".join(kept)

    text = kept if not literals else masked
    for start, end in reversed(test_module_spans(masked)):
        text = text[:start] + text[end:]
    return text


RAW_SOURCES = [
    (path.relative_to(ROOT).as_posix(), path.read_text())
    for crate in sorted((ROOT / "crates").iterdir())
    if (crate / "src").is_dir()
    for path in sorted((crate / "src").rglob("*.rs"))
]

def under_test(path):
    name = path.rsplit("/", 1)[-1]
    return name == "tests.rs" or name.endswith("_tests.rs")


def substituted(pairs, path, text):
    """`pairs` with `path` reading as `text`, appended if it is not one of them."""
    replaced = [(name, text if name == path else body) for name, body in pairs]
    if any(name == path for name, _ in pairs):
        return replaced
    return replaced + [(path, text)]


class Corpus:
    """The tree of `.rs` files every rule stated over the source is stated over.

    * `files`: the code a request can run; test modules and sibling test
      files dropped.
    * `sources`: `files` plus sibling test files; what a spelling's existence
      is asked of.
    * `gate_files`, `gate_sources`: those two with string literals kept, for
      feature-gate rules.
    * `raw`: each file's unstripped text, keyed by path.

    Passed to `main` so a test can hand it a `replacing` corpus.
    """

    def __init__(self, raw, sources=None, gate_sources=None):
        """`raw` is `(path, text)` pairs, or anything `dict` accepts.

        `sources` and `gate_sources` default to stripping `raw`.
        """
        self.raw = dict(raw)
        pairs = list(self.raw.items())
        if sources is None:
            sources = [(path, strip(text)) for path, text in pairs]
        if gate_sources is None:
            gate_sources = [(path, strip(text, literals=False)) for path, text in pairs]
        self.sources = sources
        self.gate_sources = gate_sources
        self.files = [(path, text) for path, text in sources if not under_test(path)]
        self.gate_files = [(path, text) for path, text in gate_sources if not under_test(path)]

    def naming(self, *crates):
        """The files naming any of `crates` as an identifier."""
        pattern = re.compile(r"\b(" + "|".join(crates) + r")\b")
        return {path for path, text in self.files if pattern.search(text)}

    def replacing(self, path, text):
        """This corpus with `path` reading as `text`, and nothing else stripped again.

        A path the corpus does not hold is appended.
        """
        return Corpus(
            self.raw | {path: text},
            substituted(self.sources, path, strip(text)),
            substituted(self.gate_sources, path, strip(text, literals=False)),
        )


#: This repository's tree; `main`'s default corpus. Importing runs no rule.
WORKSPACE = Corpus(RAW_SOURCES)

# Which corpus a failure says a spelling was looked for in, keyed by is-a-gate.
LOOKED_IN = {
    False: "with comments, string literals and inline `#[cfg(test)]` modules removed",
    True: "with comments and inline `#[cfg(test)]` modules removed and string literals kept",
}


#: The row-count sentence over `architecture.md`'s allowance table and
#: `testing.md`'s off-path table.
ROW_COUNT_CLAIM = r"\*\*(\w+) rows, and the count is the check\.\*\*"


def claimed(text, sentence, failures):
    """The number `text` writes into the count claim `sentence`, or `None`.

    A missing or unreadable count is appended to `failures`.
    """
    found = re.search(sentence, text)
    if found is None:
        failures.append(
            f"architecture.md no longer states a count matching /{sentence}/, so "
            "the claim this gate exists to hold is gone"
        )
        return None
    word = found.group(1)
    number = NUMBERS.get(word.capitalize())
    if number is None:
        failures.append(f"architecture.md writes an unreadable count: {word!r}")
    return number


def section(text, start, failures, end=None, unrun=""):
    """`text` from `start` to `end`, or `None` with the missing marker reported.

    First occurrence of each marker; `end` is searched after `start`, and a
    missing `end` fails rather than widening to the end of the document.
    `unrun` completes the failure message, naming the rule left unchecked.
    """
    begin = text.find(start)
    if begin < 0:
        failures.append(
            f"this gate slices a document at {start!r} and no longer finds it, "
            f"so {unrun}"
        )
        return None
    if end is None:
        return text[begin:]
    stop = text.find(end, begin)
    if stop < 0:
        failures.append(
            f"this gate ends a slice at {end!r} and no longer finds it, so {unrun}"
        )
        return None
    return text[begin:stop]


def expand(entry):
    """`server/{accept,mod}.rs` -> `server/accept.rs`, `server/mod.rs`."""
    brace = re.search(r"\{([^}]*)\}", entry)
    if brace is None:
        return [entry]
    return [
        entry[: brace.start()] + part.strip() + entry[brace.end() :]
        for part in brace.group(1).split(",")
    ]


def permitted(path, allowed):
    """Whether the `allowed` sites let `path` name tokio outside `server/`.

    Skip the rule rather than pass an empty `allowed` when its table is gone.
    """
    if path.startswith("crates/kynos/src/server/"):
        return True
    return any(path == site or path.startswith(site.rstrip("/") + "/") for site in allowed)


# --- The dependency graph ---------------------------------------------------
UNDER = "under"
ONLY_IN = "only in"


def listed(path, sites):
    """Whether `path` is one of an `ONLY_IN` row's `sites`.

    A site ending in `/` is a tree and holds every file under it; any other
    site is one file, matched exactly.
    """
    return any(path == site or (site.endswith("/") and path.startswith(site)) for site in sites)


# --- The off-path elements ---------------------------------------------------
# The outer half of the off-path proof (`testing.md#the-off-path-proof`): every
# file naming an element outside its row's sites is on the request path and
# fails. A site that stopped naming its element is stale, not a failure.
TESTING = (ROOT / "docs/testing.md").read_text()
OFF_PATH_HEADER = "| Element | Named by | Named only in | Why a request cannot reach it |"
# Bare sites are relative to this; a `crates/...` site widens its row's scope.
OFF_PATH_SCOPE = "crates/kynos/src/"


# A *Named by* spelling: an identifier or `::` path. Backticks separate several
# spellings in one cell and are optional for a single bare token.
NAMED_BY = re.compile(r"`?(\w+(?:\s*::\s*\w+)*)`?")
# A feature-gate spelling, at either polarity: `feature = "x"` names what the
# flag compiles when on, `not(feature = "x")` what it compiles when off.
# `Gate.search` (existence) reads the polarity; `Gate.named` (offenders) does not.
GATE = re.compile(r'`?feature\s*=\s*"([\w-]+)"`?')
NEGATED_GATE = re.compile(r'`?not\(\s*feature\s*=\s*"([\w-]+)"\s*\)`?')
# Where a `cfg` predicate starts: `#[cfg(`, `#![cfg(`, `cfg_attr` likewise, or
# a bare `cfg!` under any of its three delimiters (attributes accept only `(`).
# The lookbehind refuses `mycfg!`, `path::cfg!` and `$cfg!`, which text cannot
# resolve to the real macro.
PREDICATE = re.compile(r"#!?\[\s*(cfg_attr|cfg)\s*\(|(?<![\w:$])cfg!\s*([({\[])")
#: What closes a predicate's outermost delimiter; nested groups always use `(`.
CLOSING = {"(": ")", "{": "}", "[": "]"}
# A `not(` group, which inverts the polarity of everything inside it.
NEGATION = re.compile(r"\bnot\s*\(")


class Gate:
    """One gate spelling, matched against the `cfg` predicate around it.

    Each `PREDICATE` match is walked with delimiters balanced (skipping
    literals). `search` counts the flag only at the `not(` parity the spelling
    asked for; `named` counts it at either. `Name` answers the same two
    questions for an identifier.
    """

    def __init__(self, flag, negated):
        self.flag = flag
        self.negated = negated
        self.pattern = re.compile(r'feature\s*=\s*"' + re.escape(flag) + r'"')

    def search(self, text):
        """Whether any predicate in `text` names this flag at this polarity."""
        return any(self.names(text, found) for found in PREDICATE.finditer(text))

    def named(self, text):
        """Whether any predicate in `text` names this flag at either polarity.

        The offender scan's question: a site gated either way is coupled to the
        flag. Still a predicate walk, so a `cfg_attr` argument does not count.
        """
        return any(
            self.names(text, found, polarity=False)
            for found in PREDICATE.finditer(text)
        )

    def names(self, text, opened, polarity=True):
        """Whether one `opened` predicate names this flag, at this polarity or at any.

        `polarity=False` ignores `not(` parity. A `cfg_attr` is read only up to
        its first comma: the flag varies the item through the predicate, not
        through the attributes applied. Known gap: a `cfg(...)` applied by a
        `cfg_attr` returns `False`; no workspace file writes `cfg_attr`.
        """
        # `not(` parity per open paren, innermost last; ends when the
        # predicate's own delimiter closes.
        parity, i, n = [False], opened.end(), len(text)
        closes = CLOSING[text[i - 1]]
        applies = opened.group(1) == "cfg_attr"
        while i < n and parity:
            if found := NEGATION.match(text, i):
                parity.append(not parity[-1])
                i = found.end()
            elif found := self.pattern.match(text, i):
                if not polarity or parity[-1] == self.negated:
                    return True
                i = found.end()
            elif text[i] == "(":
                parity.append(parity[-1])
                i += 1
            elif text[i] == (closes if len(parity) == 1 else ")"):
                parity.pop()
                i += 1
            elif applies and text[i] == "," and len(parity) == 1:
                # Only a `cfg_attr`'s first argument is a predicate.
                return False
            elif (end := literal_end(text, i)) is not None:
                i = end
            else:
                i += 1
        # Unbalanced: the file does not compile, so report nothing.
        return False


class Name:
    """One identifier spelling, matched as the text it is.

    `Gate`'s counterpart; a name has no polarity, so `search` and `named` agree.
    """

    def __init__(self, pattern):
        self.pattern = pattern

    def search(self, text):
        """Whether `text` writes this spelling."""
        return self.pattern.search(text) is not None

    def named(self, text):
        """The same question. There is no polarity in a name to be blind to."""
        return self.search(text)


BACKTICKED = re.compile(r"`([^`]+)`")
RESIDUE = re.compile(r"[\s,]*")


def backticked(cell):
    """The backticked entries of one cell, or `None` if anything else is in it.

    Outside the backticks only commas and whitespace are allowed, so an entry
    that lost its backticks is not silently dropped. A cell with no backticks
    returns `[]` for the caller to read whole.
    """
    entries = BACKTICKED.findall(cell)
    if entries and not RESIDUE.fullmatch(BACKTICKED.sub("", cell)):
        return None
    return entries


def token(cell):
    """One `(spelling, matcher, gate)` per spelling in a *Named by* cell, or `None`.

    `None` for an unreadable cell rather than a pattern that cannot match, since
    a rule that always passes reports that the elements are off the path
    when nobody has checked.

    A cell is a comma-separated list of backticked entries, each of which may
    brace-expand (`Registry::{new,default}`). An identifier or `::` path (each
    `::` tolerating whitespace) gets a `Name`; a gate or negated gate gets a
    `Gate`. `gate` is true when the spelling must be matched over the
    literal-keeping corpus. One matcher per spelling, so each is held to
    naming a file on its own.
    """
    entries = backticked(cell)
    if entries is None:
        return None
    spellings = []
    for entry in entries or [cell]:
        for spelling in expand(entry.strip()):
            spelling = spelling.strip()
            if named := GATE.fullmatch(spelling):
                spellings.append((spelling, Gate(named.group(1), negated=False), True))
                continue
            if named := NEGATED_GATE.fullmatch(spelling):
                spellings.append((spelling, Gate(named.group(1), negated=True), True))
                continue
            readable = NAMED_BY.fullmatch(spelling)
            if readable is None:
                return None
            segments = [re.escape(part.strip()) for part in readable.group(1).split("::")]
            pattern = r"\s*::\s*".join(segments)
            spellings.append(
                (readable.group(1), Name(re.compile(r"\b" + pattern + r"\b")), False)
            )
    return spellings


def allowed_sites(cell):
    """The files one *Named only in* cell allows.

    A comma-separated list of backticked, brace-expandable paths, relative to
    `OFF_PATH_SCOPE` unless they start at `crates/`. `None` when anything but
    commas sits outside the backticks.
    """
    entries = backticked(cell)
    if entries is None:
        return None
    entries = entries or [part for part in cell.split(",") if part.strip()]
    sites = set()
    for entry in entries:
        for path in expand(entry.strip()):
            path = path.strip()
            sites.add(path if path.startswith("crates/") else OFF_PATH_SCOPE + path.lstrip("/"))
    return sites


def scanned(allowance, exists=None):
    """The `crates/<name>/src/` trees a row's own sites put it in reach of,
    paired with the derived trees that are not directories.

    Always the home scope, plus one tree per crate-qualified site. A missing
    tree is returned so a misspelt crate fails rather than narrowing the scan.
    `exists` is the directory test, injectable for tests.
    """
    if exists is None:
        exists = lambda tree: (ROOT / tree).is_dir()
    trees = {OFF_PATH_SCOPE}
    for site in allowance:
        parts = site.split("/")
        if len(parts) > 3 and parts[0] == "crates" and parts[2] == "src":
            trees.add("/".join(parts[:3]) + "/")
    return sorted(trees), sorted(tree for tree in trees if not exists(tree))


# --- The module-size budget -------------------------------------------------
# Modules past ~400 lines excluding tests are a debt `nfr.md` counts. Sibling
# `tests.rs` files are excluded; an inline `mod tests` counts, read off `raw`.
NFR = (ROOT / "docs/nfr.md").read_text()

# --- The feature grading -----------------------------------------------------
# Every flag `crates/kynos` declares must be graded in `docs/performance.md`.
# Compared as sets read off disk; the manifest is parsed so no key is dropped.
PERFORMANCE = (ROOT / "docs/performance.md").read_text()
# Flags graded here must have an off-path table row (forward only).
OFF_PATH_GRADE = "Off-path proof"


def off_path_coverage(off_path_graded, off_path_elements, grades):
    """What the off-path table fails to cover of what performance.md graded.

    `grades` is every grade the table writes; a missing `OFF_PATH_GRADE` fails
    rather than emptying the compared set.
    """
    if OFF_PATH_GRADE not in grades:
        return [
            f"performance.md's grading table no longer has a {OFF_PATH_GRADE!r} "
            "row, so nothing decides which flags owe a proof that a request "
            "cannot reach them and the off-path table is held to covering nothing"
        ]
    if unproven := sorted(set(off_path_graded) - off_path_elements):
        return [
            f"performance.md grades a flag {OFF_PATH_GRADE}, and no row of "
            "testing.md's off-path table names it. That grade says the flag's "
            "cost is nothing because a request cannot reach what it adds, which "
            "is an argument rather than a measurement, and a graded argument "
            "nobody wrote reads exactly like one that was written and holds:\n"
            "    " + "\n    ".join(unproven)
        ]
    return []


# --- The count of measurement kinds ------------------------------------------
# `performance.md`'s opening count of running kinds must match its taxonomy
# table. Only the running count is held: partial coverage is prose no token
# separates. The sentence is matched as written, `All` included.
TAXONOMY_HEADER = "| Kind | Lives in | Runs under | Proves | Status |"
TAXONOMY_CLAIM = r"All (\w+) of the kinds below run today"
# A Status cell prefix, so `not in use` reads as not running.
RUNNING = "in use"


def taxonomy_failures(text):
    """Whether `performance.md`'s opening count matches its taxonomy table.

    `text` is the document; returns a list of problems, never raises.
    """
    header = text.find(TAXONOMY_HEADER)
    if header < 0:
        return [
            "performance.md no longer has a taxonomy table headed "
            f"`{TAXONOMY_HEADER}`, so the count its opening paragraph states is "
            "held against nothing. Restore the columns, or name the new header "
            "in containment.py in the same commit"
        ]

    kinds = []
    for line in text[header:].split("\n")[2:]:
        if not line.startswith("|"):
            break
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        kinds.append((cells[0], cells[-1]))

    if not kinds:
        return [
            "performance.md's taxonomy table has no rows, so the count above it "
            "holds nothing. A kind that stopped being a kind is retired by "
            "arguing it in that section, not by emptying the table"
        ]

    problems = []
    stated = re.search(TAXONOMY_CLAIM, text)
    if stated is None:
        problems.append(
            "performance.md no longer states how many of the kinds below its "
            "taxonomy run today, so a row can be added or dropped without "
            "failing this gate. The sentence is held as written -- rewording it "
            "means rewording TAXONOMY_CLAIM in the same commit"
        )
    elif (expected := NUMBERS.get(stated.group(1).capitalize())) is None:
        problems.append(
            "performance.md writes an unreadable count of measurement kinds: "
            f"{stated.group(1)!r}"
        )
    elif expected != len(kinds):
        problems.append(
            f"performance.md claims {expected} kinds of measurement and its "
            f"taxonomy table has {len(kinds)}. Adding a kind means moving that "
            "sentence in the same commit; losing one means a row was dropped or "
            "the table was cut short by a blank line"
        )

    idle = [
        kind for kind, status in kinds if not status.casefold().startswith(RUNNING)
    ]
    if idle:
        problems.append(
            f"performance.md says all {len(kinds)} kinds below its taxonomy run "
            "today, and the Status column disagrees: a cell that does not open "
            f"`{RUNNING}` grades a kind that does not run. Either the cell is "
            "wrong, or the sentence is -- and the sentence is the one this gate "
            "holds, so reword it and TAXONOMY_CLAIM together:\n    "
            + "\n    ".join(idle)
        )
    return problems


# --- Nothing a package compiles reaches outside the package ------------------
# `cargo package` carries only the package directory, so a path literal climbing
# out of it breaks the archive. Read raw from disk, over every `.rs` file.
# Relative to the reading file.
INCLUDED = re.compile(r'include_(?:bytes|str)!\s*\(\s*"([^"]*)"')
# Relative to the package; only literals attached directly to the macro.
CONCATENATED = re.compile(r'CARGO_MANIFEST_DIR"\s*\)\s*,\s*"([^"]*)"')
JOINED = re.compile(r'CARGO_MANIFEST_DIR"\s*\)\s*\)?((?:\s*\.join\(\s*"[^"]*"\s*\))+)')
JOIN = re.compile(r'\.join\(\s*"([^"]*)"\s*\)')
# The manifest's `exclude`: files that never reach an archive are exempt.
EXCLUDED = re.compile(r"^exclude\s*=\s*\[([^\]]*)\]", re.M)


def published(package):
    """Every `.rs` file in `package` that a published archive would carry."""
    manifest = EXCLUDED.search((package / "Cargo.toml").read_text())
    exempt = re.findall(r'"([^"]*)"', manifest.group(1)) if manifest else []
    for source in sorted(package.rglob("*.rs")):
        relative = source.relative_to(package).as_posix()
        # A `CARGO_TARGET_DIR` inside a package holds generated sources.
        if "target" in source.relative_to(package).parts:
            continue
        if any(relative == entry or relative.startswith(entry.rstrip("/") + "/") for entry in exempt):
            continue
        yield source


# --- Parent re-exports ------------------------------------------------------
# One canonical path per item: outside `lib.rs`, a `pub use` may not name
# `crate`, `self`, `super` or a module the file declares. Foreign re-exports and
# `pub(crate) use` are allowed.
DECLARED_MODULE = re.compile(r"\bmod\s+(\w+)\s*[;{]")
REEXPORT = re.compile(r"^[ \t]*pub\s+use\s+(\w+)", re.MULTILINE)

# --- Placeholder bodies -----------------------------------------------------
# No `todo!` or `unimplemented!` outside doc examples, which `strip()` removes.
PLACEHOLDER = re.compile(r"\b(todo|unimplemented)!")

# --- The dev build profile ---------------------------------------------------
# `.cargo/config.toml` holds the dev-profile keys behind `nfr.md`'s Workspace
# footprint. Cargo only warns on a misspelt key and ignores a misspelt table, so
# key presence is checked here; values are left to review.
CARGO_CONFIG = ROOT / ".cargo/config.toml"
# Each key as a path through the parsed document, with its display spelling.
FOOTPRINT_KEYS = [
    (("profile", "dev", "debug"), "profile.dev.debug"),
    (("profile", "dev", "package", "*", "debug"), 'profile.dev.package."*".debug'),
]
# The only top-level tables allowed; tool settings belong in a mise task's `env`.
CONFIG_TABLES = {"profile"}


def declares(config, key):
    """Whether the parsed `config` declares the whole of `key`."""
    node = config
    for part in key:
        if not isinstance(node, dict) or part not in node:
            return False
        node = node[part]
    return True


def cargo_config_failures(text):
    """What is wrong with `.cargo/config.toml`, whose contents are `text`.

    `text` is `None` for a file that is not there. Unparseable TOML is
    reported rather than raised.
    """
    if text is None:
        return [
            ".cargo/config.toml is gone, and it is the whole of the dev build "
            "profile: without it a worktree's `target/` returns to roughly four "
            "times the size nfr.md's Workspace table records, with every other "
            "gate here green. Restore it, or move that row in the same commit"
        ]
    try:
        config = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        return [f".cargo/config.toml is not readable TOML: {error}"]

    problems = [
        f".cargo/config.toml no longer declares `{spelling}`, which is one of "
        "the two keys the dev build footprint nfr.md's Workspace table records "
        "rests on. Cargo will not say so: an unused key is a warning over a "
        "build that exits zero, and a misspelled `[profile.<name>]` table is "
        "not reported at all. Restore the key, or move that row in the same "
        "commit"
        for key, spelling in FOOTPRINT_KEYS
        if not declares(config, key)
    ]

    if foreign := sorted(set(config) - CONFIG_TABLES):
        problems.append(
            "`.cargo/config.toml` declares something other than the dev build "
            "profile it exists for. Cargo reads this file on every invocation "
            "under the repository, so what is written here runs on every "
            "machine and in every CI job and no gate reports it -- which is "
            "why the file is held to one table rather than reviewed. Put the "
            "setting in the `env` of the mise.toml task that needs it, or "
            "widen this rule and say in the same commit what the table is "
            "for:\n    " + "\n    ".join(foreign)
        )
    return problems


# --- The Python floor --------------------------------------------------------
# The grammar `scripts/` is held to, matched by every script task's `python`
# pin in mise.toml (`python_pin_failures`). `import tomllib` at the head of this
# file needs 3.11 whatever the grammar allows; that runtime floor is held only
# by running the gate under the pin, so raise this by hand with the stdlib.
PYTHON_FLOOR = (3, 11)
SCRIPTS = ROOT / "scripts"
MISE_CONFIG = ROOT / "mise.toml"
# Tasks running inline `python3` rather than a `scripts/` file, exempt by name.
PYTHON_UNPINNED = {"publish:check"}


def refused_at(source, path, version):
    """The `SyntaxError` parsing `source` at `version` raises, or `None`."""
    try:
        ast.parse(source, filename=path, feature_version=version)
    except SyntaxError as error:
        return error
    return None


def python_floor_failures(scripts, floor=PYTHON_FLOOR):
    """Which of `scripts` stopped parsing at `floor`, and what each one needs.

    `scripts` is `(path, source)` pairs. A failing file is reported with the
    first minor version up to the running interpreter's that parses it, or as
    unparseable at all of them.
    """
    stated, running = f"3.{floor[1]}", f"3.{sys.version_info.minor}"
    problems = []
    for path, source in scripts:
        refused = refused_at(source, path, floor)
        if refused is None:
            continue
        needs = next(
            (
                f"3.{minor}"
                for minor in range(floor[1] + 1, sys.version_info.minor + 1)
                if refused_at(source, path, (3, minor)) is None
            ),
            None,
        )
        stops = f"line {refused.lineno} is where it stops: {refused.msg}"
        if needs is None:
            problems.append(
                f"{path} does not parse at Python {stated}, which is the floor "
                "scripts/ is held to, and it parses at no version up to this "
                f"interpreter's own ({running}) either -- {stops}. Either the "
                "file is broken, or it is written in grammar newer than the "
                "interpreter running this gate, which the `python` pin on the "
                f"script tasks in mise.toml fixes at {stated}. Repair the line, "
                "or raise the pin and this floor in the same commit"
            )
        else:
            problems.append(
                f"{path} does not parse at Python {stated}, which is the floor "
                f"scripts/ is held to: it needs {needs} -- {stops}. Every task "
                f"that runs this file pins {stated}, so the interpreter CI reads "
                "it under is not the one that wrote it. Write the line in "
                f"grammar {stated} accepts, or raise the pin on the script tasks "
                "in mise.toml and this floor in the same commit"
            )
    return problems


def python_pin_failures(text, scripts, floor=PYTHON_FLOOR, exempt=PYTHON_UNPINNED):
    """What `mise.toml`'s `python` pins do not hold, `text` being that file.

    Every task running a `scripts/` file must pin one `python` equal to
    `floor`; only `exempt` tasks may run inline `python3`. `scripts` is the
    paths (relative to `scripts/`) the floor rule read, so a task running a
    file outside that set fails. A script no task runs is allowed. Unparseable
    TOML is reported rather than raised.
    """
    try:
        config = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        return [f"mise.toml is not readable TOML: {error}"]

    stated = f"3.{floor[1]}"
    problems, run_by, pinned, unpinned, inline = [], {}, {}, [], set()
    for name, task in sorted(config.get("tasks", {}).items()):
        run = task.get("run")
        body = "\n".join(
            part
            for part in ([run] if isinstance(run, str) else run or [])
            if isinstance(part, str)
        )
        named = set(re.findall(r"scripts/(\S+\.py)", body))
        pin = task.get("tools", {}).get("python")
        for script in named:
            run_by.setdefault(script, []).append(name)
        if named and pin is None:
            unpinned.append(name)
        elif named:
            pinned.setdefault(pin, []).append(name)
        elif pin is not None:
            problems.append(
                f"the mise task `{name}` pins a `python` and runs no file from "
                "scripts/, so the pin installs an interpreter for a task that "
                "opens none and repeats a floor it does not stand under. Drop the "
                "pin, or give the task the script it was added for"
            )
        elif "python3" in body:
            inline.add(name)

    if unpinned:
        problems.append(
            "a mise task runs a file from scripts/ and pins no `python`, so what "
            "it runs is whatever interpreter the machine has rather than the "
            f"{stated} floor containment.py declares. The pin is the only half of "
            "that number that is true of anything anybody runs; add "
            "`tools = { python = ... }` at the version its siblings carry:\n    "
            + "\n    ".join(unpinned)
        )

    if len(pinned) > 1:
        problems.append(
            "the mise tasks that run a file from scripts/ pin more than one "
            "`python`, so which interpreter the floor names depends on which task "
            "a reader opened. Raising the floor is one commit moving every pin and "
            "`PYTHON_FLOOR` together, and this is what reports that it was not "
            "one commit:\n    "
            + "\n    ".join(
                f"{version}: " + ", ".join(sorted(names))
                for version, names in sorted(pinned.items())
            )
        )

    for version, names in sorted(pinned.items()):
        read = re.match(r"(\d+)\.(\d+)", version)
        if read is None:
            problems.append(
                f'mise.toml pins `python = "{version}"` for the tasks that run '
                "scripts/ and nothing here can read a minor version out of it, so "
                "the pin cannot be compared against the floor it is half of. Pin a "
                f"version, at {stated}:\n    " + "\n    ".join(sorted(names))
            )
        elif (int(read[1]), int(read[2])) != floor:
            problems.append(
                f"mise.toml pins python {version} for the tasks that run scripts/ "
                f"and containment.py declares a floor of {stated}. These are one "
                "number written twice: a floor nothing runs at is a claim nobody "
                "keeps, and an interpreter nothing declares is what the floor rule "
                "exists to refuse. Move both in the same commit:\n    "
                + "\n    ".join(sorted(names))
            )

    if unread := sorted(set(run_by) - set(scripts)):
        problems.append(
            "a mise task runs a file under scripts/ that the floor rule never "
            "read, so a file something runs is held to no grammar at all. Either "
            "the task names a file that is not there, or the floor rule's own file "
            "set has narrowed and this is the only thing that says so:\n    "
            + "\n    ".join(
                f"{script}: run by " + ", ".join(sorted(run_by[script]))
                for script in unread
            )
        )

    if stale := sorted(set(exempt) - inline):
        problems.append(
            "mise.toml no longer holds a task `PYTHON_UNPINNED` exempts from the "
            "`python` pin. That exemption is for an inline `python3` running no "
            "file in scripts/, and so held to no floor; naming a task that has "
            "gone, or that has stopped running python3, is an exemption that "
            "outlived its argument and now hides the next one:\n    "
            + "\n    ".join(stale)
        )

    if unexempt := sorted(inline - set(exempt)):
        problems.append(
            "a mise task runs `python3` over something other than a file in "
            "scripts/, so it is held to no declared floor and nothing here can say "
            "what it should be pinned to. Move the code into scripts/ where the "
            "floor rule reads it, or name the task in `PYTHON_UNPINNED` and say "
            "there why an inline interpreter is the right trade:\n    "
            + "\n    ".join(unexempt)
        )

    return problems


def main(architecture=None, testing=None, performance=None, nfr=None, corpus=None):
    """Run every rule over this repository, and report what does not hold.

    Rules run here, never at import. Each argument defaults to this
    repository's own document or `WORKSPACE`, and may be replaced by a test.
    """
    corpus = WORKSPACE if corpus is None else corpus
    architecture = ARCHITECTURE if architecture is None else architecture
    testing = TESTING if testing is None else testing
    performance = PERFORMANCE if performance is None else performance
    nfr = NFR if nfr is None else nfr
    failures = []

    # --- The runtime allowance table ------------------------------------------
    rows = []
    table = section(
        architecture,
        "| Site | Names | Why it is not in `server/` |",
        failures,
        unrun=(
            "architecture.md's runtime allowance table goes unparsed, and with it "
            "both rules stated over it: the row count the document itself calls the "
            "check, and the `tokio` offender scan, which over an empty allowance "
            "would report every site in the crate and bury the renamed heading that "
            "caused it"
        ),
    )
    if table is not None:
        for line in table.split("\n")[2:]:
            if not line.startswith("|"):
                break
            rows.append(re.findall(r"`([^`]+)`", line.split("|")[1]))

        stated = claimed(architecture, ROW_COUNT_CLAIM, failures)
        if stated is not None and stated != len(rows):
            failures.append(
                f"architecture.md's allowance table claims {stated} rows and has {len(rows)}"
            )

        allowed = {f"crates/kynos/src/{site.lstrip('/')}" for row in rows for entry in row for site in expand(entry)}

        if offenders := sorted(p for p in corpus.naming("tokio") if not permitted(p, allowed)):
            failures.append(
                "`tokio` is named outside `server/` at a site the allowance table does "
                "not list:\n    " + "\n    ".join(offenders)
            )

    # --- The dependency graph ------------------------------------------------
    for crates, rule, where, description in [
        (("hyper", "hyper_util"), ONLY_IN,
         {"crates/kynos/src/server/connection.rs", "crates/kynos/src/http/body.rs"},
         "`hyper` and `hyper-util` are named only in `server/connection.rs` and `http/body.rs`"),
        (("rustls", "tokio_rustls"), UNDER, "crates/kynos/src/server/tls/",
         "`tokio-rustls` and `rustls` are named only under `server/tls/`"),
        (("socket2",), ONLY_IN, {"crates/kynos/src/server/tcp.rs"},
         "`socket2` is named only in `server/tcp.rs`"),
        (("matchit",), UNDER, "crates/kynos/src/router/",
         "`matchit` may be named only under `router/`"),
        (("h2", "httparse"), ONLY_IN, set(), "`h2` and `httparse` are never named"),
        (("tower", "tower_layer", "tower_service"), ONLY_IN, {"crates/kynos/src/unchecked.rs"},
         "`tower`, `tower-layer` and `tower-service` are named only in `unchecked.rs`"),
        (("http_body", "http_body_util"), ONLY_IN,
         {"crates/kynos/src/http/body.rs", "crates/kynos/src/http/body/watched.rs",
          "crates/kynos/src/extract/body/",
          "crates/kynos/src/middleware/cache/mod.rs", "crates/kynos/src/middleware/compression/",
          "crates/kynos/src/middleware/decompression/", "crates/kynos/src/middleware/limits/",
          "crates/kynos/src/response/range/source.rs", "crates/kynos/src/router/dispatch.rs",
          "crates/kynos/src/test/mod.rs"},
         "`http-body` and `http-body-util` are named only at the body sites architecture.md lists"),
        (("async_compression",), ONLY_IN,
         {"crates/kynos/src/middleware/compression/", "crates/kynos/src/middleware/decompression/"},
         "`async-compression` is named only under `middleware/compression/` and "
         "`middleware/decompression/`"),
        (("serde_html_form",), ONLY_IN,
         {"crates/kynos/src/extract/body/form.rs", "crates/kynos/src/response/codec/form.rs",
          "crates/kynos/src/test/mod.rs"},
         "`serde_html_form` is named only in `extract/body/form.rs`, `response/codec/form.rs` "
         "and `test/mod.rs`"),
        (("tracing",), ONLY_IN,
         {"crates/kynos/src/server/", "crates/kynos/src/middleware/trace.rs"},
         "`tracing` is named only under `server/` and in `middleware/trace.rs`"),
        (("futures_core",), ONLY_IN,
         {"crates/kynos/src/response/stream/", "crates/kynos/src/extract/body/json_lines/",
          "crates/kynos/src/http/body.rs"},
         "`futures-core` is named only under `response/stream/` and `extract/body/json_lines/` "
         "and in `http/body.rs`"),
        (("multer",), ONLY_IN, {"crates/kynos/src/extract/body/multipart.rs"},
         "`multer` is named only in `extract/body/multipart.rs`"),
        (("prost",), ONLY_IN,
         {"crates/kynos/src/extract/body/protobuf.rs", "crates/kynos/src/response/codec/protobuf.rs"},
         "`prost` is named only in `extract/body/protobuf.rs` and `response/codec/protobuf.rs`"),
        (("uuid",), ONLY_IN, {"crates/kynos/src/schema/impls/identifier.rs"},
         "`uuid` is named only in `schema/impls/identifier.rs`"),
        (("chrono", "jiff"), UNDER, "crates/kynos/src/schema/impls/temporal/",
         "`chrono` and `jiff` are named only under `schema/impls/temporal/`"),
        (("rust_decimal", "bigdecimal"), UNDER, "crates/kynos/src/schema/impls/decimal/",
         "`rust_decimal` and `bigdecimal` are named only under `schema/impls/decimal/`"),
        (("percent_encoding",), ONLY_IN, {"crates/kynos/src/__private/uri.rs"},
         "`percent-encoding` is named only in `__private/uri.rs`"),
        (("jsonschema",), ONLY_IN, {"crates/kynos/src/test/conformance.rs"},
         "`jsonschema` is named only in `test/conformance.rs`"),
        (("regex", "regex_syntax"), ONLY_IN,
         {"crates/kynos/src/__private/constraints/pattern.rs",
          "crates/kynos-openapi/src/pattern.rs"},
         "`regex` and `regex-syntax` are named only in `__private/constraints/pattern.rs` and "
         "`kynos-openapi`'s `pattern.rs`"),
        (("serde_yaml_ng",), ONLY_IN,
         {"crates/kynos-openapi/src/emit/", "crates/kynos/src/error/mod.rs"},
         "`serde_yaml_ng` is named only under `kynos-openapi/emit/` and in `error/mod.rs`"),
        (("indexmap",), UNDER, "crates/kynos-openapi/src/",
         "`indexmap` is named only in `kynos-openapi`"),
        (("proc_macro2", "quote", "syn"), UNDER, "crates/kynos-macros/src/",
         "`proc-macro2`, `quote` and `syn` are named only in `kynos-macros`"),
    ]:
        found = corpus.naming(*crates)
        stray = sorted(f for f in found if not f.startswith(where)) if rule == UNDER else sorted(f for f in found if not listed(f, where))
        if stray:
            failures.append(f"{description}, but it is also named in:\n    " + "\n    ".join(stray))

    # --- The off-path elements -----------------------------------------------
    halves = testing.split(OFF_PATH_HEADER)
    if len(halves) != 2:
        failures.append(
            "testing.md no longer holds exactly one off-path table under the header "
            "this rule reads, so nothing states which elements a request may not "
            "reach"
        )

    off_path_rows = 0
    # Backticked tokens of every *Element* cell, for the feature grading below.
    off_path_elements = set()
    for line in (halves[1] if len(halves) == 2 else "").split("\n")[2:]:
        if not line.startswith("|"):
            break
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) != 4:
            failures.append(f"testing.md's off-path table has a malformed row: {line.strip()}")
            continue

        element, named_by, where, reason = cells
        off_path_rows += 1
        off_path_elements |= set(re.findall(r"`([^`]+)`", element))
        allowance = allowed_sites(where)
        if allowance is None:
            failures.append(
                f"testing.md's off-path table allows {element} in {where}, which "
                "this rule cannot read as a comma-separated list of backticked "
                "paths. The row holds nothing until it can, and it holds it "
                "nowhere: the trees this row is scanned in are derived from these "
                "sites, so an unreadable cell is an unscanned crate rather than an "
                "unchecked path"
            )
            continue
        trees, missing = scanned(allowance)
        if missing:
            failures.append(
                f"testing.md's off-path table allows {element} in a crate that "
                "does not exist, so the tree derived from that site is scanned for "
                "nothing and the row narrows back to " + OFF_PATH_SCOPE + " without "
                "saying so. Either the crate was renamed and the cell was not, or "
                "the site is a typo. The row's sites go unchecked meanwhile:\n    "
                + "\n    ".join(missing)
            )
            continue
        scope = ", ".join(trees)
        spellings = token(named_by)
        if spellings is None:
            failures.append(
                f"testing.md's off-path table names {element} with {named_by}, "
                "which this rule cannot read as an identifier or a path of them. "
                "The row holds nothing until it can: teach the rule the token, or "
                "write one it already knows. The row's sites go unchecked "
                "meanwhile"
            )
            continue

        # Each spelling must exist on its own (a union would hide a typo behind a
        # live spelling). Asked of `sources`, which adds sibling test files: a
        # spelling only tests write is the row at its strongest.
        stale = [
            (spelling, gate)
            for spelling, pattern, gate in spellings
            if not any(
                any(path.startswith(tree) for tree in trees) and pattern.search(text)
                for path, text in (corpus.gate_sources if gate else corpus.sources)
            )
        ]
        if stale:
            for spelling, gate in stale:
                failures.append(
                    f"testing.md's off-path table names {element} with "
                    f"`{spelling}`, and nothing under {scope} writes that spelling "
                    f"{LOOKED_IN[gate]}, sibling test files included. The row holds "
                    "nothing under it: either the element was renamed and the cell "
                    "was not, or it now lives outside the scope this row's own "
                    "sites reach, which is a site to add rather than a spelling to "
                    "keep. The row's sites go unchecked until the cell is repaired"
                )
            # One failure per row: a stale cell's sites are not scanned.
            continue

        # Offenders: request-runnable files any spelling names, at either
        # polarity (`named`), outside the row's sites.
        named = sorted(
            {
                path
                for _, pattern, gate in spellings
                for path, text in (corpus.gate_files if gate else corpus.files)
                if any(path.startswith(tree) for tree in trees) and pattern.named(text)
            }
        )
        if offenders := [path for path in named if path not in allowance]:
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

    # The stated row count catches a deleted row or a table cut short.
    elif len(halves) == 2:
        stated = re.search(ROW_COUNT_CLAIM, testing)
        if stated is None:
            failures.append(
                "testing.md no longer states how many rows its off-path table has, "
                "so a row can be dropped without failing this gate"
            )
        else:
            expected = NUMBERS.get(stated.group(1).capitalize())
            if expected is None:
                failures.append(
                    f"testing.md writes an unreadable off-path row count: "
                    f"{stated.group(1)!r}"
                )
            elif expected != off_path_rows:
                failures.append(
                    f"testing.md claims {expected} off-path rows and the table has "
                    f"{off_path_rows}. Adding an element means saying so there; "
                    "losing one means a row was dropped or the table was cut short"
                )

    # --- Hand-rolled `Stream` implementations ---------------------------------
    # Only links in the section that enumerates them authorise a site.
    surface = section(
        architecture,
        "### Public API surface",
        failures,
        "\n## ",
        unrun=(
            "architecture.md's list of the sites that may declare a hand-rolled "
            "`Stream` goes unparsed, leaving nothing to check that every "
            "implementation sits at one of them. The count of implementations is "
            "still held, since it is read off the source rather than out of this "
            "section"
        ),
    )
    hand_rolled = {path for path, text in corpus.files if re.search(r"\bStream\s+for\b", text)}

    sites = claimed(
        architecture,
        r"\*\*One public row, (\w+) sites, and the count is the check\*\*",
        failures,
    )
    if sites is not None and sites != len(hand_rolled):
        failures.append(
            f"architecture.md claims {sites} hand-rolled `Stream` sites and there are "
            f"{len(hand_rolled)}:\n    " + "\n    ".join(sorted(hand_rolled))
        )
    if surface is not None:
        declared = set(re.findall(r"\]\(\.\./(crates/[^)]+\.rs)\)", surface))
        if undeclared := sorted(hand_rolled - declared):
            failures.append(
                "a hand-rolled `Stream` sits where architecture.md names no site:\n    "
                + "\n    ".join(undeclared)
            )

    # --- The module-size budget ----------------------------------------------
    oversized = sorted(
        path
        for path, _ in corpus.files
        if corpus.raw[path].count("\n") > 400
    )

    budget = re.search(r"a module-size budget of (\d+) files", nfr)
    if budget is None:
        failures.append("nfr.md no longer states the module-size budget")
    elif int(budget.group(1)) != len(oversized):
        failures.append(
            f"nfr.md budgets {budget.group(1)} files over ~400 lines and there are "
            f"{len(oversized)}. Splitting one means lowering the budget in the same "
            "commit; adding one means arguing for it there.\n    "
            + "\n    ".join(oversized)
        )

    # --- The feature grading -------------------------------------------------
    grading = section(
        performance,
        "| Grade | Owes | Flags |",
        failures,
        unrun=(
            "performance.md's grading table goes unparsed, and every rule stated "
            "over it goes unrun: the off-path coverage comparison, and the "
            "ungraded, undeclared and regraded checks. Over an empty grading "
            "every flag the crate declares reads as ungraded, which reports one "
            "problem per feature where there is one problem in total"
        ),
    )

    graded, off_path_graded, grades = [], [], []
    if grading is not None:
        for line in grading.split("\n")[2:]:
            if not line.startswith("|"):
                break
            cells = line.split("|")
            flags = re.findall(r"`([^`]+)`", cells[3])
            graded += flags
            grades.append(cells[1].strip())
            if cells[1].strip() == OFF_PATH_GRADE:
                off_path_graded += flags

        failures += off_path_coverage(off_path_graded, off_path_elements, grades)

    manifest = tomllib.loads((ROOT / "crates/kynos/Cargo.toml").read_text())
    flags = set(manifest["features"])
    depended = {
        member[len("dep:") :]
        for members in manifest["features"].values()
        for member in members
        if member.startswith("dep:")
    }

    # Separate guard: `flags` is now the crate's feature set, not one row's.
    if grading is not None:
        if ungraded := sorted(flags - set(graded)):
            failures.append(
                "crates/kynos declares a feature that performance.md's grading table "
                "does not grade. Grading it is the argument the table exists to force: "
                "a full battery, an off-path proof, or an aggregate that owes nothing "
                "of its own:\n    " + "\n    ".join(ungraded)
            )

        if undeclared := sorted(set(graded) - flags):
            failures.append(
                "performance.md grades a flag that crates/kynos does not "
                "declare, so the row names a battery nothing can be enabled to "
                "owe. Either the flag was renamed and the row was not, or the "
                "row outlived the feature:\n    "
                + "\n    ".join(undeclared)
            )

        if regraded := sorted({flag for flag in graded if graded.count(flag) > 1}):
            failures.append(
                "performance.md grades a flag in more than one row, where the table "
                "says every flag appears in exactly one column. Two grades are two "
                "different batteries owed and nothing decides between them:\n    "
                + "\n    ".join(regraded)
            )

    # An optional dependency no `dep:` names gets an implicit feature that
    # `[features]` does not list, invisible to the comparisons above.
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

    # --- The count of measurement kinds --------------------------------------
    failures += taxonomy_failures(performance)

    # --- Nothing a package compiles reaches outside the package --------------
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

    # --- Parent re-exports ---------------------------------------------------
    reexports = []
    for path, text in corpus.files:
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

    # --- Placeholder bodies --------------------------------------------------
    if placeholders := sorted(path for path, text in corpus.files if PLACEHOLDER.search(text)):
        failures.append(
            "a `todo!()` or `unimplemented!()` stands in for a body, and the exception that allowed one "
            "lapsed when the API-skeleton milestone ended:\n    " + "\n    ".join(placeholders)
        )

    # --- The dev build profile -----------------------------------------------
    failures += cargo_config_failures(
        CARGO_CONFIG.read_text() if CARGO_CONFIG.is_file() else None
    )

    # --- The Python floor -----------------------------------------------------
    scripts = sorted(SCRIPTS.glob("*.py"))
    failures += python_floor_failures(
        (path.relative_to(ROOT).as_posix(), path.read_text()) for path in scripts
    )
    failures += python_pin_failures(
        MISE_CONFIG.read_text(), {path.relative_to(SCRIPTS).as_posix() for path in scripts}
    )

    # --- Report ---------------------------------------------------------------
    for failure in failures:
        print(f"containment: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(
        f"containment: {len(corpus.files)} source files, {len(rows)} allowance rows, "
        f"{off_path_rows} off-path rows, {len(graded)} graded features, "
        "every rule holds"
    )
    return 0


# Importing runs no rule; `containment_test.py` relies on that.
if __name__ == "__main__":
    sys.exit(main())
