"""Containment checks: a crate may be named only where its owner allows.

`docs/architecture.md` states the allowances and says twice that the count
is the check. This is that check. It reads the tables rather than restating
them, so the document and the gate cannot drift apart.

Source is stripped of comments and string literals before anything is
matched -- these rules are discussed in prose throughout `src`, and the
`b"h2"` ALPN identifier is a literal -- and `#[cfg(test)]` modules are
dropped, both inline and as sibling files.

One rule needs the literals back: a `#[cfg(feature = "x")]` gate writes the
flag name as a string, so it is matched over a second corpus that keeps
literals and drops everything else the first drops. Comments go from both.
A rule matched over genuinely raw text reads a renamed flag off a stale
comment and reports the row as holding.
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
# The counts a document may state in words. A count this dict cannot read
# fails loudly where it is read, rather than skipping the check that holds
# it, so extending a table past the last word here is part of writing the
# rows.
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
failures = []


CHAR_LITERAL = re.compile(r"'(\\.|[^\\'])'")
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*")


def blank(text):
    """`text` with every character but its newlines replaced by a space.

    What the corpus that drops literals puts in place of one. Blanked rather
    than deleted so that the two corpora one scan produces stay the same
    length, character for character: a span found in either is a span in both,
    which is what lets `#[cfg(test)]` modules be located once. The newlines
    survive so a blanked literal leaves the lines around it where they were.
    """
    return "".join("\n" if char == "\n" else " " for char in text)


def literal_end(source, i):
    """Where the literal starting at `i` ends, or `None` if none starts there.

    Raw strings, ordinary strings and char literals, which the scanner has to
    walk rather than skip: a `//` inside one is not a comment and a `"` inside
    a raw string is not its close. A bare `'` that does not close is a lifetime
    rather than a char literal, and is ordinary code.
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

    Found once, over the text whose literals are blanked, and applied to both
    corpora -- which is the whole reason the two are the same length. The end
    of a braced module is where the braces balance, and a brace inside a string
    is not a brace: counted over text that keeps its literals, a `"{"` in a
    test module means the depth never returns to zero and every line after the
    module is dropped, so a gate written below the tests is invisible; a `"}"`
    balances one brace early, and the tail of the test module survives into a
    corpus that is supposed to hold only what a request can run.

    Outermost spans only. A nested module is already inside its parent's, and
    returning both would have the caller cut one and shift the other.
    """
    spans = []
    for match in TEST_MODULE.finditer(text):
        if spans and match.start() < spans[-1][1]:
            continue
        rest = text[match.end() :]
        if not rest.startswith("{"):
            # `mod tests;`, whose body is the sibling file `under_test` holds
            # out of `FILES` separately.
            spans.append((match.start(), match.end() + rest.startswith(";")))
            continue
        depth, end = 0, len(text)
        for k, char in enumerate(rest):
            depth += (char == "{") - (char == "}")
            if depth == 0:
                end = match.end() + k + 1
                break
        # Braces that never balance are a file that does not compile, and the
        # module runs to the end of it. A literal can no longer make one.
        spans.append((match.start(), end))
    return spans


def strip(source, literals=True):
    """Drop comments and `#[cfg(test)]` modules, and literals unless asked.

    A hand-rolled scanner rather than a set of regexes, because the three
    constructs nest: `"//"` is not a comment, `'"'` is not a string, and
    Rust's block comments nest inside each other. Getting any of those
    wrong desynchronises the scan and silently inverts what survives.

    `literals=False` keeps every literal and drops the rest, which is the
    corpus a `#[cfg(feature = "x")]` gate is read against: the flag name is a
    string, so a rule that dropped literals would find no gate anywhere, and
    one matched over raw text would find one in a comment.

    One scan produces both, because the two differ in the literals alone: a
    literal is kept verbatim in one and blanked to the same width in the other,
    so the two are the same length and the `#[cfg(test)]` modules `strip` drops
    are located once, on the text where a brace is only ever a brace.
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

SOURCES = [(path, strip(text)) for path, text in RAW_SOURCES]


def under_test(path):
    name = path.rsplit("/", 1)[-1]
    return name == "tests.rs" or name.endswith("_tests.rs")


