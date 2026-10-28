"""Containment checks: a crate may be named only where its owner allows.

Reads the allowance tables in `docs/` rather than restating them. Source is
matched with comments, string literals and `#[cfg(test)]` modules (inline and
sibling files) removed, keeping only the literals that spell a path, such as
`#[serde(with = "…")]`; feature-gate rules use a second corpus that keeps
literals, since a `#[cfg(feature = "x")]` flag name is a string.
"""

import os
import re
import sys
import tomllib
from pathlib import Path

# Relative to the script, so a run from any directory checks the same tree.
ROOT = Path(__file__).resolve().parent.parent
#: The `kynos` crate's source tree, which a bare site in every table is relative to.
KYNOS_SOURCE = "crates/kynos/src/"
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
# An attribute argument whose string is a Rust path or type rather than prose:
# serde's function, conversion and bound keys. Read up to the literal it opens.
PATH_VALUED = re.compile(
    r"[(,]\s*(?:with|serialize_with|deserialize_with|skip_serializing_if|default"
    r"|getter|remote|from|try_from|into|bound|crate)\s*=\s*\Z"
)
# How far back from a literal `PATH_VALUED` is looked for.
PATH_KEY_REACH = 64


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
    `#[cfg(feature = "x")]` gate is read against. A block comment leaves one
    space, as the compiler reads it, so `use/**/tokio` stays two tokens. A
    `PATH_VALUED` literal is code spelt as a string and survives in both.
    """
    # `masked` blanks every literal, for the brace counter; `named` keeps the
    # path-valued ones, and is what `literals=True` returns.
    masked, named, kept, i, n = [], [], [], 0, len(source)
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
            for text in (masked, named, kept):
                text.append(" ")
        elif (end := literal_end(source, i)) is not None:
            literal = source[i:end]
            masked.append(blank(literal))
            path = PATH_VALUED.search(source[max(0, i - PATH_KEY_REACH) : i])
            named.append(literal if path else blank(literal))
            kept.append(literal)
            i = end
        else:
            for text in (masked, named, kept):
                text.append(source[i])
            i += 1
    masked, named, kept = "".join(masked), "".join(named), "".join(kept)

    text = kept if not literals else named
    for start, end in reversed(test_module_spans(masked)):
        text = text[:start] + text[end:]
    return text


RAW_SOURCES = [
    (path.relative_to(ROOT).as_posix(), path.read_text())
    for crate in sorted((ROOT / "crates").iterdir())
    if (crate / "src").is_dir()
    for path in sorted((crate / "src").rglob("*.rs"))
]

#: Members whose manifest sets `publish = false`, as `crates/<name>/` prefixes.
#: They consume `kynos` as an application does rather than being part of it --
#: `kynos-profile` drives a runtime and drains bodies -- so which crates they
#: name is their own manifest's business, and the rules over which framework
#: module may name a crate, all read through `Corpus.naming`, skip them.
UNPUBLISHED = tuple(
    f"crates/{crate.name}/"
    for crate in sorted((ROOT / "crates").iterdir())
    if (crate / "Cargo.toml").is_file()
    and tomllib.loads((crate / "Cargo.toml").read_text())["package"].get("publish") is False
)


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
        """The files naming any of `crates` as an identifier, outside an
        `UNPUBLISHED` member."""
        pattern = re.compile(r"\b(" + "|".join(crates) + r")\b")
        return {
            path
            for path, text in self.files
            if not path.startswith(UNPUBLISHED) and pattern.search(text)
        }

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


def claimed(text, sentence, failures, document="architecture.md", gone=None, count="count"):
    """The number `document`'s `text` writes into the count claim `sentence`, or `None`.

    A missing count is appended to `failures` as `document` no longer stating
    `gone` (by default, a count matching `sentence`), and an unreadable one as
    an unreadable `count`.
    """
    found = re.search(sentence, text)
    if found is None:
        if gone is None:
            gone = (
                f"a count matching /{sentence}/, so the claim this gate exists "
                "to hold is gone"
            )
        failures.append(f"{document} no longer states {gone}")
        return None
    word = found.group(1)
    number = NUMBERS.get(word.capitalize())
    if number is None:
        failures.append(f"{document} writes an unreadable {count}: {word!r}")
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


def table_rows(table):
    """Each row of the Markdown table `table` opens at, as its raw line.

    `table` starts on the header line; it and the separator below it are
    skipped, and the first line not opening `|` ends the table.
    """
    for line in table.split("\n")[2:]:
        if not line.startswith("|"):
            return
        yield line


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
    if path.startswith(HOME_SITE):
        return True
    return allowed_at(path, allowed)


def allowed_at(path, sites):
    """Whether `path` is one of an allowance row's `sites`, or under one.

    A site is a file or a directory, with or without a trailing `/`, matched as
    a path segment prefix, so `runtime` holds `runtime/spawn.rs` but not
    `runtimefoo.rs`. The *Site* and *Names* rules both match through this, so
    no file a row lets name tokio escapes the items that row names.
    """
    return any(path == site or path.startswith(site.rstrip("/") + "/") for site in sites)


#: The runtime's own module: its row's *Names* cell is prose, and not enforced.
HOME_SITE = KYNOS_SOURCE + "server/"
# A `use` statement, through to its `;`.
USE_STATEMENT = re.compile(r"\buse\s+[^;]*;")
# A path rooted at the `tokio` crate: not a segment of another path, such as
# `async_compression::tokio`, and not a field or method.
RUNTIME_PATH = re.compile(r"(?<![\w:.$])(?:::\s*)?tokio\b(?:\s*::\s*\w+)*")


def runtime_names(cell):
    """The tokio items one *Names* cell lists, as `::` paths, or `None`.

    Each backticked entry is a use tree rooted at `tokio`; `None` when an entry
    is not, when anything but commas sits outside the backticks, or when the
    cell lists nothing.
    """
    entries = backticked(cell)
    if not entries:
        return None
    names = [path for entry in entries for path in use_paths(entry)]
    if not names or not all(name.startswith("tokio::") for name in names):
        return None
    return names


def runtime_paths(text):
    """Every path rooted at the `tokio` crate that `text` writes, as `::` paths.

    `use` trees are expanded first, and their text set aside so a group's
    `{tokio::…}` member is read once; a path written inline is read through
    its last segment, which for an associated item is past the type.
    """
    paths = [
        path
        for statement in USE_STATEMENT.findall(text)
        for path in use_paths(statement[len("use") : -1])
        if path.split("::")[0] == "tokio"
    ]
    inline = USE_STATEMENT.sub(lambda found: blank(found.group(0)), text)
    paths += [re.sub(r"\s", "", found.group(0)).lstrip(":") for found in RUNTIME_PATH.finditer(inline)]
    return sorted(set(paths))


def admits(names, item):
    """Whether `item` is one of `names`, or an associated item of one."""
    return any(item == name or item.startswith(name + "::") for name in names)


# --- The dependency graph ---------------------------------------------------
# `nfr.md`'s Dependencies table: each row whose Method is this gate is one
# confinement, read off its Requirement sentence.
DEPENDENCIES = "\n## Dependencies\n"
REQUIREMENTS_HEADER = "| Category | Requirement | Method | Status |"
CONTAINMENT_METHOD = "`mise run containment:check`"
# Crates, then where they may be named; `never named` lists no site.
REQUIREMENT = re.compile(
    r"(?P<crates>.*?)\b(?:is|are|may be) "
    r"(?:(?P<never>never named)|named only (?:in|under|at) (?P<sites>.*))"
)
CRATE_RESIDUE = re.compile(r"(?:[\s,]|\band\b)*")
SITE_RESIDUE = re.compile(r"(?:[\s,]|\b(?:and|in|under)\b)*")
CRATE_TOKEN = re.compile(r"[\w-]+")
#: Workspace members with a `src/` tree, by directory name.
MEMBERS = frozenset(
    crate.name for crate in (ROOT / "crates").iterdir() if (crate / "src").is_dir()
)


def site_path(site, members=MEMBERS):
    """`kynos-openapi/emit/` -> `crates/kynos-openapi/src/emit/`; `router/` ->
    `crates/kynos/src/router/`."""
    head, _, rest = site.partition("/")
    if head in members:
        return f"crates/{head}/src/{rest}"
    return KYNOS_SOURCE + site.lstrip("/")


def confinement(requirement, members=MEMBERS):
    """One Requirement sentence as `(identifiers, sites)`, or `None`.

    `identifiers` are the backticked crates before `is`/`are`, as Rust spells
    them (`hyper-util` -> `hyper_util`); `sites` are the backticked paths after
    `named only in`/`under`/`at`, through `site_path`, and empty for `never
    named`. `None` when anything but commas and the joining words sits outside
    the backticks, so a site written as prose is refused rather than dropped.
    """
    read = REQUIREMENT.fullmatch(requirement.strip())
    if read is None:
        return None
    crates = BACKTICKED.findall(read["crates"])
    if not crates or not CRATE_RESIDUE.fullmatch(BACKTICKED.sub("", read["crates"])):
        return None
    if not all(CRATE_TOKEN.fullmatch(crate) for crate in crates):
        return None
    sites = []
    if read["sites"] is not None:
        sites = BACKTICKED.findall(read["sites"])
        if not sites or not SITE_RESIDUE.fullmatch(BACKTICKED.sub("", read["sites"])):
            return None
    identifiers = tuple(crate.replace("-", "_") for crate in crates)
    return identifiers, {site_path(site, members) for site in sites}


def dependency_rules(nfr, failures, members=MEMBERS):
    """Every confinement `nfr.md`'s Dependencies table assigns this gate.

    `(identifiers, sites, requirement)` per row whose Method is
    `CONTAINMENT_METHOD`. A missing section or header, an unreadable row, or
    no row at all is appended to `failures`.
    """
    unrun = (
        "the dependency graph goes unchecked: every crate it confines may be "
        "named anywhere"
    )
    text = section(nfr, DEPENDENCIES, failures, unrun=unrun)
    if text is None:
        return []
    # Up to the next section, so another section's table is never read as this.
    if (end := text.find("\n## ", 1)) >= 0:
        text = text[:end]
    header = text.find(REQUIREMENTS_HEADER)
    if header < 0:
        failures.append(
            f"nfr.md's Dependencies section no longer holds a table headed "
            f"`{REQUIREMENTS_HEADER}`, so {unrun}"
        )
        return []
    rules = []
    for line in table_rows(text[header:]):
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if CONTAINMENT_METHOD not in line:
            continue
        if len(cells) != 4 or cells[2] != CONTAINMENT_METHOD:
            failures.append(
                f"nfr.md's Dependencies table has a malformed row: {line.strip()}"
            )
            continue
        read = confinement(cells[1], members)
        if read is None:
            failures.append(
                f"nfr.md's Dependencies table states {cells[1]!r}, which this rule "
                "cannot read as backticked crates that `are named only in`, "
                "`under` or `at` backticked sites, or `are never named`. The row "
                "holds nothing until it can"
            )
            continue
        rules.append((*read, cells[1]))
    if not rules:
        failures.append(
            "nfr.md's Dependencies table assigns this gate no row, so " + unrun
        )
    return rules


def listed(path, sites):
    """Whether `path` is one of a dependency row's `sites`.

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


