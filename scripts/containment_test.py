"""Tests for the parsers `containment.py` reads its tables and sources with, for
the rules stated over them whose own failure is silence, and for the shape that
keeps a broken document from taking this run down with it.

Most of what is under test here takes text and returns a corpus, a set of paths,
a list of patterns or a slice. Running the rules against the intact tree is what
`containment:check` is, and this file does not repeat it.

Three rules are here all the same, each for its own reason.
`off_path_coverage` compares two documents and reads a grade out of one by
name, so a name that has gone empties the compared set rather than the table --
the failure mode a parser has, in a rule. `cargo_config_failures` reads a
configuration file that nothing else in this repository observes, and the
mistake it exists to catch is one cargo itself reports as a warning over a
successful build, or does not report at all. `taxonomy_failures` holds a
sentence against the table below it, and nothing here reads Markdown prose. All
three have their inputs stated below rather than read off disk, since what is
under test is what the rule does with a document or a config and not what this
repository's own happen to say.

`Main` is the exception to that, deliberately. What each rule *stops* checking
when the marker it slices at is gone is a decision `main` makes four times over,
and it is a decision about the real tree: an emptied allowance reports every
file in the crate, and a widened slice authorises a hand-rolled `Stream` off any
link in the document. So those cases hand `main` this repository's own documents
with one marker removed, which `main` takes as arguments for that purpose, and
assert which rules ran rather than counting failures.

`ImportTime` is not about a parser or a rule at all. It holds the file to
running neither at import, which is what keeps a document nobody can read from
killing the gate and this run with it.

This docstring is the testing standard for this file, and for a gate script's
tests generally, and deliberately.
`docs/testing.md` allocates a method to five kinds of *Rust* code and names
`containment.py` only as a consumer of the tables it holds; this file has never
appeared in it, and neither has `cost_features_test.py`, which is held by its
own gate and says nothing about this one. A gate script is tooling
rather than shipped surface, and its tests are held by the gate that runs them,
so there is no row here to write and none owed.

The parsers are where a regression is silent. A rule that breaks reports a
failure and exits one; a parser that breaks drops a spelling, a site or a whole
region of a file and the gate still prints `every rule holds` -- the shape
`containment.py` names as the worst outcome it has, "a rule that always passes
reports that the elements are off the path when nobody has checked". Each case
below is a mutation the real tree happens not to contain: a cell whose second
token lost its backticks, a site that would have opened a sibling crate, a brace
inside a string in a test module. Every one of them passed the gate green.

The Rust fragments are written for this file rather than captured. They are the
minimum that reaches a branch: what a corpus is asked is only whether a spelling
appears in it, so a fragment needs to be recognisable Rust and no more.

Run it as `mise run containment:test`, or directly. There is no Python test
runner in this repository and `unittest` needs none.

What it costs, said here because `hooks:pre-push` runs it and nothing else
states a ceiling. Almost all of the time is `Main`: each of its cases calls
`main()`, and each call re-runs every rule over the real tree, which is the
rule half of what `containment:check` itself costs. The suites above it are
free beside that -- they hand a parser some text and read what comes back,
and the corpora they read are built once, at import. A `Main` case that
rewrites one file of the corpus is priced with the rest of them:
`Corpus.replacing` strips the file it rewrites and shares the other three
hundred. So a new `Main` case is priced at roughly one more run of the gate,
and a suite that has stopped being affordable is shortened at `Main` rather
than at the parsers.
"""

import contextlib
import io
import re
import sys
import tempfile
import unittest
from pathlib import Path

# Before the import below, and before anything else can trigger one: a run
# writes no `.pyc` into the tree it reads, and `.gitignore` covers the imports
# no task controls. The task passes `-B` for the same reason; this covers a
# direct `python3` run.
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))

import containment as gate  # noqa: E402  (the path insert must come first)

# A feature gate as `#[cfg]` writes it, which is the one construct that has to
# survive into the literals-kept corpus: the flag name is a string.
GATE = '#[cfg(feature = "uuid")]\nfn probe() {}\n'

# Live code, so every fixture has something a rule could legitimately find.
LIVE = "pub fn dispatch() {}\n"


def with_tests(body, before=LIVE, after=GATE):
    """A file with an inline `#[cfg(test)]` module between two live regions."""
    return f"{before}\n#[cfg(test)]\nmod tests {{\n{body}\n}}\n\n{after}"


class InlineTestModules(unittest.TestCase):
    """Where an inline `#[cfg(test)] mod` ends, in both corpora.

    The brace counter that finds the end is the whole of the answer, and it runs
    over a corpus that keeps string literals. A literal brace it takes for code
    either never closes the module -- deleting every line after it, so an
    offender past the tests is invisible -- or closes it early, so test-only code
    is reported as a request-path offender.
    """

    def kept(self, source):
        return gate.strip(source, literals=False)

    def dropped(self, source):
        return gate.strip(source, literals=True)

    def test_an_open_brace_in_a_test_literal_does_not_swallow_the_rest(self):
        source = with_tests('    const OPEN: &str = "{";')
        self.assertIn('feature = "uuid"', self.kept(source))

    def test_an_open_brace_in_a_test_char_literal_does_not_either(self):
        source = with_tests("    const OPEN: char = '{';")
        self.assertIn('feature = "uuid"', self.kept(source))

    def test_an_open_brace_in_a_raw_string_does_not_either(self):
        source = with_tests('    const OPEN: &str = r#"{"#;')
        self.assertIn('feature = "uuid"', self.kept(source))

    def test_a_closing_brace_in_a_test_literal_does_not_end_the_module(self):
        source = with_tests(
            '    let _brace = "}";\n'
            '    #[cfg(feature = "uuid")]\n'
            "    fn only_under_test() {}"
        )
        self.assertNotIn("only_under_test", self.kept(source))

    def test_a_closing_brace_in_a_test_char_literal_does_not_either(self):
        source = with_tests(
            "    let _brace = '}';\n"
            '    #[cfg(feature = "uuid")]\n'
            "    fn only_under_test() {}"
        )
        self.assertNotIn("only_under_test", self.kept(source))

    def test_the_module_goes_from_the_corpus_that_drops_literals_too(self):
        source = with_tests('    const OPEN: &str = "{";\n    fn helper() {}')
        self.assertNotIn("helper", self.dropped(source))
        self.assertIn("dispatch", self.dropped(source))

    def test_the_code_on_both_sides_of_the_module_survives(self):
        source = with_tests("    fn helper() {}")
        for text in (self.kept(source), self.dropped(source)):
            self.assertIn("dispatch", text)
            self.assertIn("probe", text)
            self.assertNotIn("helper", text)

    def test_a_nested_test_module_goes_with_its_parent(self):
        source = with_tests(
            "    #[cfg(test)]\n    mod inner {\n        fn nested() {}\n    }"
        )
        for text in (self.kept(source), self.dropped(source)):
            self.assertNotIn("nested", text)
            self.assertIn("probe", text)

    def test_a_declared_sibling_module_goes_without_its_semicolon(self):
        source = f"{LIVE}\n#[cfg(test)]\nmod tests;\n\n{GATE}"
        for text in (self.kept(source), self.dropped(source)):
            self.assertNotIn("mod tests", text)
            self.assertNotIn(";", text.split("dispatch")[1].split("#[cfg")[0])
            self.assertIn("probe", text)

    def test_a_gate_only_under_test_is_not_in_either_corpus(self):
        source = with_tests('    #[cfg(feature = "cookie")]\n    fn helper() {}')
        for text in (self.kept(source), self.dropped(source)):
            self.assertNotIn("cookie", text)


class Literals(unittest.TestCase):
    """Which corpus keeps a string, and what a string is."""

    def test_a_flag_name_is_a_literal_and_only_one_corpus_keeps_it(self):
        self.assertIn('feature = "uuid"', gate.strip(GATE, literals=False))
        self.assertNotIn("uuid", gate.strip(GATE, literals=True))

    def test_a_comment_goes_from_both_corpora(self):
        source = '// renamed from feature = "legacy"\n' + LIVE
        for literals in (True, False):
            self.assertNotIn("legacy", gate.strip(source, literals=literals))

    def test_a_nested_block_comment_goes_whole(self):
        source = "/* outer /* inner */ still comment */\n" + LIVE
        for literals in (True, False):
            text = gate.strip(source, literals=literals)
            self.assertNotIn("inner", text)
            self.assertIn("dispatch", text)

    def test_a_comment_marker_inside_a_string_is_not_a_comment(self):
        source = 'const PREFIX: &str = "// not a comment";\n' + LIVE
        self.assertIn("dispatch", gate.strip(source, literals=False))
        self.assertIn("not a comment", gate.strip(source, literals=False))
        self.assertIn("dispatch", gate.strip(source, literals=True))

    def test_a_lifetime_is_not_a_char_literal(self):
        source = "impl<'a> Body for Reader<'a> { fn poll() {} }\n"
        for literals in (True, False):
            self.assertIn("Reader", gate.strip(source, literals=literals))


class Section(unittest.TestCase):
    """The slice of a document a rule reads, and a marker that is no longer in it.

    Every rule below reaches its document by slicing at a heading, and a heading
    is a thing a document may be reworded past. `.index()` raises there, and a
    raise inside this module is not one failing rule: it is the gate exiting
    before it reports anything, and the import at the head of this file taking
    the whole test run down with it -- so the run that would have named the
    missing marker never starts. Reported and skipped instead, with the rule
    that goes unchecked named in the failure.

    The documents below are written for this file. What is under test is what
    the helper does with a marker that is there and one that is not, and reading
    this repository's own documents would make it a test of today's headings.
    """

    #: A marker with no regex meaning and every kind of whitespace a heading has.
    GRADING = "| Grade | Owes | Flags |"

    def test_a_renamed_heading_is_reported_rather_than_raising(self):
        failures = []
        self.assertIsNone(gate.section("# Doc\n\nprose\n", self.GRADING, failures))
        self.assertEqual(len(failures), 1)
        self.assertIn(self.GRADING, failures[0])

    def test_a_present_heading_yields_the_text_from_it(self):
        failures = []
        body = f"{self.GRADING}\n| --- | --- | --- |\n| Full battery | a suite | `uuid` |\n"
        self.assertEqual(
            gate.section(f"# Doc\n\nprose\n\n{body}", self.GRADING, failures), body
        )
        self.assertEqual(failures, [])

    def test_an_end_marker_truncates_at_it(self):
        failures = []
        document = f"# Doc\n\n{self.GRADING}\n| --- |\n\n## Next\n\nprose\n"
        self.assertEqual(
            gate.section(document, self.GRADING, failures, "\n## "),
            f"{self.GRADING}\n| --- |\n",
        )
        self.assertEqual(failures, [])

    def test_a_missing_end_marker_is_reported_rather_than_running_to_the_end_of_the_document(self):
        failures = []
        document = f"# Doc\n\n{self.GRADING}\n| --- |\n\nprose to the end\n"
        self.assertIsNone(gate.section(document, self.GRADING, failures, "\n## "))
        self.assertEqual(len(failures), 1)
        self.assertIn(repr("\n## "), failures[0])

    def test_the_first_of_two_headings_is_the_one_taken(self):
        failures = []
        document = f"{self.GRADING}\n| first |\n\n{self.GRADING}\n| second |\n"
        self.assertEqual(
            gate.section(document, self.GRADING, failures, "\n\n"),
            f"{self.GRADING}\n| first |",
        )
        self.assertEqual(failures, [])


class ImportTime(unittest.TestCase):
    """That importing `containment.py` runs no rule.

    The `if __name__ == "__main__":` guard at the foot of that file used to
    cover the report alone. Every rule body was a top-level statement, so the
    import at the head of this file ran the whole gate against the real tree
    before the first test started -- and the cost was not the file reads, which
    are cheap and sanctioned. It was that a rule raising over a reworded
    document took this run down with it, and reported the parsers as untested
    exactly when a parser was what broke.

    The first two cases below are proxies: they say where the rules live. The
    third is the property itself, because a proxy can be satisfied by a module
    that has moved its rules and kept one slice behind.
    """

    #: Every marker a rule slices or splits a document at. A document holding
    #: none of them is one no rule here can read, which is the tree #134 is
    #: about.
    MARKERS = (
        "| Site | Names | Why it is not in `server/` |",
        "### Public API surface",
        "\n## ",
        "| Grade | Owes | Flags |",
        "| Element | Named by | Named only in | Why a request cannot reach it |",
        "| Kind | Lives in | Runs under | Proves | Status |",
    )

    def test_the_module_holds_no_failure_list_at_import(self):
        self.assertFalse(hasattr(gate, "failures"))

    def test_every_rule_is_reachable_as_main(self):
        self.assertTrue(callable(getattr(gate, "main", None)))

    def test_importing_it_survives_documents_holding_none_of_those_markers(self):
        """The property the two above stand in for.

        Re-adding one module-level slice -- say
        `STALE = ARCHITECTURE[ARCHITECTURE.index("### Public API surface"):]` --
        leaves both of them green and every other case in this file green, and
        turns this one into the `ValueError` that killed the gate and this run
        with it. That is the whole of #134, and it is the one shape a structural
        assertion cannot see.

        The module is re-executed into a namespace of its own rather than
        reloaded, so the `gate` every other case here holds is untouched, and
        `sys.modules` is not written at all.

        This is the one stdlib attribute this branch rebinds by hand, rather
        than through a context manager that puts it back, and it ships rather
        than living in a validation script -- so it is worth saying why it is
        here and why it is not the thing decision 9 refused. The paragraph below
        names every other write, context-managed or not.
        That decision rejected patching `gate.ARCHITECTURE` as an alternative
        to passing the documents to `main`, because it would have left every
        case mutating shared module state to reach a value the signature could
        have carried. This patch reaches something no signature can: what the
        module does *while it is being imported*, before `main` exists to be
        called. It is scoped to one case, restored in a `finally` whether the
        exec raises or not, and reaches nothing another case can observe.

        The whole of what this file writes outside its own namespace, since a
        reader auditing that should not have to go looking:
        `sys.dont_write_bytecode` and `sys.path`, set once at the head of the
        file and deliberately never restored -- the first keeps a `.pyc` out of
        `scripts/__pycache__/`, the second is how `containment` is
        imported at all, and undoing either would undo the import; `sys.stdout`
        and `sys.stderr`, swapped here and in `Main.report` by
        `contextlib.redirect_*`, which restores them on the way out;
        `pathlib.Path.read_text`, rebound in two places -- here, restored in
        the `finally` below, and by `Main.reading`, restored in its own, which
        `Main`'s two restore cases hold; and one temporary directory per
        `Published` case, made by `tempfile` outside this repository and
        removed by `addCleanup`. The first two are on master and predate this
        branch.
        """
        source = Path(gate.__file__).read_text()
        unpatched = Path.read_text

        def without_markers(path, *args, **kwargs):
            text = unpatched(path, *args, **kwargs)
            if path.suffix != ".md":
                return text
            for marker in self.MARKERS:
                text = text.replace(marker, "")
            return text

        probe = type(gate)("containment_over_unreadable_documents")
        probe.__file__ = gate.__file__
        Path.read_text = without_markers
        try:
            exec(compile(source, gate.__file__, "exec"), probe.__dict__)
        finally:
            Path.read_text = unpatched

        self.assertFalse(hasattr(probe, "failures"))
        # And the rules are still all there to be run, reporting rather than
        # raising over the documents they cannot read.
        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(probe.main(), 1)