# What every rule here is stated over: the code that a request can run. Test
# modules are dropped, inline by `strip` and as sibling files here.
FILES = [(path, text) for path, text in SOURCES if not under_test(path)]

# The same two corpora for the one rule that needs a string literal back. A
# `#[cfg(feature = "x")]` gate writes the flag name as a literal, so the corpus
# above cannot see one at all; raw text can, but it also sees the flag named in
# every comment, every rustdoc example and every `#[cfg(test)]` module beside
# the code -- and a gate found in one of those holds a row up on a mention no
# build reads, which is the false-positive class `strip()` exists to remove. So
# this pair drops exactly what the pair above drops, minus the literals.
GATE_SOURCES = [(path, strip(text, literals=False)) for path, text in RAW_SOURCES]

GATE_FILES = [(path, text) for path, text in GATE_SOURCES if not under_test(path)]

# What a failure says a spelling was looked for in, keyed by whether it is a
# gate. Two corpora are two claims, and a message naming neither leaves a
# reviewer guessing which text the rule read.
CORPUS = {
    False: "with comments, string literals and inline `#[cfg(test)]` modules removed",
    True: "with comments and inline `#[cfg(test)]` modules removed and string literals kept",
}


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
# The scope every row is read against, and the one a bare site is relative to.
# A row reaches further by writing a `crates/...` site: the scope it is checked
# under is derived from its own sites, below, so widening is a property of the
# row that needs it rather than of the whole table. The alternative -- one scope
# spanning both crates for every row -- fails the `Document` row on sight, since
# `kynos-openapi` is where the type is declared.
OFF_PATH_SCOPE = "crates/kynos/src/"


# What one spelling in a *Named by* cell may hold: an identifier, or a path of
# them. The backticks are load-bearing as soon as a cell holds more than one
# spelling -- they are what separates them, and `backticked` refuses a cell that
# writes anything but commas outside them -- and are optional here only so that
# a cell written as a single bare token is still read whole.
NAMED_BY = re.compile(r"`?(\w+(?:\s*::\s*\w+)*)`?")
# The other kind: a Cargo feature gate, written as the `#[cfg]` attribute writes
# it. A flag is not an identifier -- `decimal-big` is not even a Rust name -- so
# an element whose whole contribution is what a gate compiles has no crate or
# type to be named by, and the gate is the only thing that names it.
#
# Matched as text, so it cannot tell a gate from its negation or from a
# `cfg_attr` that compiles nothing: #134. The `openapi31` row is why that is not
# a one-line narrowing -- both of its sites are
# `#[cfg(not(feature = "openapi31"))] compile_error!`, so a pattern that reads
# only the positive form empties the one row that has nothing else to hold.
GATE = re.compile(r'`?feature\s*=\s*"([\w-]+)"`?')


BACKTICKED = re.compile(r"`([^`]+)`")
RESIDUE = re.compile(r"[\s,]*")


def backticked(cell):
    """The backticked entries of one cell, or `None` if anything else is in it.

    Both cells below are comma-separated lists of backticked entries, and both
    are read by collecting the runs. Collecting them is not enough on its own:
    a list of two whose second entry lost its backticks collects as a list of
    one, and every rule downstream then holds a row up by the half of it that
    still parses. So the residue is checked too -- outside the runs a cell may
    write commas and whitespace and nothing else.

    A cell with no backticks at all is a different case and is not refused
    here. `None` means a run was found *and* something outside the runs was;
    a bare cell returns an empty list, and the caller reads it whole and fails
    loudly there if it is not a token or a path.
    """
    entries = BACKTICKED.findall(cell)
    if entries and not RESIDUE.fullmatch(BACKTICKED.sub("", cell)):
        return None
    return entries