# A *Named by* spelling: an identifier or `::` path. Backticks separate several
# spellings in one cell and are optional for a single bare token.
NAMED_BY = re.compile(r"`?(\w+(?:\s*::\s*\w+)*)`?")
# A feature-gate spelling, at either polarity: `feature = "x"` names what the
# flag compiles when on, `not(feature = "x")` what it compiles when off.
# `Gate.search` (existence) reads the polarity; `Gate.named` (offenders) does not.
GATE = re.compile(r'`?feature\s*=\s*"([\w-]+)"`?')
NEGATED_GATE = re.compile(r'`?not\(\s*feature\s*=\s*"([\w-]+)"\s*\)`?')
# Where a `cfg` predicate starts: `#[cfg(`, `#![cfg(`, `cfg_attr` likewise, or
# a `cfg!` under any of its three delimiters (attributes accept only `(`), bare
# or through `std::`/`core::`. The lookbehind refuses `mycfg!`, any other
# `path::cfg!` and `$cfg!`, which text cannot resolve to the real macro.
PREDICATE = re.compile(
    r"#!?\[\s*(cfg_attr|cfg)\s*\(|(?<![\w:$])(?:(?:::\s*)?(?:std|core)\s*::\s*)?cfg\s*!\s*([({\[])"
)
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
        # The lookbehind refuses `target_feature`, a different predicate.
        self.pattern = re.compile(r'(?<!\w)feature\s*=\s*"' + re.escape(flag) + r'"')

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