class Token(unittest.TestCase):
    """What one *Named by* cell parses to, and when it refuses to parse."""

    def spellings(self, cell):
        parsed = gate.token(cell)
        self.assertIsNotNone(parsed, f"{cell!r} was expected to parse")
        return [(spelling, is_gate) for spelling, _, is_gate in parsed]

    def test_a_two_token_cell_yields_both_spellings(self):
        self.assertEqual(
            self.spellings('`uuid`, `feature = "uuid"`'),
            [("uuid", False), ('feature = "uuid"', True)],
        )

    def test_a_gate_is_matched_over_the_corpus_that_keeps_literals(self):
        (_, pattern, is_gate), = gate.token('`feature = "decimal-big"`')
        self.assertTrue(is_gate)
        self.assertTrue(pattern.search('#[cfg(feature = "decimal-big")]'))

    def test_a_path_matches_the_whitespace_a_formatter_may_insert(self):
        (_, pattern, _), = gate.token("`Registry::new`")
        self.assertTrue(pattern.search("Registry :: new()"))

    def test_a_braced_cell_expands_to_one_spelling_each(self):
        self.assertEqual(
            self.spellings("`Registry::{new,default}`"),
            [("Registry::new", False), ("Registry::default", False)],
        )

    def test_a_second_token_that_lost_its_backticks_is_refused(self):
        self.assertIsNone(gate.token('`uuid`, feature = "uuid"'))

    def test_a_first_token_that_lost_its_backticks_is_refused(self):
        self.assertIsNone(gate.token('uuid, `feature = "uuid"`'))

    def test_prose_after_the_last_token_is_refused(self):
        self.assertIsNone(gate.token("`Registry::new` and anything it calls"))

    def test_prose_between_two_tokens_is_refused(self):
        self.assertIsNone(gate.token("`uuid` or else `Document`"))

    def test_a_cell_that_is_one_bare_token_still_parses(self):
        self.assertEqual(self.spellings("Document"), [("Document", False)])

    def test_a_cell_this_rule_cannot_read_is_refused(self):
        self.assertIsNone(gate.token("whatever the emitter happens to call"))


class GatePolarity(unittest.TestCase):
    """What a gate spelling matches in source, and what it refuses to match.

    A gate is a claim about code a build compiles, and the string
    `feature = "x"` is written by the gate, by its negation, by a `cfg_attr`
    that compiles nothing in any configuration, by a `cfg!` that compiles in
    every one of them, and by any nesting of the four. Matched as text a
    spelling could tell none of them apart, so a
    `#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]` in a file the `uuid` row
    does not allow failed the build over an annotation no build compiles
    anything from; anchored on `#[` a spelling could not see the `cfg!` at all,
    which is the opposite error and the worse one, since it passes in silence.
    Read as a predicate instead: each `#[cfg(`, `#![cfg(`, `#[cfg_attr(`,
    `#![cfg_attr(` and `cfg!` under any of its three delimiters -- what
    `PREDICATE` matches -- is walked with its delimiters balanced, and `search`
    matches the flag at the polarity the cell asked for.

    `named` is the same walk reading no polarity, and is what the offender scan
    asks: a cell states a polarity as its own claim, and a *site* names the
    flag whichever way its gate reads.

    The fragments are written for this file, with one exception: the compound
    predicate below is copied from `crates/kynos/src/lib.rs`, because a case
    about reading a nested `not(any(` should be a case about one this
    repository writes. Compound predicates are routine here rather than the
    exotic case, which is why a case about one belongs in this file.

    Two known limits, recorded rather than fixed. The second is in
    `Gate.names`' docstring: a `cfg` applied *by* a `cfg_attr`, as
    `#[cfg_attr(pred, cfg(feature = "x"))]`, does compile its item
    conditionally and the walk stops at the comma before reaching it. No `.rs`
    file here writes `cfg_attr`, so no fragment below is that shape.

    The first is here: the walk finds its attributes over the whole corpus, so
    an attribute written inside a *raw* string --
    `const D: &str = r#"#[cfg(feature = "uuid")]"#;` -- is read as a gate. The
    substring match this replaced read it as one too, so nothing regressed, and
    the corpus a gate is asked of has to keep its literals because a flag name
    is one. `literal_end` is what keeps a paren inside a literal from
    desynchronising a predicate the walk is already inside, which is a
    different claim and is the one the case below holds.
    """

    #: `crates/kynos/src/lib.rs`: `time` names no library of its own, and the
    #: gate that says so names both backends negatively.
    COMPOUND = (
        "#[cfg(all(\n"
        '    feature = "time",\n'
        '    not(any(feature = "time-chrono", feature = "time-jiff"))\n'
        "))]\n"
        'compile_error!("the `time` feature carries no types of its own");\n'
    )

    def matcher(self, cell):
        parsed = gate.token(cell)
        self.assertIsNotNone(parsed, f"{cell!r} was expected to parse")
        self.assertEqual(len(parsed), 1, f"{cell!r} was expected to hold one spelling")
        _, pattern, is_gate = parsed[0]
        self.assertTrue(is_gate, f"{cell!r} was expected to parse as a gate")
        return pattern

    def test_a_negated_gate_is_not_the_positive_one(self):
        self.assertFalse(
            self.matcher('`feature = "uuid"`').search(
                '#[cfg(not(feature = "uuid"))]\nfn absent_when_uuid_is_on() {}\n'
            )
        )

    def test_either_polarity_names_the_flag_for_the_offender_scan(self):
        # `named`, which is what the offender scan asks and the one question
        # here that reads no polarity: a site naming the flag is a site the row
        # has to cover, whichever way its gate reads.
        matcher = self.matcher('`feature = "uuid"`')
        self.assertTrue(matcher.named('#[cfg(feature = "uuid")]\nfn f() {}\n'))
        self.assertTrue(matcher.named('#[cfg(not(feature = "uuid"))]\nfn f() {}\n'))

    def test_a_negated_cell_names_the_flag_at_either_polarity_too(self):
        # The same, from the other cell. A row writing the negation is asking
        # about the same file the positive one is.
        matcher = self.matcher('`not(feature = "openapi31")`')
        self.assertTrue(
            matcher.named('#[cfg(not(feature = "openapi31"))]\ncompile_error!("no");\n')
        )
        self.assertTrue(matcher.named('#[cfg(feature = "openapi31")]\nfn f() {}\n'))

    def test_a_documentation_annotation_names_the_flag_at_no_polarity(self):
        # Blind to the polarity, and not to the predicate: the annotation
        # compiles nothing in any configuration, so it is a site of nothing at
        # either polarity. Reading `named` as a text match passes this fixture
        # and reinstates the false positive half of #134.
        self.assertFalse(
            self.matcher('`feature = "uuid"`').named(
                '#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]\npub fn f() {}\n'
            )
        )

    def test_a_documentation_annotation_is_not_a_gate(self):
        self.assertFalse(
            self.matcher('`feature = "uuid"`').search(
                '#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]\npub fn f() {}\n'
            )
        )

    def test_a_cfg_attr_predicate_is_still_a_gate(self):
        self.assertTrue(
            self.matcher('`feature = "uuid"`').search(
                '#[cfg_attr(feature = "uuid", derive(Debug))]\nstruct S;\n'
            )
        )

    def test_a_negated_cell_names_the_negation(self):
        pattern = self.matcher('`not(feature = "openapi31")`')
        self.assertTrue(
            pattern.search('#[cfg(not(feature = "openapi31"))]\ncompile_error!("no");\n')
        )
        self.assertFalse(pattern.search('#[cfg(feature = "openapi31")]\nfn f() {}\n'))

    def test_a_flag_positive_under_all_beside_a_negated_backend_is_read_both_ways(self):
        self.assertTrue(self.matcher('`feature = "time"`').search(self.COMPOUND))
        self.assertFalse(self.matcher('`feature = "time-chrono"`').search(self.COMPOUND))
        self.assertTrue(
            self.matcher('`not(feature = "time-chrono")`').search(self.COMPOUND)
        )

    def test_a_flag_inside_all_is_still_a_gate(self):
        self.assertTrue(
            self.matcher('`feature = "uuid"`').search(
                '#[cfg(all(feature = "uuid", feature = "yaml"))]\nfn both() {}\n'
            )
        )

    def test_a_bare_cfg_gate_is_still_a_gate(self):
        self.assertTrue(self.matcher('`feature = "uuid"`').search(GATE))

    def test_an_inner_attribute_is_read_like_an_outer_one(self):
        self.assertTrue(
            self.matcher('`feature = "test-util"`').search(
                '#![cfg(feature = "test-util")]\npub mod test {}\n'
            )
        )

    def test_a_doubly_negated_gate_is_the_positive_one(self):
        source = '#[cfg(not(not(feature = "uuid")))]\nfn present_with_uuid() {}\n'
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))
        self.assertFalse(self.matcher('`not(feature = "uuid")`').search(source))

    def test_a_negation_written_with_a_space_is_still_a_negation(self):
        # `\bnot\s*\(`, both halves. Rust allows the space, rustfmt does not
        # insert it, and a pattern reading `not(` alone takes the spaced form
        # for a bare paren -- which reads the flag POSITIVE, over code that
        # exists only when the flag is off. That is #134's reported symptom.
        source = '#[cfg(not (feature = "uuid"))]\nfn absent_when_uuid_is_on() {}\n'
        self.assertFalse(self.matcher('`feature = "uuid"`').search(source))
        self.assertTrue(self.matcher('`not(feature = "uuid")`').search(source))

    def test_a_cfg_attr_predicate_nesting_a_comma_is_read_past_it(self):
        # The comma-depth test. Only a comma at the attribute's own depth ends
        # a `cfg_attr` predicate; one inside `all(...)` separates that group's
        # own arguments. Reading any comma as the end makes this a non-gate,
        # and an offender naming an off-path flag then goes unreported.
        source = '#[cfg_attr(all(docsrs, feature = "uuid"), doc(hidden))]\npub fn f() {}\n'
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))

    def test_a_flag_after_a_closed_negation_is_read_at_the_outer_polarity(self):
        # The `)` pop: without it the walk stays inside `not(` and reads `uuid`
        # as negative, which is the whole predicate inverted by one branch.
        source = '#[cfg(all(not(feature = "yaml"), feature = "uuid"))]\nfn f() {}\n'
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))
        self.assertTrue(self.matcher('`not(feature = "yaml")`').search(source))

    def test_a_flag_beside_a_closed_group_is_still_reached(self):
        # The plain `(` push: without it the group's two closes empty the stack
        # early, the walk stops at the end of the attribute's first argument,
        # and everything after it is invisible.
        source = (
            '#[cfg(any(all(feature = "yaml", feature = "json"), feature = "uuid"))]\n'
            "fn f() {}\n"
        )
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))

    def test_a_parenthesis_inside_a_string_literal_does_not_close_the_predicate(self):
        # `literal_end`: without it the `)` in `"a)b"` closes two groups that
        # are still open and the rest of the predicate is never read.
        source = (
            '#[cfg(all(feature = "yaml", not(cfgname = "a)b")), feature = "uuid"))]\n'
            "fn f() {}\n"
        )
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))

    def test_a_cfg_macro_is_a_gate(self):
        # `cfg!(...)`, the fifth form. It is not an attribute and compiles in
        # every configuration, which is exactly why it counts: it expands to a
        # `bool` and branches at run time, so what it guards is code the build
        # emitted whichever way the flag reads. A file writing one is coupled
        # to the flag as firmly as one writing the attribute, and reading only
        # the attribute forms let such a site past the offender scan in
        # silence -- the outcome `containment.py` names as its worst.
        source = 'pub fn gated() -> bool {\n    cfg!(feature = "uuid")\n}\n'
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))
        self.assertTrue(self.matcher('`feature = "uuid"`').named(source))

    def test_a_cfg_macro_reads_its_polarity_like_an_attribute(self):
        # The predicate inside a `cfg!` is the predicate inside a `#[cfg]`, so
        # one walk answers both and a negated macro is negated for `search`
        # and named all the same for `named`.
        source = 'pub fn absent() -> bool {\n    cfg!(not(feature = "uuid"))\n}\n'
        self.assertFalse(self.matcher('`feature = "uuid"`').search(source))
        self.assertTrue(self.matcher('`not(feature = "uuid")`').search(source))
        self.assertTrue(self.matcher('`feature = "uuid"`').named(source))

    def test_a_cfg_macro_is_a_gate_whatever_delimiters_it_opens(self):
        # A macro invocation may be delimited `(`, `{` or `[`, and `rustc`
        # compiles all three: `cfg!{feature = "uuid"}` and
        # `cfg![feature = "uuid"]` gate a build exactly as the parenthesised
        # form does. Anchored on `(` alone the other two read as no gate at
        # all, which is the silent pass this pattern exists to close and the
        # same one the attribute anchoring was.
        for source in (
            'pub fn gated() -> bool {\n    cfg!{feature = "uuid"}\n}\n',
            'pub fn gated() -> bool {\n    cfg![feature = "uuid"]\n}\n',
        ):
            self.assertTrue(self.matcher('`feature = "uuid"`').search(source))
            self.assertTrue(self.matcher('`feature = "uuid"`').named(source))

    def test_a_braced_cfg_macro_ends_at_the_delimiter_that_opened_it(self):
        # The walk's half of the case above. A predicate opened on `{` closes
        # on `}`, and a walk counting parentheses alone would run past the end
        # of the macro into the code below it -- so this holds the polarity
        # rather than the match: `not(` inside a braced predicate is the
        # negation it is, and the positive spelling does not match it.
        source = 'pub fn absent() -> bool {\n    cfg!{not(feature = "uuid")}\n}\n'
        self.assertFalse(self.matcher('`feature = "uuid"`').search(source))
        self.assertTrue(self.matcher('`not(feature = "uuid")`').search(source))
        self.assertTrue(self.matcher('`feature = "uuid"`').named(source))

    def test_an_attribute_written_with_other_delimiters_is_not_a_gate(self):
        # The asymmetry, and it is Rust's rather than this pattern's. `rustc`
        # rejects `#[cfg{...}]` and `#[cfg[...]]` -- "wrong meta list
        # delimiters", with a `help` naming `(` and `)` -- so the delimiter
        # freedom above belongs to the macro form alone, and the four
        # attribute alternatives stay anchored on `(`. Widening them would
        # read a gate into source no build compiles.
        for source in (
            '#[cfg{feature = "uuid"}]\nfn f() {}\n',
            '#[cfg[feature = "uuid"]]\nfn f() {}\n',
        ):
            self.assertFalse(self.matcher('`feature = "uuid"`').search(source))
            self.assertFalse(self.matcher('`feature = "uuid"`').named(source))

    def test_a_rust_negation_before_a_cfg_macro_is_not_a_predicate_negation(self):
        # `!cfg!(feature = "openapi32")`, which this workspace writes five
        # times. The `!` is Rust's operator applied to the `bool` the macro
        # expanded to, and it is outside the predicate: the gate names the
        # flag positively and a cell writing the negation does not match it.
        source = 'if !cfg!(feature = "uuid") {\n    unreachable!()\n}\n'
        self.assertTrue(self.matcher('`feature = "uuid"`').search(source))
        self.assertFalse(self.matcher('`not(feature = "uuid")`').search(source))

    def test_a_macro_name_carrying_anything_before_cfg_is_not_this_gate(self):
        # `feature = "…"` is not reserved to `cfg`, and a macro somebody else
        # wrote is not a gate on the build. Three shapes put something in
        # front of the name, and a word boundary stops only the first of them:
        # `mycfg!` is a different identifier; `other::cfg!` is a path this scan
        # cannot resolve, since a module may export any macro under that name;
        # and `$cfg!` is a `macro_rules!` metavariable, where the macro
        # actually invoked is whatever the caller passed. `rustc` compiles the
        # last two, so both are shapes real source may hold.
        #
        # `search` reading one as a gate lets a stale cell read live in
        # silence, and `named` reading one lets the offender scan report a
        # file that gates nothing. This tree writes all 19 of its `cfg!` calls
        # bare, so the narrowing is inert on it.
        for source in (
            'let d = mycfg!(feature = "uuid");\n',
            'let d = other::cfg!(feature = "uuid");\n',
            'let d = $cfg!(feature = "uuid");\n',
        ):
            self.assertFalse(self.matcher('`feature = "uuid"`').search(source))
            self.assertFalse(self.matcher('`feature = "uuid"`').named(source))

    def test_an_attribute_written_with_escaped_quotes_is_not_a_gate(self):
        # Named for what it holds: the flag pattern wants an unescaped quote
        # after the `=`, and a Rust string literal spelling an attribute has a
        # backslash there. It says nothing about literal-awareness -- the case
        # above is what says that, and the raw-string limit is in the docstring.
        self.assertFalse(
            self.matcher('`feature = "uuid"`').search(
                'const DOC: &str = "#[cfg(feature = \\"uuid\\")]";\n'
            )
        )