def token(cell):
    """One `(spelling, regex, gate)` per spelling in a *Named by* cell, or `None`.

    `None` loudly rather than a pattern that cannot match: a cell this function
    guesses at compiles to an escaped literal nothing in Rust source contains,
    and a rule that always passes reports that the elements are off the path
    when nobody has checked. A new kind of token belongs in `NAMED_BY` and here,
    not in a fallback. A cell writing anything but commas outside its backticks
    is `None` for the same reason and by `backticked`: a spelling silently
    dropped for having lost its backticks is a claim nobody is holding, and it
    reads exactly like a row with one spelling that holds.

    `Registry::new` is a path rather than an identifier, and the source may
    write it spaced or wrapped, so each `::` matches the whitespace a formatter
    is free to put around it.

    A cell may hold more than one spelling -- a comma-separated list of
    backticked entries, each of which may brace-expand the way a *Named only in*
    cell expands a directory of siblings -- and the row holds all of them at
    once. `Registry::{new,default}` is the case that forced the brace form:
    `new` is `Self::default()` and `Registry` derives `Default`, so a row
    holding only `new` lets a derived `default()` mint a registry anywhere with
    the gate green. The comma form is what a feature row needs: a flag's
    contribution is the code its gate compiles *and* the crate that code calls,
    and `` `uuid`, `feature = "uuid"` `` is one element with two names, not two
    elements sharing a reason written twice.

    Which corpus a spelling is matched over is `gate` in each triple: an
    identifier over `SOURCES`, a gate over `GATE_SOURCES`. The two differ in
    the literals alone. A gate matched over the stripped text would name
    nothing anywhere, since a flag name is a literal and the rule would read
    every `#[cfg]` in the workspace as absent and every feature row as
    vacuously held; a gate matched over raw text would be satisfied by the
    flag named in a comment, a rustdoc example or an inline test module, and a
    renamed flag would go on reporting that its row holds.

    Each spelling keeps its own pattern rather than joining them into one
    alternation, so the caller can hold every spelling to naming a file. A
    union hides a stale spelling behind a live one: `Registry::{new,defualt}`
    matches wherever `new` is written, and the row goes on reporting that a
    registry is off the path while `Registry::default()` mints one anywhere.
    """
    entries = backticked(cell)
    if entries is None:
        return None
    spellings = []
    for entry in entries or [cell]:
        for spelling in expand(entry.strip()):
            spelling = spelling.strip()
            if gate := GATE.fullmatch(spelling):
                flag = re.escape(gate.group(1))
                spellings.append(
                    (spelling, re.compile(r'feature\s*=\s*"' + flag + r'"'), True)
                )
                continue
            readable = NAMED_BY.fullmatch(spelling)
            if readable is None:
                return None
            segments = [re.escape(part.strip()) for part in readable.group(1).split("::")]
            pattern = r"\s*::\s*".join(segments)
            spellings.append(
                (readable.group(1), re.compile(r"\b" + pattern + r"\b"), False)
            )
    return spellings


def allowed_sites(cell):
    """The files one *Named only in* cell allows.

    A comma-separated list of backticked paths, each of which may brace-expand:
    a row naming nine files is one cell, and `router/{describe,install}.rs` is
    the same shorthand the allowance table above uses.

    A path is relative to `OFF_PATH_SCOPE` unless it starts at `crates/`, which
    makes it relative to the repository root and is how a row names a sibling
    crate. The prefix is the whole of the distinction on purpose: it is what a
    reader of the table already has to type to say where the file is, so a row
    reaching into another crate cannot be written without saying so.

    `None` when the cell writes anything but commas outside its backticks, by
    `backticked` and for a consequence worse than a lost spelling: the scan
    scope is derived from these sites, so a `crates/...` site dropped for
    having lost its backticks does not merely go unchecked -- it takes the
    sibling crate out of the scan with it, and the row narrows silently back
    to the home scope where every spelling it names is still written.
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

    Always the home scope, plus one tree per crate-qualified site. Derived
    rather than declared, so the widening and the reason for it are the same
    edit: a row that names `crates/kynos-openapi/src/emit/mod.rs` has said
    where its element lives, and a second cell repeating that as a scope could
    only ever disagree with the first.

    The narrowness is the point. Scanning both crates for every row would fail
    the `Document`, `Registry` and `Validator` rows immediately -- all three
    elements are *declared* in `kynos-openapi`, and the claim those rows make
    is about the crate a request runs in.

    A derived tree is held to existing because the derivation is string
    surgery over a hand-written cell, and a crate name is not spell-checked by
    anything else here. `crates/kynos-opanapi/src/emit/mod.rs` is backticked,
    is `crates/`-prefixed, and reads as a path, so every other check on the
    cell passes -- and the tree it yields matches no file, which silently
    narrows the row back to the home scope where the element it names is not
    written at all. Both halves of the row then pass: the spelling is found in
    the home crate, and nothing in the sibling crate is scanned for offenders.
    One transposed letter takes a whole crate out of the gate while the run
    reports that every rule holds, so the tree is checked rather than trusted.

    `exists` is the directory test, injected so the rule can be exercised
    against a stated tree set rather than the repository's own.
    """
    if exists is None:
        exists = lambda tree: (ROOT / tree).is_dir()
    trees = {OFF_PATH_SCOPE}
    for site in allowance:
        parts = site.split("/")
        if len(parts) > 3 and parts[0] == "crates" and parts[2] == "src":
            trees.add("/".join(parts[:3]) + "/")
    return sorted(trees), sorted(tree for tree in trees if not exists(tree))