class Owned:
    """One type, matched wherever it is named other than behind a reference.

    `Name`'s other counterpart: `&T`, `&mut T` and `&'a mut T` borrow a value
    minted elsewhere, and a `use` only brings the name into scope, so neither
    counts. Every other mention -- a path through the type, a field, binding,
    parameter or return of it, a generic argument -- holds or makes one by
    value, which is what a `Default::default()` or a derived `Default` needs
    to have been written beside.
    """

    # The path segments before the type, then what precedes the path.
    PREFIX = re.compile(r"(?:(?:::\s*)?\w+\s*::\s*)*(?:::\s*)?\Z")
    BORROWED = re.compile(r"&\s*(?:'\w+\s*)?(?:mut\s+)?\Z")

    def __init__(self, type_name):
        self.pattern = re.compile(r"\b" + re.escape(type_name) + r"\b")

    def search(self, text):
        """Whether `text` names this type by value anywhere."""
        text = USE_STATEMENT.sub(lambda found: blank(found.group(0)), text)
        for found in self.pattern.finditer(text):
            before = text[: found.start()]
            path = self.PREFIX.search(before[-256:])
            if not self.BORROWED.search(before[: len(before) - len(path.group(0))]):
                return True
        return False

    def named(self, text):
        """The same question: a borrow is no more a site at either polarity."""
        return self.search(text)