class AllowedSites(unittest.TestCase):
    """What one *Named only in* cell allows, and what it refuses to allow."""

    def test_a_bare_path_is_relative_to_the_home_scope(self):
        self.assertEqual(
            gate.allowed_sites("`router/dispatch.rs`"),
            {"crates/kynos/src/router/dispatch.rs"},
        )

    def test_a_crate_qualified_path_is_relative_to_the_root(self):
        self.assertEqual(
            gate.allowed_sites("`crates/kynos-openapi/src/emit/mod.rs`"),
            {"crates/kynos-openapi/src/emit/mod.rs"},
        )

    def test_a_braced_path_expands_to_its_siblings(self):
        self.assertEqual(
            gate.allowed_sites("`router/{describe,install}.rs`"),
            {"crates/kynos/src/router/describe.rs", "crates/kynos/src/router/install.rs"},
        )

    def test_a_cell_that_is_one_bare_path_still_parses(self):
        self.assertEqual(
            gate.allowed_sites("router/dispatch.rs"),
            {"crates/kynos/src/router/dispatch.rs"},
        )

    def test_a_site_that_lost_its_backticks_is_refused(self):
        self.assertIsNone(
            gate.allowed_sites("`router/dispatch.rs`, crates/kynos-openapi/src/emit/mod.rs")
        )

    def test_prose_after_the_last_site_is_refused(self):
        self.assertIsNone(gate.allowed_sites("`router/dispatch.rs` and nowhere else"))


# The trees this repository actually has, for the cases that do not care which
# tree is missing. Stated rather than read off disk: a case asserting what a
# derived tree does is about the derivation, and reading the real layout would
# make it about the layout instead.
REAL = {"crates/kynos/src/", "crates/kynos-openapi/src/", "crates/kynos-macros/src/"}


class Permitted(unittest.TestCase):
    """Which files the allowance table lets name `tokio` outside `server/`.

    An allowed site is a path *segment* prefix and not a string prefix, which
    is what the third case holds: an allowance for `runtime/` that also
    permitted `runtimefoo.rs` would grant a file nobody wrote a row for, and
    the row it was granted under would go on reading as the check.
    """

    #: Written for this file: a directory site, which is the shape a row's
    #: brace expansion yields several of and the only shape a prefix can be
    #: read wrongly for.
    ALLOWED = {"crates/kynos/src/runtime"}

    def test_the_server_module_is_permitted_whatever_the_table_says(self):
        self.assertTrue(gate.permitted("crates/kynos/src/server/accept.rs", set()))

    def test_an_allowed_site_permits_the_files_under_it(self):
        self.assertTrue(
            gate.permitted("crates/kynos/src/runtime/spawn.rs", self.ALLOWED)
        )

    def test_a_sibling_that_shares_an_allowed_site_s_name_is_not_permitted(self):
        self.assertFalse(
            gate.permitted("crates/kynos/src/runtimefoo.rs", self.ALLOWED)
        )


class Scanned(unittest.TestCase):
    """The trees a row's own sites put it in reach of, and which of them exist.

    The derivation is string surgery over a hand-written crate name, and no
    other check on the cell can see a misspelling: a typo'd crate is still
    backticked, still `crates/`-prefixed and still reads as a path. What it
    yields is a tree matching no file, which narrows the row back to the home
    scope where its spelling is still written -- so both halves of the row pass
    and the sibling crate leaves the gate without a word.
    """

    def scanned(self, sites, trees=REAL):
        return gate.scanned(sites, exists=trees.__contains__)

    def test_a_row_with_no_crate_qualified_site_scans_its_home_scope(self):
        sites = gate.allowed_sites("`router/dispatch.rs`, `router/describe.rs`")
        self.assertEqual(self.scanned(sites), (["crates/kynos/src/"], []))

    def test_a_crate_qualified_site_opens_that_crate_to_the_scan(self):
        sites = gate.allowed_sites(
            "`router/dispatch.rs`, `crates/kynos-openapi/src/emit/mod.rs`"
        )
        self.assertEqual(
            self.scanned(sites),
            (["crates/kynos-openapi/src/", "crates/kynos/src/"], []),
        )

    def test_a_crate_path_outside_a_src_tree_opens_nothing(self):
        self.assertEqual(
            self.scanned({"crates/kynos-openapi/Cargo.toml"}),
            (["crates/kynos/src/"], []),
        )

    def test_a_misspelled_crate_is_reported_rather_than_scanned_for_nothing(self):
        sites = gate.allowed_sites("`crates/kynos-opanapi/src/emit/mod.rs`")
        trees, missing = self.scanned(sites)
        self.assertEqual(missing, ["crates/kynos-opanapi/src/"])
        self.assertIn("crates/kynos-opanapi/src/", trees)

    def test_the_real_layout_leaves_every_derived_tree_standing(self):
        sites = gate.allowed_sites("`crates/kynos-openapi/src/emit/mod.rs`")
        self.assertEqual(gate.scanned(sites)[1], [])