halves = TESTING.split(OFF_PATH_HEADER)
if len(halves) != 2:
    failures.append(
        "testing.md no longer holds exactly one off-path table under the header "
        "this rule reads, so nothing states which elements a request may not "
        "reach"
    )

off_path_rows = 0
# Every backticked token an *Element* cell writes, which is where a row says
# which flag it is the proof for. Read from the same rows the rest of this loop
# checks, so a row cannot satisfy the grading below without also being held
# above: the feature grading compares against this set further down.
off_path_elements = set()
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

    # Every spelling is held to naming something, one at a time rather than as
    # a union. A union hides a stale spelling behind a live one: with the cell
    # written `Registry::{new,defualt}`, `new` keeps the row's match set
    # non-empty and the typo is swallowed, so the row goes on reporting that a
    # registry is off the request path while `Registry::default()` mints one
    # anywhere. A spelling is a claim about a name, and a name nothing in the
    # workspace writes is a rename or a typo rather than an element nothing
    # reaches. (Stale *sites* stay tolerated, deliberately -- subset semantics,
    # as above -- because a site claims a location, and locations may empty out
    # while the claim stays true.)
    #
    # Existence is asked of `SOURCES`, not `FILES`: a mint spelling earns its
    # place in a cell by being reachable, not by being reached, so the row is at
    # its strongest when no file a request can run writes it at all.
    # `Registry::default` is that case -- it is written only in a sibling test
    # file, which is exactly the row holding.
    #
    # Sibling test files, and not every test: `strip()` has already dropped the
    # inline `#[cfg(test)] mod` bodies from `SOURCES` and from `GATE_SOURCES`
    # alike, so the corpus this widens to is exactly the `tests.rs` siblings
    # `under_test` holds out of `FILES`. That is the layout rule's corpus rather
    # than an approximation of it -- a module's tests belong in a sibling -- and
    # lifting the inline removal would re-admit the comment and literal mentions
    # `strip()` exists to drop.
    #
    # A gate spelling is asked of `GATE_SOURCES` for the reason `token` gives:
    # the flag name is a string literal, invisible in the stripped text, so
    # that corpus is the same source with its literals kept. Its comments and
    # inline test modules go all the same -- a gate is a claim about code a
    # build compiles, and a flag named in a comment is not one.
    stale = [
        (spelling, gate)
        for spelling, pattern, gate in spellings
        if not any(
            any(path.startswith(tree) for tree in trees) and pattern.search(text)
            for path, text in (GATE_SOURCES if gate else SOURCES)
        )
    ]
    if stale:
        for spelling, gate in stale:
            failures.append(
                f"testing.md's off-path table names {element} with "
                f"`{spelling}`, and nothing under {scope} writes that spelling "
                f"{CORPUS[gate]}, sibling test files included. The row holds "
                "nothing under it: either the element was renamed and the cell "
                "was not, or it now lives outside the scope this row's own "
                "sites reach, which is a site to add rather than a spelling to "
                "keep. The row's sites go unchecked until the cell is repaired"
            )
        # One failure per row. A cell this rule has just called untrustworthy
        # does not also get to render a verdict on the sites: the offender scan
        # under a stale spelling reports against a match set nobody should
        # believe, and a reviewer handed two failures repairs the second by
        # editing the row the first says is already wrong. The message above
        # says the sites went unchecked, so the skip is stated rather than
        # inferred from a passing build.
        continue

    # The offender scan, over the request-runnable half of each corpus a
    # spelling asked for -- the same corpus its existence was asked of, so a
    # row cannot be held up by a mention the offender scan would not have
    # counted. A file counts as naming the element if any one spelling matches
    # it, in that spelling's own text.
    named = sorted(
        {
            path
            for _, pattern, gate in spellings
            for path, text in (GATE_FILES if gate else FILES)
            if any(path.startswith(tree) for tree in trees) and pattern.search(text)
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

# The row *set* needs holding as well as the rows. Every check above runs per
# row, so a row that is deleted -- or cut off early, which one blank line in
# the middle of the table does, since the loop breaks on the first line that is
# not a row -- takes its element out of the gate while the run still reports
# that every rule holds. `testing.md` states the count for that reason, and
# this compares it.
elif len(halves) == 2:
    stated = re.search(r"\*\*(\w+) rows, and the count is the check\.\*\*", TESTING)
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
# One grade is not self-executing. A full battery is owed to a suite that either
# runs or does not; an aggregate owes nothing. An off-path proof is an argument,
# and the failure it has is the one every argument has -- being graded and never
# written. So the flags in this row are held to appearing in the off-path table
# above, and regrading a flag into this column is a failing build until its row
# exists. Forward only: a row for a flag graded elsewhere is not an error, since
# an element may be worth holding under any grade.
OFF_PATH_GRADE = "Off-path proof"


def off_path_coverage(off_path_graded, off_path_elements, grades):
    """What the off-path table fails to cover of what performance.md graded.

    A function rather than two statements in the rule body, because this is
    the one comparison here whose inputs are both parsed and whose own failure
    mode is silence: the check reads a grade by name, and a name that is gone
    empties the compared set instead of emptying the table. Written inline it
    was reachable only by running the gate against the real two documents, so
    `and False` on the guard below left every case green and the gate at zero.

    `grades` is every grade the table writes, and is what the name is checked
    against: renaming the grade in `performance.md` and not here would pass
    every flag in it silently -- a rule about the argument nobody wrote,
    itself passing because nobody wrote the grade.
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


graded, off_path_graded, grades = [], [], []
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

# --- The count of measurement kinds ------------------------------------------
# `performance.md` opens by saying how many of the kinds in its taxonomy run
# today, and until this rule nothing read that sentence. It went stale twice in
# one day (#130): two branches rewrote the same count from two readings of the
# same five rows, and the gate stayed green through both. There is no Markdown
# linter here, so prose and the table it summarises can only agree by someone
# recounting the rows by hand -- which is the check this replaces.
#
# The first of the two numbers is held and the second is not. Whether a kind
# runs is the Status cell opening `in use`, which is countable. Whether it
# covers only part of what it names is a concession the rest of that cell makes
# in prose, and no token separates the four cells that make one from the one
# that does not: an em dash misses the off-path row, a semicolon and `no ` miss
# the size guard, and `rather than` matches the allocation count, whose only
# qualifier explains why it has four targets rather than limiting what it
# covers. Every candidate was run against the five cells before this said so. A
# conjunction fitted to today's five cells would pass because it was drawn
# around them and would fail the first honest rewording, which is worse than an
# unheld number that the document admits is unheld. Holding it would take a
# signal in the table -- a column, or a marker in the cell -- rather than a
# cleverer regex over the same prose.
#
# The sentence is held as written, `All` included. Rewording it past this
# pattern fails loudly rather than silently unholding the count, so a taxonomy
# that stops running in full is a deliberate edit here and in the document
# together.
TAXONOMY_HEADER = "| Kind | Lives in | Runs under | Proves | Status |"
TAXONOMY_CLAIM = r"All (\w+) of the kinds below run today"
# What a Status cell opens with when the kind it grades runs. A prefix rather
# than a search, so that a cell reading `not in use` -- or one conceding a limit
# by naming what is *not* in use -- is read as the kind not running.
RUNNING = "in use"


def taxonomy_failures(text):
    """Whether `performance.md`'s opening count matches its taxonomy table.

    Text in, problems out, for `cargo_config_failures`' reason: the rule is
    what needs the cases, and this repository's own document is one input to it
    rather than the definition of it. The header is searched for rather than
    sliced at, so a renamed column is a failure here instead of a traceback
    that takes the test run with it.
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
        # Loudly, rather than skipping the check, for the reason NUMBERS gives.
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


failures += taxonomy_failures(PERFORMANCE)

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

# --- The dev build profile ---------------------------------------------------
# `.cargo/config.toml` is what keeps a worktree's `target/` near the 17 GiB
# `nfr.md`'s Workspace table records rather than the 44 GiB before it, and until
# this rule nothing observed it at all. Every task in `mise run check` and every
# CI job compiles the same code whether the file is there or not -- only slower,
# and onto four times the disk -- so deleting it, losing it to a merge, or
# misspelling a key inside it is a change no gate here could see.
#
# Cargo will not see it either, which was run rather than assumed. Against a
# scratch package, a misspelled key is `warning: unused config key
# profile.dev.debgu` and an exit status of zero, and a misspelled profile
# *table* -- `[profile.dve]` -- is not reported at all. Both builds finish
# `unoptimized + debuginfo`. The failure this catches is the one that looks
# exactly like a passing build.
#
# Presence, not value. `debug = 2` written over `line-tables-only` is a
# one-token diff on a line whose comment prices six alternatives against each
# other, and a reviewer reads it; a key that has lost a letter is what nobody
# reads. Pinning the value would also turn the file's own escape hatch --
# `cargo --config 'profile.dev.package."*".debug=2'`, offered there for reading
# a panic through `hyper` -- into a setting someone has to argue with a gate
# about. So the size itself stays unmeasured and the row in `nfr.md` says so:
# this holds the cause, and `du -sh target` is the effect nothing reads.
CARGO_CONFIG = ROOT / ".cargo/config.toml"
# Each key as a path through the parsed document, with the spelling a failure
# names it by -- `"*"` is a table name rather than an identifier, and the
# difference is what the file's own comment turns on: `CARGO_PROFILE_DEV_DEBUG`
# spells the first key and there is no environment spelling of the second.
FOOTPRINT_KEYS = [
    (("profile", "dev", "debug"), "profile.dev.debug"),
    (("profile", "dev", "package", "*", "debug"), 'profile.dev.package."*".debug'),
]
# And nothing else at the top level. Cargo consults this file on every
# invocation anywhere under the repository, so a `[build] rustc-wrapper` or a
# `[source] replace-with` written here runs on every machine that builds the
# workspace and in every CI job, with nothing reading it -- while the file's own
# comment justifies its existence by the dev profile alone. A tool setting
# belongs in the `env` of the mise task that needs it, where the task names it.
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

    `None` for a file that is not there, which is a case rather than an error:
    a merge that drops it leaves a tree where every other rule here holds.

    Unparseable TOML is reported rather than raised, for the reason the guard
    at the foot of this file gives -- a rule that takes the process down takes
    the test run with it, and reports the parsers as untested exactly when
    something they read is what broke.
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


failures += cargo_config_failures(
    CARGO_CONFIG.read_text() if CARGO_CONFIG.is_file() else None
)


# --- Report -----------------------------------------------------------------
# Under the guard so this module can be imported. `containment_test.py` holds
# the parsers above to their own cases, and a module that exits on a broken
# rule would take the test run down with the tree it was reading -- reporting
# the parsers as untested exactly when a parser is what broke. The rules
# themselves still run on import, which costs a few file reads and no build.
#
# Which is only half of what the guard is meant to buy, and #134 is the rest:
# the three bare `.index()` slices above raise rather than failing, so renaming
# `| Grade | Owes | Flags |` in `performance.md` kills this script *and* the
# test run that would have reported it. The fix is a `def main()`, which
# reindents every rule body and so does not belong on a branch changing what
# the rules say.
if __name__ == "__main__":
    for failure in failures:
        print(f"containment: {failure}", file=sys.stderr)
    if failures:
        sys.exit(1)
    print(
        f"containment: {len(FILES)} source files, {len(rows)} allowance rows, "
        f"{off_path_rows} off-path rows, {len(graded)} graded features, every rule holds"
    )