BACKTICKED = re.compile(r"`([^`]+)`")
RESIDUE = re.compile(r"[\s,]*")
# An *owned* spelling: the type, wherever it is not behind a reference.
OWNED = re.compile(r"owned\s+(\w+)")


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
    `::` tolerating whitespace) gets a `Name`; `owned T` gets an `Owned`; a
    gate or negated gate gets a `Gate`. `gate` is true when the spelling must be matched over the
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
            if named := OWNED.fullmatch(spelling):
                spellings.append((spelling, Owned(named.group(1)), False))
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
    `KYNOS_SOURCE` unless they start at `crates/`. `None` when anything but
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
            sites.add(path if path.startswith("crates/") else KYNOS_SOURCE + path.lstrip("/"))
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
    trees = {KYNOS_SOURCE}
    for site in allowance:
        parts = site.split("/")
        if len(parts) > 3 and parts[0] == "crates" and parts[2] == "src":
            trees.add("/".join(parts[:3]) + "/")
    return sorted(trees), sorted(tree for tree in trees if not exists(tree))


# --- The module-size budget -------------------------------------------------
# Modules past ~400 lines excluding tests are a debt `nfr.md` lists by path.
# Sibling `tests.rs` files are excluded; an inline `mod tests` counts, read off
# `raw`.
NFR = (ROOT / "docs/nfr.md").read_text()
BUDGET = "### The module-size budget"
# One list item per budgeted path, nothing else on the line.
BUDGETED = re.compile(r"^- `(crates/[^`]+\.rs)`$", re.MULTILINE)
MODULE_LINES = 400