class OffPathCoverage(unittest.TestCase):
    """Every flag graded `Off-path proof` owes a row, and the grade owes its name.

    The comparison is between two documents, so its inputs are stated here
    rather than read: what is under test is what the rule does with a grading
    and a table, not what this repository's two happen to say today.
    """

    def test_a_graded_flag_with_a_row_owes_nothing(self):
        self.assertEqual(
            gate.off_path_coverage(["uuid"], {"the `uuid` feature", "uuid"}, ["Off-path proof"]),
            [],
        )

    def test_a_graded_flag_with_no_row_is_named(self):
        failures = gate.off_path_coverage(
            ["uuid", "yaml"], {"uuid"}, ["Full battery", "Off-path proof"]
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("yaml", failures[0])
        self.assertNotIn("uuid\n", failures[0])

    def test_a_row_for_a_flag_graded_elsewhere_is_not_an_error(self):
        self.assertEqual(
            gate.off_path_coverage([], {"docs", "macros"}, ["Off-path proof"]), []
        )

    def test_a_renamed_grade_fails_rather_than_comparing_an_empty_set(self):
        failures = gate.off_path_coverage([], {"uuid"}, ["Full battery", "Off-path argument"])
        self.assertEqual(len(failures), 1)
        self.assertIn("no longer has a", failures[0])


class CargoConfig(unittest.TestCase):
    """What `.cargo/config.toml` must declare, and what it may not declare.

    A rule rather than a parser, and here for `off_path_coverage`'s reason: its
    failure mode is silence, and the silence is cargo's. A key that has lost a
    letter is `warning: unused config key` and an exit status of zero; a
    profile *table* that has lost one is not reported at all. Both were run
    against a scratch package before the rule was written, and both finished
    `unoptimized + debuginfo`. So every case below is a config the real tree
    does not contain and every gate in `mise run check` compiles happily over.

    The inputs are stated rather than read off disk, because what is under test
    is what the rule does with a config file and not what this repository's one
    says today.
    """

    #: The shape the repository ships: the two keys, and nothing else at all.
    PROFILE = (
        '[profile.dev]\ndebug = "line-tables-only"\n\n'
        '[profile.dev.package."*"]\ndebug = false\n'
    )

    def test_the_shape_this_repository_ships_holds(self):
        self.assertEqual(gate.cargo_config_failures(self.PROFILE), [])

    def test_a_deleted_file_is_named_rather_than_skipped(self):
        failures = gate.cargo_config_failures(None)
        self.assertEqual(len(failures), 1)
        self.assertIn(".cargo/config.toml", failures[0])

    def test_a_file_that_is_not_toml_fails_rather_than_raising(self):
        failures = gate.cargo_config_failures("[profile.dev\ndebug =\n")
        self.assertEqual(len(failures), 1)
        self.assertIn("TOML", failures[0])

    def test_a_misspelled_key_is_a_missing_key(self):
        config = self.PROFILE.replace("debug = \"line", "debgu = \"line")
        failures = gate.cargo_config_failures(config)
        self.assertEqual(len(failures), 1)
        self.assertIn("profile.dev.debug", failures[0])

    def test_a_misspelled_profile_table_is_a_missing_key_too(self):
        failures = gate.cargo_config_failures(self.PROFILE.replace("[profile.dev]", "[profile.dve]"))
        self.assertEqual(len(failures), 1)
        self.assertIn("profile.dev.debug", failures[0])

    def test_a_dropped_dependency_profile_is_named_by_its_own_key(self):
        failures = gate.cargo_config_failures('[profile.dev]\ndebug = "line-tables-only"\n')
        self.assertEqual(len(failures), 1)
        self.assertIn('profile.dev.package."*".debug', failures[0])

    def test_a_misspelled_package_table_is_the_same_failure(self):
        failures = gate.cargo_config_failures(self.PROFILE.replace("pack", "pcak"))
        self.assertEqual(len(failures), 1)
        self.assertIn('profile.dev.package."*".debug', failures[0])

    def test_a_renamed_glob_leaves_no_dependency_profile(self):
        failures = gate.cargo_config_failures(self.PROFILE.replace('."*"', '."hyper"'))
        self.assertEqual(len(failures), 1)
        self.assertIn('profile.dev.package."*".debug', failures[0])

    def test_both_keys_gone_are_two_failures(self):
        self.assertEqual(len(gate.cargo_config_failures("")), 2)

    def test_a_build_table_is_refused_and_the_message_says_where_it_belongs(self):
        failures = gate.cargo_config_failures(
            self.PROFILE + '\n[build]\nrustc-wrapper = "sccache"\n'
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("build", failures[0])
        self.assertIn("mise.toml", failures[0])

    def test_a_source_replacement_is_refused(self):
        failures = gate.cargo_config_failures(
            self.PROFILE + '\n[source.crates-io]\nreplace-with = "mirror"\n'
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("source", failures[0])

    def test_a_top_level_key_outside_any_table_is_refused_too(self):
        failures = gate.cargo_config_failures('paths = ["../patched"]\n' + self.PROFILE)
        self.assertEqual(len(failures), 1)
        self.assertIn("paths", failures[0])

    def test_another_profile_is_not_a_foreign_table(self):
        self.assertEqual(
            gate.cargo_config_failures(self.PROFILE + "\n[profile.release]\nlto = true\n"),
            [],
        )

    def test_a_missing_key_and_a_foreign_table_are_reported_together(self):
        failures = gate.cargo_config_failures('[build]\njobs = 4\n')
        self.assertEqual(len(failures), 3)


class TaxonomyCount(unittest.TestCase):
    """How many of `performance.md`'s kinds of measurement run today.

    A rule rather than a parser, and here for `cargo_config_failures`' reason:
    its failure mode is silence. Nothing in this repository reads Markdown
    prose, so the sentence and the table below it drifted apart twice in one
    day with every gate green -- two branches rewrote the same count from two
    readings of the same five rows. So every case below is a document this
    repository does not ship and `mise run check` passes over unchanged.

    The inputs are stated rather than read off disk, for the reason the two
    classes above give: what is under test is what the rule does with a
    document, not what `performance.md` happens to say today.
    """

    #: The kinds, in the order the shipped table writes them.
    KINDS = [
        "Allocation count",
        "Size guard",
        "Off-path proof",
        "Codegen delta",
        "Binary delta",
    ]

    def document(self, claim="All five of the kinds below run today", statuses=None, header=None):
        """A `performance.md` whose opening sentence states `claim`.

        The cells carry no prose past their status: what the rule reads of a
        row is its first column and its last, and a fixture that copied the
        rest would only assert that the shipped table still says it.
        """
        statuses = ["in use"] * len(self.KINDS) if statuses is None else statuses
        head = gate.TAXONOMY_HEADER if header is None else header
        rows = "".join(
            f"| {kind} | a target | `cargo nextest` | that it did not grow | {status} |\n"
            for kind, status in zip(self.KINDS, statuses)
        )
        return (
            f"# Performance\n\n{claim}, four of them only for part of what they\n"
            f"cover.\n\n## The taxonomy\n\n{head}\n| --- | --- | --- | --- | --- |\n"
            f"{rows}\nProse after the table.\n"
        )

    def test_the_shape_this_document_ships_holds(self):
        self.assertEqual(gate.taxonomy_failures(self.document()), [])

    def test_a_kind_that_stopped_running_is_named(self):
        failures = gate.taxonomy_failures(
            self.document(statuses=["in use", "in use", "planned", "in use", "in use"])
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("Off-path proof", failures[0])
        self.assertNotIn("Size guard", failures[0])

    def test_a_status_that_denies_running_does_not_read_as_running(self):
        failures = gate.taxonomy_failures(
            self.document(statuses=["not in use"] + ["in use"] * 4)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("Allocation count", failures[0])

    def test_a_dropped_row_fails_the_stated_count(self):
        document = self.document()
        last = document[document.index("| Binary delta") :]
        failures = gate.taxonomy_failures(document.replace(last.split("\n")[0] + "\n", ""))
        self.assertEqual(len(failures), 1)
        self.assertIn("5", failures[0])
        self.assertIn("4", failures[0])

    def test_a_table_cut_short_by_a_blank_line_fails(self):
        document = self.document().replace(
            "| Off-path proof", "\n| Off-path proof", 1
        )
        failures = gate.taxonomy_failures(document)
        self.assertEqual(len(failures), 1)
        self.assertIn("cut short", failures[0])

    def test_a_prose_count_moved_without_the_table_fails(self):
        failures = gate.taxonomy_failures(
            self.document(claim="All six of the kinds below run today")
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("6", failures[0])
        self.assertIn("5", failures[0])

    def test_a_kind_added_with_the_count_moved_holds(self):
        document = self.document(claim="All six of the kinds below run today")
        document = document.replace(
            "\nProse after",
            "| Timing figure | a harness | a runner | how long it took | in use |\n\nProse after",
        )
        self.assertEqual(gate.taxonomy_failures(document), [])

    def test_a_reworded_claim_is_named_rather_than_skipped(self):
        failures = gate.taxonomy_failures(
            self.document(claim="Two of the five kinds below run today")
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("no longer states", failures[0])

    def test_an_unreadable_count_fails_rather_than_passing(self):
        failures = gate.taxonomy_failures(
            self.document(claim="All eighteen of the kinds below run today")
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("eighteen", failures[0])

    def test_a_renamed_header_fails_rather_than_raising(self):
        failures = gate.taxonomy_failures(
            self.document(header="| Kind | Lives in | Runs under | Proves | State |")
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("taxonomy table", failures[0])

    def test_an_emptied_table_holds_nothing_and_says_so(self):
        document = self.document()
        document = document[: document.index("| Allocation count")] + "\nProse after the table.\n"
        failures = gate.taxonomy_failures(document)
        self.assertEqual(len(failures), 1)
        self.assertIn("no rows", failures[0])

    def test_a_stale_count_over_a_stopped_kind_is_reported_twice(self):
        document = self.document(
            claim="All six of the kinds below run today",
            statuses=["in use", "in use", "in use", "in use", "planned"],
        )
        self.assertEqual(len(gate.taxonomy_failures(document)), 2)


class Published(unittest.TestCase):
    """Which files of a package a published archive would carry.

    The one suite here whose fixture is a directory rather than text, and it
    has to be: `published` takes a package and walks it, so what a case hands
    it is a package. Written into a temporary directory, removed on the way
    out, and never inside this repository.

    Both skips under test are ones no file here reaches. `target/` sits at the
    workspace root and never inside `crates/<name>/`, so nothing in this tree
    exercises the skip that drops one -- but a `CARGO_TARGET_DIR` pointed
    inside a package is enough to produce one, and what it would feed the
    escape rule is a build script's generated sources, every `include!` of
    which resolves outside the package by construction. The skip stays, and
    this is where it is reachable.
    """

    def package(self, exclude, *paths):
        """A package holding `paths`, with `exclude` in its manifest."""
        holder = tempfile.TemporaryDirectory()
        self.addCleanup(holder.cleanup)
        root = Path(holder.name)
        entries = ", ".join(f'"{entry}"' for entry in exclude)
        (root / "Cargo.toml").write_text(
            f'[package]\nname = "probe"\nexclude = [{entries}]\n'
        )
        for relative in paths:
            source = root / relative
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text("pub fn probe() {}\n")
        return root

    def published(self, root):
        return sorted(source.relative_to(root).as_posix() for source in gate.published(root))

    def test_a_source_the_manifest_excludes_is_not_published(self):
        root = self.package(["benches"], "src/lib.rs", "benches/one.rs")
        self.assertEqual(self.published(root), ["src/lib.rs"])

    def test_a_sibling_that_shares_an_excluded_name_is_published(self):
        # The `+ "/"`: an `exclude` is a path segment prefix, and reading it as
        # a string prefix exempts a directory nobody excluded -- which is a
        # file that ships with the archive and reaches outside it unchecked.
        root = self.package(["benches"], "src/lib.rs", "benches_old/one.rs")
        self.assertEqual(self.published(root), ["benches_old/one.rs", "src/lib.rs"])

    def test_nothing_under_a_target_directory_is_published(self):
        root = self.package([], "src/lib.rs", "target/debug/build/probe/out/rows.rs")
        self.assertEqual(self.published(root), ["src/lib.rs"])


class PythonFloor(unittest.TestCase):
    """Which scripts are reported as having raised the grammar floor.

    A rule rather than a parser, and here for `CargoConfig`'s reason: its
    failure mode is silence. A file written in newer grammar runs perfectly for
    whoever wrote it and is a `SyntaxError` before any rule or any case runs
    for everybody else, which is the shape of #134's import-time defect one
    layer down. Nothing in this repository reported it until this rule, and the
    only record that the floor had already moved once is a commit message.

    The floor is stated per case rather than taken from `gate.PYTHON_FLOOR`,
    and that is not the usual preference for stated inputs -- it is forced.
    Syntax that violates a floor is syntax some interpreter cannot parse at
    all, so a case written against the real floor of 3.11 would assert one
    thing on 3.12 and another on the pinned interpreter itself, where PEP 695
    is not grammar the parser has. A floor of 3.9 with a `match` statement over
    it is the same claim and reads identically on every interpreter the pin
    allows. `Main` below is what holds the real number.
    """

    #: A `match` statement: 3.10 grammar, which every interpreter that can run
    #: this file parses and which `feature_version=(3, 9)` refuses.
    MATCHED = "def read(value):\n    match value:\n        case 1:\n            return 1\n"
    #: Grammar no version has: a file that is broken rather than new.
    BROKEN = "def read(:\n"

    def failures(self, *scripts, floor=(3, 9)):
        return gate.python_floor_failures(scripts, floor)

    def test_a_script_that_parses_at_the_floor_is_not_named(self):
        self.assertEqual(self.failures(("scripts/probe.py", "value = 1\n")), [])

    def test_a_script_written_above_the_floor_is_named(self):
        failures = self.failures(("scripts/probe.py", self.MATCHED))
        self.assertEqual(len(failures), 1)
        self.assertIn("scripts/probe.py", failures[0])

    def test_the_floor_it_stopped_holding_is_named(self):
        self.assertIn(
            "does not parse at Python 3.9",
            self.failures(("scripts/probe.py", self.MATCHED))[0],
        )

    def test_the_version_it_needs_is_reported_rather_than_a_mismatch(self):
        self.assertIn(
            "it needs 3.10", self.failures(("scripts/probe.py", self.MATCHED))[0]
        )

    def test_the_line_the_grammar_stops_at_is_reported(self):
        # Over the broken source rather than the newer one. Where CPython
        # reports a `match` statement it will not accept is the end of the
        # block rather than its head, and which line that is has moved between
        # versions -- so a case anchored on it would hold the interpreter's
        # choice and not this rule's reporting of it.
        self.assertIn(
            "line 3 is where it stops",
            self.failures(("scripts/probe.py", "value = 1\n\n" + self.BROKEN))[0],
        )

    def test_a_script_that_parses_at_no_version_is_reported_rather_than_raising(self):
        # The `SyntaxError` is caught for `cargo_config_failures`' reason: a
        # rule that takes the process down takes the test run with it, and a
        # script nothing can read is exactly when the other rules have to go on
        # reporting. Without the `except`, this case is an error rather than a
        # failure and it is `containment:test` that stops running, not the one
        # rule that could not read one file.
        failures = self.failures(("scripts/probe.py", self.BROKEN))
        self.assertEqual(len(failures), 1)
        self.assertIn("parses at no version", failures[0])

    def test_a_broken_script_is_not_reported_as_merely_newer(self):
        # The pair to the case above, and the reason the rule writes two
        # sentences rather than one: the remedies differ -- repair the line, or
        # raise the pin -- and a rule that ran them together would tell half
        # its readers to bump an interpreter over a typo.
        self.assertNotIn("it needs", self.failures(("scripts/probe.py", self.BROKEN))[0])

    def test_a_script_holding_the_floor_beside_one_that_does_not_is_not_named(self):
        failures = self.failures(
            ("scripts/held.py", "value = 1\n"),
            ("scripts/raised.py", self.MATCHED),
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("scripts/raised.py", failures[0])

    def test_two_scripts_over_the_floor_are_two_failures(self):
        failures = self.failures(
            ("scripts/first.py", self.MATCHED),
            ("scripts/second.py", self.MATCHED),
        )
        self.assertEqual(len(failures), 2)

    # There is deliberately no case here asserting what `gate.PYTHON_FLOOR` is.
    # One was written -- `assertGreaterEqual(gate.PYTHON_FLOOR, (3, 11))` -- and
    # it was a third hardcoded copy of the number rather than a check on it: the
    # only edit it could refuse was one that had already agreed with it. What
    # holds the number now is `PythonPins` below, which compares the floor
    # against the `python` pin that decides what actually runs, so lowering the
    # floor alone reports and lowering both is a commit a reader sees. The
    # runtime half -- that `import tomllib` at the head of `containment.py`
    # needs 3.11 whatever the grammar allows -- is held by nothing here and by
    # nothing anywhere else either: it is held by the gate running under the
    # pin, where a pin below 3.11 fails at that import before a rule runs.
    # `containment.py`'s own comment on `PYTHON_FLOOR` records that.


class PythonPins(unittest.TestCase):
    """Whether `mise.toml` still pins the floor `containment.py` declares.

    The other half of `PythonFloor`, and the half that was written in nine
    places with nothing holding them together. A rule rather than a parser, and
    with the sharpest silence of any of them: setting `PYTHON_FLOOR` two minors
    above every pin printed `every rule holds` and left every case here green,
    and so did a ninth task under `scripts/` carrying no pin at all.

    The configurations are stated rather than read off disk, for `CargoConfig`'s
    reason: what is under test is what the rule does with a `mise.toml`, not
    what this repository's own says today. `Main` below reaches the real one.
    """

    def config(self, *tasks):
        """A `mise.toml` declaring `tasks`, each `(name, run, pin)`."""
        blocks = []
        for name, run, pin in tasks:
            block = f'[tasks."{name}"]\n'
            if pin is not None:
                block += f'tools = {{ python = "{pin}" }}\n'
            blocks.append(block + f'run = "{run}"\n')
        return "\n".join(blocks)

    def failures(self, *tasks, scripts=("probe.py",), floor=(3, 11), exempt=()):
        return gate.python_pin_failures(
            self.config(*tasks), set(scripts), floor, frozenset(exempt)
        )

    def test_a_pinned_task_running_a_script_holds(self):
        self.assertEqual(
            self.failures(("probe", "python3 scripts/probe.py", "3.11.16")), []
        )

    def test_a_script_task_with_no_pin_is_named(self):
        failures = self.failures(("probe", "python3 scripts/probe.py", None))
        self.assertEqual(len(failures), 1)
        self.assertIn("probe", failures[0])
        self.assertIn("pins no `python`", failures[0])

    def test_two_tasks_pinning_different_versions_are_named_with_their_versions(self):
        failures = self.failures(
            ("first", "python3 scripts/probe.py", "3.11.16"),
            ("second", "python3 scripts/probe.py", "3.11.15"),
        )
        disagreement = [f for f in failures if "more than one" in f]
        self.assertEqual(len(disagreement), 1)
        self.assertIn("3.11.16: first", disagreement[0])
        self.assertIn("3.11.15: second", disagreement[0])

    def test_a_pin_whose_minor_is_not_the_floor_is_named(self):
        # The mutation this rule was written for: the declaration moves and the
        # interpreter does not, or the other way round, and until this nothing
        # in either gate could see it.
        failures = self.failures(("probe", "python3 scripts/probe.py", "3.13.2"))
        self.assertEqual(len(failures), 1)
        self.assertIn("declares a floor of 3.11", failures[0])

    def test_a_pin_that_names_no_minor_version_is_named(self):
        failures = self.failures(("probe", "python3 scripts/probe.py", "latest"))
        self.assertEqual(len(failures), 1)
        self.assertIn("can read a minor version out of it", failures[0])

    def test_a_pin_on_a_task_that_runs_no_script_is_named(self):
        failures = self.failures(("probe", "cargo test", "3.11.16"))
        self.assertEqual(len(failures), 1)
        self.assertIn("runs no file from scripts/", failures[0])

    def test_an_inline_interpreter_no_exemption_names_is_named(self):
        failures = self.failures(("probe", "python3 -c 'print(1)'", None))
        self.assertEqual(len(failures), 1)
        self.assertIn("something other than a file in scripts/", failures[0])

    def test_an_exempted_inline_interpreter_is_not_named(self):
        # The presence pair for the case above: an exemption that silenced
        # nothing would leave that one green whether it worked or not.
        self.assertEqual(
            self.failures(("probe", "python3 -c 'print(1)'", None), exempt=("probe",)),
            [],
        )

    def test_an_exemption_naming_a_task_that_runs_no_interpreter_is_named(self):
        failures = self.failures(("probe", "cargo test", None), exempt=("probe",))
        self.assertEqual(len(failures), 1)
        self.assertIn("outlived its argument", failures[0])

    def test_a_script_a_task_runs_that_the_floor_rule_never_read_is_named(self):
        # What holds the floor rule's own file set, which nothing else can. The
        # rule is handed the files that rule read; a narrowed glob reaches this
        # as a script a task runs and the floor rule does not.
        failures = self.failures(
            ("probe", "python3 scripts/probe.py", "3.11.16"), scripts=("other.py",)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("the floor rule never read", failures[0])
        self.assertIn("probe.py: run by probe", failures[0])

    def test_a_nested_script_a_task_runs_is_named_by_its_path(self):
        failures = self.failures(
            ("probe", "python3 scripts/gates/probe.py", "3.11.16"), scripts=("probe.py",)
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("gates/probe.py: run by probe", failures[0])

    def test_a_file_no_task_runs_is_deliberately_not_named(self):
        # Recorded rather than decorative: a module imported by a script rather
        # than run as one is a shape this rule has no reason to refuse, and the
        # floor rule reads it either way. The case above is what makes this
        # absence falsifiable -- the same comparison in the other direction does
        # report.
        self.assertEqual(
            self.failures(
                ("probe", "python3 scripts/probe.py", "3.11.16"),
                scripts=("probe.py", "helper.py"),
            ),
            [],
        )

    def test_a_file_that_is_not_toml_fails_rather_than_raising(self):
        failures = gate.python_pin_failures('[tasks."x"\nrun =\n', {"probe.py"})
        self.assertEqual(len(failures), 1)
        self.assertIn("TOML", failures[0])

    def test_a_quoted_task_key_is_read(self):
        # The reason the file is parsed rather than scanned. Every task key in
        # the real `mise.toml` is quoted, and a task this rule cannot see is a
        # task whose missing pin it reports as absent because it never looked.
        failures = self.failures(("cost:features", "python3 scripts/probe.py", None))
        self.assertEqual(len(failures), 1)
        self.assertIn("cost:features", failures[0])


class Main(unittest.TestCase):
    """What each rule stops checking when the marker it reads is gone.

    `Section` above pins the helper's refusal to widen a slice. This pins that
    the callers ask for it, which is a different claim and the one this file's
    subject rests on: with the helper in place and a call site written
    `table or ""`, or written without its end marker, both gates stay green and
    the rule goes on reporting against text it was written to exclude. Every case
    below fails against a mutation of the thing it names and passes against the
    code as written. That mutation is a call site's guard for some, a `main()`
    parameter ignored for others, and a rule's own failure deleted for the
    rest.

    The documents are this repository's own with one marker removed, and the
    corpus is this repository's own with at most one file rewritten. What is
    under test here is a *rule* rather than a parser: the `tokio` scan is only
    wrong about the tree it reads, and a synthetic tree would make the case a
    test of its own fixture. `main` takes both as arguments for this reason,
    the way `scanned` takes `exists`.

    The assertions name the rules that must and must not have run rather than
    counting failures, so a case says what it holds and does not fail for a
    reason belonging to `containment:check`. Nothing here writes to the
    repository. The process state it touches is `sys.stdout` and `sys.stderr`,
    swapped by `contextlib.redirect_*`, which restores them, and
    `pathlib.Path.read_text`, rebound by `reading` below for the length of one
    call and restored in a `finally` -- which the two cases above hold, since a
    patch that leaked would leave every later case reading through a closure
    belonging to a case that has finished. `ImportTime`'s docstring inventories
    both, and everything else this file writes outside its own namespace.

    One rule earns a case its place, and it is the rule two cases here were
    removed for failing: an assertion that a failure class is *absent* holds
    nothing unless something can make that class appear. A rule that ran and
    passed and a rule that never ran report the same nothing. So an absence
    assertion stays only if a mutation of the rule it names is caught by it --
    normally because a sibling case asserts the presence over the same document,
    as `test_a_widened_surface_slice_would_report_the_site_this_one_hides` does.

    That rule has been **swept across this suite**, not merely applied wherever
    a review found an instance. Three separate rounds each repaired the one case
    that prompted the finding and left the rest, and the third instance was the
    cost of that. Every absence assertion below is now paired with a presence
    case, or held by a mutation of the control flow it is an assertion about,
    or labelled in place as decorative.

    The second of those three is what the two `"is off the request path"`
    absences in the off-path row loop are, and the inventory named only the
    other two while they were in it. Each says that a row this rule has just
    called untrustworthy is not *also* scanned for offenders, and what each
    catches is the deletion of the `continue` that skips the scan: with either
    one gone, the row is scanned against a match set the same run has already
    reported as wrong, and the case fails. Run, not assumed.

    Of the third there is one: the `"names no site"` check in the end-marker
    case, which no input here can falsify because a widened slice still holds
    every link. The class it names is held by the two surface cases instead. A
    new absence assertion joins one of the three or it does not go in.

    The third kind of mutation named above -- a rule's own failure deleted --
    is held across every document `main` takes, and across both shapes `main`
    accumulates a failure in: `failures.append(...)` and
    `failures += helper(...)`. Every one of them a rewritten `architecture.md`,
    `testing.md`, `performance.md` or `nfr.md` can reach has a case below that
    makes it appear. That was not true before, and the gap was not decorative.
    Silencing the off-path offender scan -- the rule the second defect of #134
    exists to correct -- left both gates green, with `containment:check`
    printing that every rule holds. The repair is cheap for the reason the
    absence pairs are cheap, and this branch is what made it available: the
    documents are `main`'s arguments, so one rewritten cell reaches one rule.

    Both shapes, and the second is written down because the instrument drew
    that boundary once and drew it wrong. The sweep that found this class
    matched `failures.append` and nothing else, so
    `failures += taxonomy_failures(...)` and
    `failures += cargo_config_failures(...)` were never mutated at all, and the
    inventory below named fewer sites than the tree held. Silencing the
    taxonomy wiring left `containment:check` printing that every rule holds
    with the whole rule disconnected and `TaxonomyCount` green beside it: a
    helper held to its own cases says nothing about whether anything calls it.
    What a later sweep has to match is the accumulation, not one spelling of
    it.

    The rules `main` states over the tree rather than over a document split in
    two, and the split is when the read happens rather than what is read. The
    manifest rule, the package-escape read and the `cargo_config_failures`
    wiring open their file *while `main` runs*, so what a case has to reach is
    the read. `reading` below reaches it: one path, rebound for the length of
    one call and put back in a `finally`. That is the patch `ImportTime`
    justifies at length and it is justified the same way -- it reaches
    something no signature carries -- and it is not what decision 9 refused,
    which was patching a module constant a parameter could have carried
    instead. These three reads are not module constants, and no parameter
    would spare them.

    The three rules stated over the corpus and over no document at all -- the
    dependency-graph stray scan, the parent re-export scan and the placeholder
    scan -- are reached the same way, and are why `main` has a fifth argument.
    While the corpus was built at import nothing a case could pass reached
    them: by the time `main` ran it was already what the tree said, and neither
    an argument nor a rebound read went back that far. Silencing any one of
    their failures left both gates green, which is the severity class this file
    exists for. Each has a case below that hands over this repository's tree
    with one file rewritten, which is what `appending` is.

    The case the manifest rule cost, and why it stays gone rather than coming
    back: `test_the_manifest_rule_below_the_grading_still_runs` asserted that
    the rule reported nothing over a document with no grading header, and an
    assertion that a rule reported nothing is equally true of a rule that never
    ran. The presence case below is what that rule was owed. What used to be
    written here -- that the rule "is not injectable" -- was wrong, and the
    excuse outlived the reason for it.

    Left unheld deliberately, and on severity rather than on cost, because that
    is the part worth keeping: wrapping that rule in the grading guard makes it
    skipped only when `grading is None`, and that arises only when the grading
    header has been renamed -- which has already appended a loud failure, is
    returning 1, and says in its own message which rules stopped running. So the
    unheld mutant costs one rule quietly not running inside an already-red gate
    that enumerates them. Every mutant this file does hold left a *green* gate
    over a live defect. A mutant with no silent pass in it is a different class
    from one whose whole cost is a silent pass, and only the second kind is
    worth paying for.

    `main`'s success path is held here as well as by `containment:check`, and
    the split is which half of it each holds. The *report line* -- the counts
    and the sentence -- stays that gate's, because running every rule over the
    intact tree and printing what it found is what that gate is. The
    *emptiness* is this file's, and until a mutation pass said so it was held
    by neither: every other case below asserts `status == 1` and then filters
    the failures by a needle, so a rule reporting something *else* over this
    repository's own tree is invisible to all of them at once -- the needle
    still finds its one failure and the extra lines go unread. That is a whole
    class of regression, over-reporting, with nothing on it.

    `containment:check` catches such a rule by going red and says only that; a
    case here says which rule, in the file whose subject is the rules.
    `Corpus.naming` reading `sources` rather than `files` is the mutation that
    measured the gap: eleven offender lines over the pristine tree, and a green
    suite beside them.

    It does mean a genuine breakage in the tree reds this file as well as that
    gate. That is the cost, and it is the smaller one: a suite that cannot see
    a rule over-report holds only the half of each rule that fires.
    """

    ALLOWANCE = "| Site | Names | Why it is not in `server/` |"
    SURFACE = "### Public API surface"
    GRADING = "| Grade | Owes | Flags |"
    #: `architecture.md`'s count of hand-rolled `Stream` sites, read as written
    #: so a case does not depend on today's number.
    SITE_COUNT = re.compile(r"(\*\*One public row, )\w+( sites, and the count is the check\*\*)")
    #: Where `probing` writes its gate: a file the tree does not have, under a
    #: directory no off-path row allows a feature gate in.
    PROBE = "crates/kynos/src/router/probe.rs"

    def report(self, **given):
        """`main`'s status and what it reported, with its own output held.

        `given` is whichever of `main`'s arguments the case supplies -- one of
        the four documents, or the corpus -- and the rest default to this
        repository's own.

        A failure is one `containment: ` line plus the indented lines under it,
        joined: several of these messages name the offending paths below the
        sentence, and a case asserting which path was reported has to see them.
        """
        err, out = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
            status = gate.main(**given)
        head, failures = "containment: ", []
        for line in err.getvalue().split("\n"):
            if line.startswith(head):
                failures.append(line[len(head) :])
            elif line.strip() and failures:
                failures[-1] += "\n" + line
        return status, failures

    @contextlib.contextmanager
    def reading(self, path, rewrite):
        """One path in the tree reading as `rewrite` returns, for one call.

        Three rules below the grading are stated over the tree rather than over
        a document, and `main` opens their file itself: the manifest, each
        source a package publishes, and `.cargo/config.toml`. No argument
        reaches those, so the read is what a case reaches instead.

        Scoped to one path, restored whether the body raises or not, and
        reaching nothing another case can observe: the four documents and the
        default corpus were read at import and are already what they are.
        """
        target = gate.ROOT / path
        unpatched = Path.read_text

        def read(opened, *args, **kwargs):
            text = unpatched(opened, *args, **kwargs)
            return rewrite(text) if opened == target else text

        Path.read_text = read
        try:
            yield
        finally:
            Path.read_text = unpatched

    def rewriting(self, document, anchor, replacement):
        """`document` with the first `anchor` rewritten, refusing a no-op.

        For a fixture anchored on a detail of the document that is not what its
        case is about -- the cells of the `uuid` off-path row, below. That row
        may be rewritten in ways this gate accepts: writing the two paths out
        where the cell braces them is one, and it turns every anchored
        `replace` into a no-op at once. The cases then report `0 != 1` each,
        none of them naming the row, over a documentation edit that is fine.
        A fixture whose anchor IS its subject -- a header, a count sentence --
        needs no guard, because a case that reports the header unfound has
        named what moved. Nor does one with no anchor at all: appending to a
        source cannot miss.

        Every incidental anchor in this file goes through it, and not only the
        `uuid` ones: `DECLARED_SITE`, the `` `cookie` `` grade, the Aggregate
        row and one real allowed site in the `tokio` table are all details the
        cases that name them are not about. Two of those replaced *every*
        occurrence rather than the first, which this helper could not express
        while it passed a count; it no longer passes one, so it expresses them.
        Each anchor is written once in its document today, which is why routing
        them changed no fixture -- the guard is what they gain.

        Used on a source file as well as on a document, since a manifest key
        can move for the same reason a table cell can.

        The guard is the load-bearing half and the only half. The `replace`
        rewrote the first occurrence alone, and that count held nothing:
        instrumenting this helper to fail on an anchor occurring other than
        once left the whole suite green, so every caller anchors on a marker
        its document writes exactly once and the limit could not be observed.
        A caller that some day anchors on a marker written twice will rewrite
        both, which is the case to write a count into this guard for -- there
        is none today, and a limit nothing can falsify is the kind of detail
        this file removes rather than keeps.
        """
        if anchor not in document:
            # `self.fail` rather than `assertIn`, whose message renders the
            # whole document twice for want of a truncation.
            self.fail(f"this fixture no longer anchors on: {anchor!r}")
        return document.replace(anchor, replacement)

    def probing(self, written):
        """The real corpus with one feature gate at a site no row allows.

        `router/` is where no off-path row's *Named only in* cell reaches for a
        feature gate, and the file is one the tree does not have, so nothing
        else in the corpus moves and the only rule the fixture reaches is the
        offender scan of the row whose flag `written` names.

        `written` is a whole line rather than an attribute, because a gate is
        not only an attribute: a `cfg!` is an expression and has to be written
        as one. What follows it is an item either way.
        """
        return gate.WORKSPACE.replacing(
            self.PROBE, f"{written}\npub fn probe() {{}}\n"
        )

    def appending(self, path, addition):
        """The real corpus with `addition` at the foot of `path`.

        What a document argument is for the rules stated over a document:
        `main` takes the corpus, so every rule but the one under test goes on
        reading exactly what it reads over the real tree and the case asserts
        one new failure.

        No anchor guard, for the reason `rewriting`'s docstring gives one:
        appending cannot silently miss. A file this repository has moved is a
        `KeyError` naming the path, which is the sentence a guard would have
        written.
        """
        return gate.WORKSPACE.replacing(path, gate.WORKSPACE.raw[path] + addition)

    def naming(self, failures, needle):
        return [failure for failure in failures if needle in failure]

    def test_a_fixture_whose_anchor_has_moved_fails_naming_the_anchor(self):
        # `rewriting`'s guard, and the only case here about this file rather
        # than about `containment.py`. Without it a documentation edit the gate
        # accepts turns every anchored case below into `0 != 1`, none of them
        # naming what moved -- which is the failure the guard was written for,
        # and which nothing held.
        with self.assertRaises(self.failureException) as caught:
            self.rewriting("a document", "an anchor it does not hold", "x")
        self.assertIn(
            "this fixture no longer anchors on: 'an anchor it does not hold'",
            str(caught.exception),
        )

    def test_the_read_patch_is_put_back(self):
        # `reading`'s restore, which is the whole of this file's claim to
        # leaving `pathlib` as it found it. A leak here is invisible: the patch
        # rewrites one path and passes every other read through, so a case that
        # inherited it would go on passing while every read in the process ran
        # through a closure belonging to a case that has finished.
        original = Path.read_text
        with self.reading("crates/kynos/Cargo.toml", lambda text: text):
            self.assertIsNot(Path.read_text, original)
        self.assertIs(Path.read_text, original)

    def test_the_read_patch_is_put_back_when_the_body_raises(self):
        # The `finally`, which is the half a failing case reaches. Without it
        # the patch outlives the first case whose body raises -- which is any
        # case that fails inside the block -- and nothing observes it.
        original = Path.read_text
        with self.assertRaises(RuntimeError):
            with self.reading("crates/kynos/Cargo.toml", lambda text: text):
                raise RuntimeError("what a failing case does")
        self.assertIs(Path.read_text, original)

    def test_the_unmodified_tree_reports_nothing(self):
        # The one case here that hands `main` nothing at all, and the only one
        # that reads the failure list whole rather than through a needle. What
        # it holds is the emptiness: every rule below is asked what it reports
        # over a broken input, and none of them is asked what it reports over
        # an intact one, so a rule that over-reports goes unseen by the lot.
        #
        # Non-vacuous, and measured rather than argued: `Corpus.naming` reading
        # `sources` rather than `files` -- one word -- puts eleven offender
        # lines over this repository's own tree. Nine of them are the `tokio`
        # allowance scan's, over the `tests.rs` siblings that word restores to
        # the corpus, and two are the `hyper`/`hyper-util` and `rustls`
        # confinements'; the dependency-graph scan is two of the three
        # failures rather than all of them.
        #
        # Two other cases go red on it now, and did not when this was written:
        # `test_a_crate_name_matches_as_a_word_and_not_as_a_substring` and
        # `test_a_pub_use_in_lib_rs_is_exempt` each assert `(0, [])` over an
        # appended real corpus, so each detects the same class. The disclosure
        # is written beside both.
        #
        # The corpus rules are the ones this reaches that no document argument
        # would: `naming`'s own word boundaries, the views `Corpus.__init__`
        # derives, and every rule stated over `files` where `sources` would
        # have done. The status is asserted with the failures rather than
        # separately so that a red run prints what was reported.
        self.assertEqual(self.report(), (0, []))

    def test_a_renamed_allowance_header_skips_the_row_count_and_the_tokio_scan(self):
        broken = gate.ARCHITECTURE.replace(self.ALLOWANCE, "| Site | Named | Why |", 1)
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, self.ALLOWANCE)), 1)
        # Both rules over the table, and the scan especially: over an empty
        # allowance it reports every non-`server/` file in the crate.
        self.assertEqual(self.naming(failures, "allowance table claims"), [])
        self.assertEqual(self.naming(failures, "named outside `server/`"), [])

    #: One of the three links the surface section declares, removed from the
    #: whole document so that a widened slice cannot find it either. Without
    #: this, skipping and widening are indistinguishable: the widened slice
    #: holds every link the narrow one did, so both pass.
    DECLARED_SITE = "crates/kynos/src/response/stream/sse.rs"

    def test_a_stated_row_count_that_does_not_match_the_table_is_reported(self):
        # The presence half of the allowance case's `allowance table claims`
        # absence. The count word has to be one `NUMBERS` can read, or
        # `claimed` reports an unreadable count instead of the mismatch.
        broken = self.ROW_COUNT.sub(
            "**Seven rows, and the count is the check.**", gate.ARCHITECTURE, count=1
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "allowance table claims")), 1)

    def test_a_reworded_allowance_count_claim_is_reported(self):
        # `claimed`'s first branch. The sentence is reworded rather than
        # deleted, which is what a documentation edit does to it, and the count
        # word is left readable so the case cannot pass for the next branch's
        # reason.
        broken = self.ROW_COUNT.sub(
            lambda found: f"**{found.group(1)} rows, and that count is the check.**",
            gate.ARCHITECTURE,
            count=1,
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "no longer states a count matching")), 1)
        # And the rule the sentence holds stops running rather than comparing
        # against a number nobody wrote.
        self.assertEqual(self.naming(failures, "allowance table claims"), [])

    def test_an_unreadable_allowance_count_is_reported(self):
        # `claimed`'s second branch: the sentence is there and states a number
        # `NUMBERS` cannot read, which is a count nothing is holding. Loudly
        # rather than skipped, and this is what holds that choice.
        broken = self.ROW_COUNT.sub(
            "**Nineteen rows, and the count is the check.**", gate.ARCHITECTURE, count=1
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "writes an unreadable count")
        self.assertEqual(len(reported), 1)
        self.assertIn("Nineteen", reported[0])

    def test_a_tokio_site_the_table_stops_allowing_is_reported(self):
        # The presence half of its `named outside `server/`` absence: one real
        # allowed site is renamed to a path nothing occupies, so the file that
        # names tokio there becomes an offender.
        broken = self.rewriting(
            gate.ARCHITECTURE,
            "| `response/stream/sse.rs` | `tokio::time::{Instant, Sleep, sleep}` |",
            "| `x/y.rs` | `tokio::time::{Instant, Sleep, sleep}` |",
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "named outside `server/`")), 1)
        self.assertIn(
            "crates/kynos/src/response/stream/sse.rs",
            self.naming(failures, "named outside `server/`")[0],
        )

    def test_a_crate_named_outside_the_tree_its_row_allows_is_reported(self):
        # The dependency-graph stray scan, which reads the corpus and no
        # document: the five rules in that loop are written in this file rather
        # than read out of `architecture.md`, so a corpus is the only thing a
        # case can hand it. `matchit` is the router's, and `unchecked.rs` is not
        # under `router/`.
        corpus = self.appending(
            "crates/kynos/src/unchecked.rs", '\nuse matchit::Router;\n'
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "may be named only under `router/`")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/unchecked.rs", reported[0])

    def test_a_crate_name_matches_as_a_word_and_not_as_a_substring(self):
        # `Corpus.naming`'s word boundaries, which every rule in the loop below
        # and the `tokio` scan above are all stated through: one pattern, so
        # dropping them reports files that name no dependency at all. `h2` is
        # the shortest token in the table and the most substring-prone, which
        # is why the case is written on it.
        #
        # Both boundaries, since they drop one at a time: `h2_frames` holds
        # the trailing one and `frames_h2` the leading one, and a fixture with
        # only the first leaves `(h2|httparse)\b` matching nothing new.
        #
        # Both halves in one case too, so the absence is falsifiable by the
        # rule and not only by the mutation: the same file names `h2` for real
        # below and is reported for it.
        #
        # Held against a fixture rather than left to the intact-tree case
        # above. That case catches the mutation today, and only because three
        # files in this workspace happen to spell `h2` inside a longer
        # identifier; a rename tomorrow takes the hold away without touching
        # either the rule or the case.
        #
        # This asserts the failure list whole over a real corpus, so it is a
        # second detector for the over-reporting class
        # `test_the_unmodified_tree_reports_nothing` above was written for,
        # and reds on mutations that have nothing to do with `Corpus.naming`'s word boundaries.
        # Measured on six: `Corpus.naming` reading `sources` for `files` or
        # losing its word boundaries, the re-export scan's `lib.rs` exemption
        # disabled, either the `tower` or the `hyper`/`hyper-util` confinement
        # emptied, and `permitted`'s `server/` branch deleted. Every one reds
        # this case, the other detector and the intact-tree case together, so
        # a red here can misname its cause -- read the intact-tree case's
        # verdict first. That cost is the one the intact-tree case states and
        # accepts, and it is accepted here rather than designed away: an
        # absence asserted through a needle instead would hand this rule back
        # the blindness that case closes.
        inside = self.appending(
            "crates/kynos/src/unchecked.rs",
            "\nfn h2_frames() -> usize {\n    0\n}\nfn frames_h2() {}\n",
        )
        self.assertEqual(self.report(corpus=inside), (0, []))

        whole = self.appending(
            "crates/kynos/src/unchecked.rs", "\nuse h2::client::SendRequest;\n"
        )
        status, failures = self.report(corpus=whole)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "`h2` and `httparse` are never named")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/unchecked.rs", reported[0])

    def test_a_crate_confined_to_two_files_is_reported_outside_them(self):
        # The other branch of the same loop, and the one no case reached. The
        # row above is `UNDER` a tree; `sorted(found - where)` is what an
        # `ONLY_IN` row runs, and it ran over an empty difference every time.
        # So the `hyper` row could be widened to "anywhere" -- rewritten
        # `UNDER, ""`, which every path starts with -- and the suite stayed
        # green while the confinement held nothing. `unchecked.rs` is neither
        # `server/connection.rs` nor `http/body.rs`.
        corpus = self.appending(
            "crates/kynos/src/unchecked.rs", "\nuse hyper::body::Incoming;\n"
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "`hyper` and `hyper-util` are named only in")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/unchecked.rs", reported[0])

    def test_a_crate_confined_to_one_file_is_reported_outside_it(self):
        # Each row of that loop is a separate claim and widening one says
        # nothing about the others, so the branch being reached is not enough:
        # with `tower` rewritten `UNDER, ""` its row holds nothing at all while
        # the four beside it go on holding, and the run reports every rule
        # holds. `http/body.rs` is not `unchecked.rs`, which is the one file
        # this row allows.
        corpus = self.appending(
            "crates/kynos/src/http/body.rs", "\nuse tower::Service;\n"
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(
            failures, "`tower` and `tower-service` are named only in `unchecked.rs`"
        )
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/http/body.rs", reported[0])

    def test_a_renamed_surface_heading_skips_the_declaration_check(self):
        broken = gate.ARCHITECTURE.replace(self.SURFACE, "### The public surface", 1)
        broken = self.rewriting(broken, self.DECLARED_SITE, "crates/kynos/src/lib.rs")
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, self.SURFACE)), 1)
        # Skipped, so the undeclared check does not run. Were the slice to fall
        # back to the whole document -- `section(...) or architecture`, the
        # shape a `.split(...)[-1]` regression takes -- it would run over a
        # document that no longer declares the site above, and report it. That
        # is the silent widening the design exists to prevent, and it is what
        # separates the two here.
        self.assertEqual(self.naming(failures, "names no site"), [])

    def test_a_widened_surface_slice_would_report_the_site_this_one_hides(self):
        """The other half: that the fixture above can see a widened slice.

        An assertion that a failure class is absent holds nothing unless some
        input makes that class appear. This is that input -- the same document
        with the heading left alone, so the slice is taken and the removed link
        is missing from it.
        """
        broken = self.rewriting(
            gate.ARCHITECTURE, self.DECLARED_SITE, "crates/kynos/src/lib.rs"
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "names no site")), 1)
        self.assertIn(self.DECLARED_SITE, self.naming(failures, "names no site")[0])

    def test_a_renamed_surface_heading_leaves_the_stated_site_count_held(self):
        broken = gate.ARCHITECTURE.replace(self.SURFACE, "### The public surface", 1)
        broken = self.SITE_COUNT.sub(r"\g<1>Sixteen\g<2>", broken)
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        # The count reads the sites off the source, so losing the section does
        # not cost it. Moving it inside the guard would.
        self.assertEqual(len(self.naming(failures, "hand-rolled `Stream` sites and there are")), 1)

    def test_a_missing_end_marker_reports_rather_than_widening_the_slice(self):
        at = gate.ARCHITECTURE.index(self.SURFACE)
        broken = gate.ARCHITECTURE[:at] + gate.ARCHITECTURE[at:].replace("\n## ", "\n<> ")
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        # Without the end marker the slice runs to the foot of the document and
        # every link in it authorises a hand-rolled `Stream`, silently.
        self.assertEqual(len(self.naming(failures, repr("\n## "))), 1)
        # `section` interpolates `unrun` twice, once per marker. The grading
        # case holds the start message; this holds the end one.
        self.assertIn("goes unparsed", self.naming(failures, repr("\n## "))[0])
        # Decorative, deliberately and said so: a widened slice still holds
        # every link, so this absence is not falsifiable here. What holds the
        # class is the pair of surface cases above.
        self.assertEqual(self.naming(failures, "names no site"), [])

    def test_a_renamed_grading_header_skips_every_rule_over_the_grading(self):
        broken = gate.PERFORMANCE.replace(self.GRADING, "| Grade | Owes | Flag |", 1)
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, self.GRADING)), 1)
        for signature in ("does not grade", "does not declare", "in more than one row", "no row of"):
            self.assertEqual(self.naming(failures, signature), [], signature)
        # And the coverage comparison must not run at all rather than run over
        # an empty grading: moved out of the guard it reports that the
        # `Off-path proof` row is gone, which is false -- the row is there and
        # only the header changed -- and contradicts the `unrun` sentence
        # printed beside it. That message class is held live by
        # `OffPathCoverage` below, and this absence is falsifiable by the
        # statement-move mutation the pull request's ledger names.
        self.assertEqual(self.naming(failures, "no longer has a"), [])

    #: The Aggregate row, which the regrading case appends a flag to. Named as
    #: its own constant so the case says which row it writes into.
    AGGREGATE = "| Aggregate | nothing of its own; it is the union of what it enables |"

    def test_a_flag_no_row_grades_is_reported(self):
        # The presence half of the grading case's `does not grade` absence.
        broken = self.rewriting(gate.PERFORMANCE, "`cookie`, ", "")
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "does not grade")), 1)
        self.assertIn("cookie", self.naming(failures, "does not grade")[0])

    def test_a_graded_flag_the_crate_does_not_declare_is_reported(self):
        # The presence half of its `does not declare` absence.
        broken = self.rewriting(gate.PERFORMANCE, "`cookie`", "`kooky`")
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "does not declare")), 1)
        self.assertIn("kooky", self.naming(failures, "does not declare")[0])

    def test_a_flag_graded_in_two_rows_is_reported(self):
        # The presence half of its `in more than one row` absence.
        broken = self.rewriting(
            gate.PERFORMANCE,
            self.AGGREGATE + " `default`, `full` |",
            self.AGGREGATE + " `default`, `full`, `cookie` |",
        )
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "in more than one row")), 1)
        self.assertIn("cookie", self.naming(failures, "in more than one row")[0])

    def test_a_flag_graded_off_path_with_no_row_is_reported(self):
        # The presence half of its `no row of` absence. The flag is moved out of
        # the Full battery row and into the Off-path proof one, where the grade
        # says a request cannot reach what it adds -- an argument, and no row of
        # testing.md's off-path table makes it.
        broken = self.rewriting(
            self.rewriting(gate.PERFORMANCE, "`cookie`, ", ""),
            "`decimal-big` |",
            "`decimal-big`, `cookie` |",
        )
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "no row of")), 1)
        self.assertIn("cookie", self.naming(failures, "no row of")[0])

    def test_the_reported_marker_says_which_rules_stopped_running(self):
        # `section`'s `unrun` sentence, which the marker alone does not carry.
        # A reader handed only the missing marker knows what is gone and not
        # what stopped being held, and it is the second that decides whether
        # the build may proceed.
        broken = gate.PERFORMANCE.replace(self.GRADING, "| Grade | Owes | Flag |", 1)
        _, failures = self.report(performance=broken)
        reported = self.naming(failures, self.GRADING)[0]
        self.assertIn("goes unparsed", reported)
        self.assertIn("off-path coverage comparison", reported)

    def test_a_reworded_taxonomy_claim_is_reported(self):
        # `taxonomy_failures`, reached through the one line of `main` that
        # calls it. Its own suite holds what the helper returns; this holds
        # that `main` still adds it to `failures`, which is a separate claim
        # and the one a sweep over `failures.append` alone could not see --
        # replacing that line with `pass` disconnects the rule with both gates
        # green.
        broken = re.sub(
            gate.TAXONOMY_CLAIM,
            lambda found: f"All {found.group(1)} of the kinds listed below run today",
            gate.PERFORMANCE,
            count=1,
        )
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, "no longer states how many of the kinds below")), 1
        )

    def test_a_missing_off_path_header_holds_nothing_and_says_so(self):
        # `testing`. The off-path table is split at its header rather than
        # sliced, so this rule fails on its own terms -- but it fails only if
        # the document handed in is the one read.
        broken = gate.TESTING.replace(gate.OFF_PATH_HEADER, "| Element | Named by |", 1)
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "exactly one off-path table")), 1)

    #: The `uuid` off-path row, in the three cells the cases below rewrite. One
    #: row reaches every rule the off-path loop states per row, and naming its
    #: cells apart keeps each fixture to the one cell it breaks.
    UUID_ELEMENT = "| the `uuid` feature |"
    UUID_NAMED_BY = ' `uuid`, `feature = "uuid"` |'
    UUID_SITES = " `schema/impls/{mod,identifier}.rs` |"
    UUID_ROW = UUID_ELEMENT + UUID_NAMED_BY + UUID_SITES
    #: The row-count sentence, read off the gate rather than respelled here,
    #: and matched rather than written out for the reason `SITE_COUNT` is: a
    #: case about the count rule must not depend on today's count. One pattern
    #: for both documents, because one rule reads it out of both.
    ROW_COUNT = re.compile(gate.ROW_COUNT_CLAIM)

    def test_a_malformed_off_path_row_is_reported(self):
        # A cell lost with its pipe, which is what a hand-edited table does. The
        # row is refused before anything else reads it, so the row-count rule
        # fires beside this one; the shape failure is what is asserted.
        broken = self.rewriting(
            gate.TESTING, self.UUID_ROW, self.UUID_ELEMENT + self.UUID_NAMED_BY
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "off-path table has a malformed row")
        self.assertEqual(len(reported), 1)
        self.assertIn("the `uuid` feature", reported[0])

    def test_an_unreadable_off_path_site_cell_is_reported(self):
        # A *Named only in* cell writing prose outside its backticks. The scan
        # scope is derived from these sites, so the cell is refused whole rather
        # than read for the half of it that still parses.
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT
            + self.UUID_NAMED_BY
            + " `schema/impls/mod.rs` and `schema/impls/identifier.rs` |",
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "cannot read as a comma-separated list")
        self.assertEqual(len(reported), 1)
        self.assertIn("the `uuid` feature", reported[0])

    def test_an_off_path_site_in_a_misspelled_crate_is_reported(self):
        # One transposed letter in a crate name: every other check on the cell
        # passes, and the tree it derives matches no file, which would narrow
        # the row back to the home scope where its spellings are still written.
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT
            + self.UUID_NAMED_BY
            + " `crates/kynos-opanapi/src/schema/impls/mod.rs` |",
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "in a crate that does not exist")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos-opanapi/src/", reported[0])
        # And the skip that follows it. Dropping that `continue` leaves the row
        # scanned against a tree derived from a crate that is not there, which
        # is the narrowing back to the home scope this failure says did not
        # happen: the scan then reports every site the misspelled cell does not
        # list, on top of the failure above.
        self.assertEqual(self.naming(failures, "is off the request path"), [])

    def test_an_unreadable_off_path_named_by_cell_is_reported(self):
        # The same residue, in the cell that says what names the element. A
        # spelling dropped for having lost its backticks reads exactly like a
        # row with one spelling that holds.
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT + ' `uuid` and `feature = "uuid"` |' + self.UUID_SITES,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "cannot read as an identifier or a path of them")
        self.assertEqual(len(reported), 1)
        self.assertIn("the `uuid` feature", reported[0])

    def test_an_off_path_spelling_nothing_writes_is_reported(self):
        # A renamed or mistyped spelling, and the row's other spelling still
        # matching: this is the case the one-at-a-time hold exists for, since a
        # union over the cell would report the row as holding.
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT + ' `uuidd`, `feature = "uuid"` |' + self.UUID_SITES,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "writes that spelling")
        self.assertEqual(len(reported), 1)
        self.assertIn("`uuidd`", reported[0])

    def test_a_row_with_a_stale_spelling_is_not_also_scanned_for_offenders(self):
        # One failure per row, which is a rule of its own rather than an
        # accident of control flow: a cell this rule has just called
        # untrustworthy does not also get to render a verdict on the sites. The
        # fixture writes both faults at once -- a spelling nothing writes, and
        # sites narrowed to one file that leaves offenders standing -- so the
        # skip is the only thing between the row and a second failure.
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT + ' `uuidd`, `feature = "uuid"` |' + " `schema/mod.rs` |",
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "writes that spelling")), 1)
        self.assertEqual(self.naming(failures, "is off the request path"), [])

    def test_an_off_path_element_named_outside_its_sites_is_reported(self):
        # The offender scan, which is the rule the second defect of #134 exists
        # to correct: the row's sites are narrowed to one file, so every other
        # file naming the element is a site a request may now reach it from.
        site = "schema/mod.rs"
        broken = self.rewriting(
            gate.TESTING,
            self.UUID_ROW,
            self.UUID_ELEMENT + self.UUID_NAMED_BY + f" `{site}` |",
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "is off the request path")
        self.assertEqual(len(reported), 1)
        # Which file the scan names is read off the message rather than written
        # here. This rule tolerates a stale *site* deliberately -- "locations
        # may empty out while the claim stays true" -- so a rename that empties
        # one of the cell's files is an edit the gate accepts, and a case
        # naming that file would red `containment:test` over a rule that is
        # fine. What is held is that a site outside the cell this fixture just
        # narrowed was reported at all.
        offenders = [line.strip() for line in reported[0].split("\n")[1:] if line.strip()]
        self.assertTrue(offenders)
        self.assertNotIn(gate.OFF_PATH_SCOPE + site, offenders)

    def test_a_flag_named_at_a_site_no_row_allows_is_reported(self):
        # The offender scan again, reached through the corpus rather than
        # through the table, and over a gate-only row: `test-util` has no
        # companion identifier, so the gate is the whole of what names it and
        # the row's cell allows `lib.rs` alone.
        status, failures = self.report(
            corpus=self.probing('#[cfg(feature = "test-util")]')
        )
        self.assertEqual(status, 1)
        reported = self.naming(failures, "is off the request path")
        self.assertEqual(len(reported), 1)
        self.assertIn("`test-util` feature", reported[0])
        self.assertIn(self.PROBE, reported[0])

    def test_a_flag_named_negatively_at_a_site_no_row_allows_is_reported(self):
        # The same site and the same flag, gated on the flag being *off*. The
        # row says a request cannot reach the element, and a
        # `not(feature = "x")` names the flag and couples the site to it just
        # as the positive form does: what it compiles is code that exists in
        # every build the flag is off in, which is a build the row's reason
        # says nothing about. Reading the polarity here narrowed the scan to
        # one half of what a row claims, and for the four gate-only rows --
        # `test-util`, `time`, `decimal`, `openapi31` -- there is no companion
        # identifier to catch the other half, so the loss was total. Negated
        # gates are live idiom in this workspace: `lib.rs` writes four.
        status, failures = self.report(
            corpus=self.probing('#[cfg(not(feature = "test-util"))]')
        )
        self.assertEqual(status, 1)
        reported = self.naming(failures, "is off the request path")
        self.assertEqual(len(reported), 1)
        self.assertIn("`test-util` feature", reported[0])
        self.assertIn(self.PROBE, reported[0])

    def test_a_flag_named_by_a_cfg_macro_at_a_site_no_row_allows_is_reported(self):
        # The third form of the same gate at the same site, and the one the
        # attribute-anchored pattern could not see. `cfg!` compiles in every
        # configuration and branches at run time, so what it guards is on the
        # request path in every build -- which is more than either attribute
        # form can say, and the row's reason has to cover it. It lands on the
        # four gate-only rows the polarity defect landed on, for the same
        # reason: `test-util`, `time`, `decimal` and `openapi31` have no
        # companion identifier to catch the site by another spelling.
        #
        # Live idiom rather than a shape invented here: the workspace writes
        # eleven `cfg!(feature = "…")`, all naming `openapi32`, all under
        # `crates/kynos-macros/src/` or in a `tests.rs` sibling -- outside
        # every row's scope and outside `gate_files` -- which is why the tree
        # stays silent while this `crates/kynos/src/` site does not.
        status, failures = self.report(
            corpus=self.probing('const GATED: bool = cfg!(feature = "test-util");')
        )
        self.assertEqual(status, 1)
        reported = self.naming(failures, "is off the request path")
        self.assertEqual(len(reported), 1)
        self.assertIn("`test-util` feature", reported[0])
        self.assertIn(self.PROBE, reported[0])

    def test_an_emptied_off_path_table_is_reported(self):
        # Every row dropped, header and separator left standing. Each per-row
        # rule above reports nothing over an empty table, which is why the
        # table's emptiness is a rule of its own.
        head, tail = gate.TESTING.split(gate.OFF_PATH_HEADER, 1)
        lines = tail.split("\n")
        body = lines[2:]
        while body and body[0].startswith("|"):
            body.pop(0)
        broken = head + gate.OFF_PATH_HEADER + "\n".join(lines[:2] + body)
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "off-path table has no rows")), 1)

    def test_a_reworded_off_path_count_claim_is_reported(self):
        # The count sentence is what holds the row *set*, so losing it is a
        # failure of its own rather than a rule that quietly stops.
        broken = self.ROW_COUNT.sub(
            lambda found: f"**{found.group(1)} rows, and that count is the check.**",
            gate.TESTING,
            count=1,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, "no longer states how many rows its off-path table has")),
            1,
        )

    def test_an_unreadable_off_path_count_is_reported(self):
        # As `claimed`'s second branch, for the count this rule reads itself.
        broken = self.ROW_COUNT.sub(
            "**Nineteen rows, and the count is the check.**", gate.TESTING, count=1
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "unreadable off-path row count")
        self.assertEqual(len(reported), 1)
        self.assertIn("Nineteen", reported[0])

    def test_a_stated_off_path_count_that_does_not_match_the_table_is_reported(self):
        # A readable count the document does not state, so the claim and the
        # table disagree whatever the table's length is.
        written = self.ROW_COUNT.search(gate.TESTING).group(1)
        wrong = "Seven" if written != "Seven" else "Eight"
        broken = self.ROW_COUNT.sub(
            f"**{wrong} rows, and the count is the check.**", gate.TESTING, count=1
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, f"claims {gate.NUMBERS[wrong]} off-path rows")), 1
        )

    def test_a_moved_module_size_budget_is_reported(self):
        # `nfr`. The budget is a number in prose, and the rule holds it against
        # the count of files over the line.
        broken = re.sub(
            r"a module-size budget of \d+ files",
            "a module-size budget of 99 files",
            gate.NFR,
        )
        status, failures = self.report(nfr=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "nfr.md budgets 99 files")), 1)

    def test_a_reworded_module_size_budget_claim_is_reported(self):
        # The other half of the budget rule: the case above holds the
        # comparison, this holds the sentence the comparison reads.
        broken = re.sub(
            r"a module-size budget of (\d+) files",
            r"a module-size budget covering \1 files",
            gate.NFR,
        )
        status, failures = self.report(nfr=broken)
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, "no longer states the module-size budget")), 1
        )

    def test_an_optional_dependency_named_by_no_dep_is_reported(self):
        # `crates/kynos/Cargo.toml`, which `main` reads rather than takes.
        # `[features]` is left byte-identical, which is exactly what makes the
        # flag Cargo synthesises for this dependency invisible to the three
        # grading comparisons above the rule.
        manifest = "crates/kynos/Cargo.toml"
        broken = self.rewriting(
            (gate.ROOT / manifest).read_text(),
            "\n[dependencies]\n",
            '\n[dependencies]\nprobe-unnamed = { version = "0", optional = true }\n',
        )
        with self.reading(manifest, lambda _: broken):
            status, failures = self.report()
        self.assertEqual(status, 1)
        reported = self.naming(failures, "named by no `dep:`")
        self.assertEqual(len(reported), 1)
        self.assertIn("probe-unnamed", reported[0])

    def test_a_published_source_reading_above_its_package_is_reported(self):
        # The package-escape read, over a source `published()` yields. The
        # literal climbs out of `crates/kynos/` to a file the repository really
        # has, so what the rule reports is the escape and not a missing target.
        with self.reading(
            "crates/kynos/src/lib.rs",
            lambda text: text + '\nconst PROBE: &str = include_str!("../../../README.md");\n',
        ):
            status, failures = self.report()
        self.assertEqual(status, 1)
        reported = self.naming(failures, "resolves outside crates/kynos")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/lib.rs reads '../../../README.md'", reported[0])

    def test_a_pub_use_of_a_module_the_same_file_declares_is_reported(self):
        # The parent re-export scan. `http/body.rs` is not `lib.rs`, which is
        # the one file the rule exempts, and the module is declared in the same
        # file so the `pub use` names a second path to an item of ours.
        corpus = self.appending(
            "crates/kynos/src/http/body.rs",
            "\nmod probe_mod;\npub use probe_mod::Thing;\n",
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "re-publishes one of our own items")
        self.assertEqual(len(reported), 1)
        self.assertIn(
            "crates/kynos/src/http/body.rs: pub use probe_mod::...", reported[0]
        )

    def test_a_pub_use_of_crate_self_or_super_is_reported(self):
        # The three literal heads beside what the file declares.
        # `DECLARED_MODULE` finds a module the same file opens, and `crate`,
        # `self` and `super` name our own items while declaring nothing -- so
        # dropping them leaves the rule holding only the shape the case above
        # already writes, and every `pub use crate::…` in the crate goes
        # unreported. All three in one fixture, since a case naming one leaves
        # the other two droppable.
        corpus = self.appending(
            "crates/kynos/src/http/body.rs",
            "\npub use crate::http::Body as A;\n"
            "pub use self::Body as B;\n"
            "pub use super::Body as C;\n",
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "re-publishes one of our own items")
        self.assertEqual(len(reported), 1)
        for head in ("crate", "self", "super"):
            self.assertIn(
                f"crates/kynos/src/http/body.rs: pub use {head}::...", reported[0]
            )

    def test_a_pub_use_in_lib_rs_is_exempt(self):
        # The one exemption the scan makes, and the sibling of the presence
        # case above: the same two lines in `http/body.rs` are reported, and in
        # a `lib.rs` they are the crate root doing what CLAUDE.md says only it
        # may -- "the crate root and `kynos::prelude` are the only curated
        # shortcuts". Nothing distinguished an exemption from a rule that never
        # fires, so deleting the branch cost nothing here.
        #
        # Held against a fixture rather than left to the intact-tree case
        # above. That case catches the deletion today, and only because some
        # `lib.rs` in this workspace happens to write such a `pub use`: a
        # rename tomorrow takes the hold away without touching the rule or the
        # case, and the failure it prints names every re-export in the crate
        # rather than the branch that stopped guarding them.
        #
        # This asserts the failure list whole over a real corpus, so it is a
        # second detector for the over-reporting class
        # `test_the_unmodified_tree_reports_nothing` above was written for,
        # and reds on mutations that have nothing to do with the `lib.rs` exemption.
        # Measured on six: `Corpus.naming` reading `sources` for `files` or
        # losing its word boundaries, the re-export scan's `lib.rs` exemption
        # disabled, either the `tower` or the `hyper`/`hyper-util` confinement
        # emptied, and `permitted`'s `server/` branch deleted. Every one reds
        # this case, the other detector and the intact-tree case together, so
        # a red here can misname its cause -- read the intact-tree case's
        # verdict first. That cost is the one the intact-tree case states and
        # accepts, and it is accepted here rather than designed away: an
        # absence asserted through a needle instead would hand this rule back
        # the blindness that case closes.
        corpus = self.appending(
            "crates/kynos/src/lib.rs",
            "\nmod probe_mod;\npub use probe_mod::Thing;\npub use crate::http::Body as A;\n",
        )
        self.assertEqual(self.report(corpus=corpus), (0, []))

    def test_a_todo_body_is_reported(self):
        # The placeholder scan. Written as a body rather than in a doc example,
        # which is where every `todo!()` the tree really holds sits and which
        # `strip` has already removed by the time the rule runs.
        corpus = self.appending(
            "crates/kynos/src/unchecked.rs",
            "\npub fn probe() -> u8 {\n    todo!()\n}\n",
        )
        status, failures = self.report(corpus=corpus)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "stands in for a body")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/unchecked.rs", reported[0])

    def test_a_misspelled_dev_profile_table_is_reported(self):
        # `cargo_config_failures`, reached through the one line of `main` that
        # calls it: the second `failures += helper(...)`, and the second rule
        # whose helper had a suite of its own while nothing held the call. The
        # fault is the one the rule exists for -- a table cargo does not report
        # at all -- so `containment:check` is the only thing that could see it.
        config = ".cargo/config.toml"
        broken = self.rewriting(
            (gate.ROOT / config).read_text(),
            '[profile.dev.package."*"]',
            '[profile.dev.pakcage."*"]',
        )
        with self.reading(config, lambda _: broken):
            status, failures = self.report()
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, 'no longer declares `profile.dev.package."*".debug`')),
            1,
        )

    def test_a_script_written_above_the_declared_python_floor_is_reported(self):
        # `python_floor_failures`, reached through the one line of `main` that
        # calls it: the third `failures += helper(...)`, and the third rule
        # whose helper has a suite of its own while nothing holds the call. The
        # rule opens `scripts/` while `main` runs, so `reading` is what a case
        # reaches, the same way the `.cargo/config.toml` case above does.
        #
        # A PEP 695 `type` statement rather than the `match` statement
        # `PythonFloor` uses, because the floor here is the real one and 3.11
        # accepts `match`. Which of the rule's two sentences comes back depends
        # on the interpreter -- 3.12 and later name the version, the pinned
        # 3.11 can parse it at no version and says so -- so what is asserted is
        # the half both of them write.
        floor = ".".join(str(part) for part in gate.PYTHON_FLOOR)
        with self.reading(
            "scripts/cost_features.py", lambda text: text + "\ntype Raised = int\n"
        ):
            status, failures = self.report()
        self.assertEqual(status, 1)
        needle = f"scripts/cost_features.py does not parse at Python {floor}"
        self.assertEqual(len(self.naming(failures, needle)), 1)

    def test_a_script_task_that_stops_pinning_the_python_floor_is_reported(self):
        # `python_pin_failures`, reached through the one line of `main` that
        # calls it: the fourth `failures += helper(...)`, and the rule whose
        # whole subject is a file `main` opens while it runs. `reading` is what
        # a case reaches it with, as for `.cargo/config.toml` below.
        pin = 'tools = { python = "3.11.16" }\n'
        with self.reading(
            "mise.toml",
            lambda text: self.rewriting(
                text, pin + 'run = "python3 -B scripts/commits_test.py"',
                'run = "python3 -B scripts/commits_test.py"',
            ),
        ):
            status, failures = self.report()
        self.assertEqual(status, 1)
        reported = self.naming(failures, "pins no `python`")
        self.assertEqual(len(reported), 1)
        self.assertIn("commits:test", reported[0])


if __name__ == "__main__":
    unittest.main()