def budget_failures(listed_paths, oversized):
    """What the budget's `listed_paths` and the `oversized` files disagree on.

    Compared as sets, so each failure names its paths and a split cannot
    cancel a regression out.
    """
    problems = []
    if unlisted := sorted(set(oversized) - set(listed_paths)):
        problems.append(
            f"a module is past ~{MODULE_LINES} lines and nfr.md's module-size "
            "budget does not list it. Either split it, or list it there with the "
            "argument for why it stays one file, in the same commit:\n    "
            + "\n    ".join(unlisted)
        )
    if stale := sorted(set(listed_paths) - set(oversized)):
        problems.append(
            "nfr.md's module-size budget lists a path that is no longer past "
            f"~{MODULE_LINES} lines, or no longer there. The budget only shrinks: "
            "drop the path in the commit that split or moved it:\n    "
            + "\n    ".join(stale)
        )
    return problems

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
    for line in table_rows(text[header:]):
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        kinds.append((cells[0], cells[-1]))

    if not kinds:
        return [
            "performance.md's taxonomy table has no rows, so the count above it "
            "holds nothing. A kind that stopped being a kind is retired by "
            "arguing it in that section, not by emptying the table"
        ]

    problems = []
    expected = claimed(
        text,
        TAXONOMY_CLAIM,
        problems,
        "performance.md",
        gone=(
            "how many of the kinds below its taxonomy run today, so a row can "
            "be added or dropped without failing this gate. The sentence is "
            "held as written -- rewording it means rewording TAXONOMY_CLAIM in "
            "the same commit"
        ),
        count="count of measurement kinds",
    )
    if expected is not None and expected != len(kinds):
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
# Relative to the reading file: the three `include` macros, and a module's
# `#[path]`.
INCLUDED = re.compile(
    r'include(?:_bytes|_str)?\s*!\s*\(\s*"([^"]*)"|#\[\s*path\s*=\s*"([^"]*)"\s*\]'
)
# Relative to the package: `env!("CARGO_MANIFEST_DIR")`, alone or as the head of
# a `concat!`, closing whatever `Path::new(` wraps it.
MANIFEST_DIR = r'env\s*!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)'
MANIFEST_ROOT = re.compile(
    r'(?:concat\s*!\s*\(\s*' + MANIFEST_DIR + r'\s*,\s*"(?P<concat>[^"]*)"\s*\)|'
    + MANIFEST_DIR + r")(?:\s*\))*"
)
# What a path is walked through: `.join("…")`, `.parent()`, and the
# `.unwrap()`/`.expect(…)` a `parent()` is opened with.
STEPS = re.compile(
    r'(?:\s*\.\s*(?:join\(\s*"[^"]*"\s*\)|parent\(\s*\)|unwrap\(\s*\)|expect\([^)]*\)))*'
)
STEP = re.compile(r'\.\s*(?:join\(\s*"([^"]*)"\s*\)|(parent)\(\s*\))')
# A `let` the walked path is bound by, read back from where the path starts.
BOUND = re.compile(
    r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=\s*(?:&\s*)?(?:[\w:]+\s*\(\s*)?\Z"
)


def manifest_reads(text):
    """Every path `text` walks from its package's `CARGO_MANIFEST_DIR`.

    Each is relative to the package, `..` standing for a `parent()`. A `let`
    binding one carries it on: a name bound to one is a root in its own right
    from there on, as far as `.join` and `.parent()` walk it, so a climb split
    across a binding is still the climb it is.
    """
    bound, reads, i = {}, [], 0
    while True:
        found = MANIFEST_ROOT.search(text, i)
        name = (
            re.compile(r"(?<![\w.:])(" + "|".join(map(re.escape, bound)) + r")\b").search(text, i)
            if bound
            else None
        )
        if found is None and name is None:
            return reads
        if found is None or (name is not None and name.start() < found.start()):
            start, base, end = name.start(), bound[name.group(1)], name.end()
        else:
            concat = (found["concat"] or "").lstrip("/")
            start, base, end = found.start(), [concat] if concat else [], found.end()
        steps = STEPS.match(text, end)
        walked = list(base)
        for joined, parent in STEP.findall(steps.group(0)):
            # `join` with an absolute path replaces what it was joined to.
            walked = [joined] if joined.startswith("/") else walked + [".." if parent else joined]
        reads.append("/".join(walked) or ".")
        if binding := BOUND.search(text[max(0, start - 200) : start]):
            bound[binding.group(1)] = walked
        i = max(steps.end(), end, start + 1)
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
# The whole use tree, so a brace group's heads are read as well as a bare one.
REEXPORT = re.compile(r"^[ \t]*pub\s+use\s+([^;]*);", re.MULTILINE)
USE_TOKEN = re.compile(r"\w+|::|[{},*]")


def use_paths(tree):
    """Every path a `use` tree imports, with `::` joining its segments.

    `a::{b, c::{self, d as e}}` -> `a::b`, `a::c`, `a::c::d`; a glob ends in
    `*`, an alias is dropped and a leading `::` is not kept.
    """

    def group(tokens, i, prefix):
        paths = []
        while i < len(tokens) and tokens[i] != "}":
            found, i = branch(tokens, i, prefix)
            paths += found
            if i < len(tokens) and tokens[i] == ",":
                i += 1
        return paths, i + 1

    def branch(tokens, i, prefix):
        path = list(prefix)
        while i < len(tokens) and tokens[i] not in (",", "}"):
            token = tokens[i]
            if token == "{":
                return group(tokens, i + 1, path)
            if token == "as":
                i += 2
                break
            if token == "*" or (token != "::" and not (token == "self" and path)):
                path.append(token)
            i += 1
        return (["::".join(path)] if path else []), i

    return group(USE_TOKEN.findall(tree), 0, [])[0]


# --- Placeholder bodies -----------------------------------------------------
# No `todo!` or `unimplemented!` outside doc examples, which `strip()` removes.
PLACEHOLDER = re.compile(r"\b(todo|unimplemented)\s*!")


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
    # `(sites, names)` per row whose *Names* cell this rule enforces, and every
    # row's sites together.
    named_rows, allowed = [], set()
    if table is not None:
        for line in table_rows(table):
            cells = line.split("|")
            row = re.findall(r"`([^`]+)`", cells[1])
            rows.append(row)
            sites = {KYNOS_SOURCE + site.lstrip("/") for entry in row for site in expand(entry)}
            allowed |= sites
            if sites == {HOME_SITE}:
                continue
            names = runtime_names(cells[2])
            if names is None:
                failures.append(
                    f"architecture.md's allowance table lets {', '.join(sorted(sites))} "
                    f"name {cells[2].strip()}, which this rule cannot read as "
                    "backticked `tokio::` use trees. The row allows every tokio item "
                    "until it can, so its sites go unchecked meanwhile"
                )
                continue
            named_rows.append((sites, names))

        stated = claimed(architecture, ROW_COUNT_CLAIM, failures)
        if stated is not None and stated != len(rows):
            failures.append(
                f"architecture.md's allowance table claims {stated} rows and has {len(rows)}"
            )

        if offenders := sorted(p for p in corpus.naming("tokio") if not permitted(p, allowed)):
            failures.append(
                "`tokio` is named outside `server/` at a site the allowance table does "
                "not list:\n    " + "\n    ".join(offenders)
            )

        unlisted = []
        texts = dict(corpus.files)
        for path in sorted(corpus.naming("tokio")):
            covering = [names for sites, names in named_rows if allowed_at(path, sites)]
            if not covering:
                continue
            unlisted += [
                f"{path}: {item}"
                for item in runtime_paths(texts[path])
                if not any(admits(names, item) for names in covering)
            ]
        if unlisted:
            failures.append(
                "a site the allowance table lists names a tokio item its row's Names "
                "cell does not. The cell is the claim of what the site couples to the "
                "runtime, so either it grows with the argument for the item, or the "
                "site stops naming it:\n    " + "\n    ".join(unlisted)
            )

    # --- The dependency graph ------------------------------------------------
    for crates, where, description in dependency_rules(nfr, failures):
        if stray := sorted(f for f in corpus.naming(*crates) if not listed(f, where)):
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
    for line in table_rows(halves[1] if len(halves) == 2 else ""):
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
                "nothing and the row narrows back to " + KYNOS_SOURCE + " without "
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
        expected = claimed(
            testing,
            ROW_COUNT_CLAIM,
            failures,
            "testing.md",
            gone=(
                "how many rows its off-path table has, so a row can be dropped "
                "without failing this gate"
            ),
            count="off-path row count",
        )
        if expected is not None and expected != off_path_rows:
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
        if corpus.raw[path].count("\n") > MODULE_LINES
    )

    budget = section(
        nfr,
        BUDGET,
        failures,
        "\n## ",
        unrun="no module's size is held against the list of those allowed past the line",
    )
    if budget is not None:
        failures += budget_failures(BUDGETED.findall(budget), oversized)

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
        for line in table_rows(grading):
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
            reached = [
                (source.parent, included or attributed)
                for included, attributed in INCLUDED.findall(text)
            ] + [(package, walked) for walked in manifest_reads(text)]
            # A bound root is read once per use; each climb is reported once.
            for base, literal in sorted(set(reached)):
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
        heads = {
            imported.split("::")[0]
            for tree in REEXPORT.findall(text)
            for imported in use_paths(tree)
        }
        reexports += [f"{path}: pub use {head}::..." for head in sorted(heads & own)]

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
