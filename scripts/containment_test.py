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
"""

import contextlib
import io
import re
import sys
import unittest
from pathlib import Path

# Before the import below, and before anything else can trigger one: a `.pyc`
# written beside the scripts would be an untracked directory in every working
# tree that ran these tests, and `.gitignore` has no entry for one. The task
# passes `-B` for the same reason; this covers a direct `python3` run.
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
        an untracked `scripts/__pycache__/`, the second is how `containment` is
        imported at all, and undoing either would undo the import; `sys.stdout`
        and `sys.stderr`, swapped here and in `Main.report` by
        `contextlib.redirect_*`, which restores them on the way out; and
        `pathlib.Path.read_text`, restored below. The first two are on master
        and predate this branch.
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
    that compiles nothing in any configuration, and by any nesting of the
    three. Matched as text a spelling could tell none of them apart, so a
    `#[cfg(not(feature = "uuid"))]` in a file the `uuid` row does not allow
    failed the build over code that exists only when `uuid` is off. Read as a
    predicate instead: each `#[cfg(`, `#![cfg(` and `#[cfg_attr(` is walked
    with its parentheses balanced, and the flag is matched at the polarity the
    cell asked for.

    The fragments are written for this file, with one exception: the compound
    predicate below is copied from `crates/kynos/src/lib.rs`, because a case
    about reading a nested `not(any(` should be a case about one this
    repository writes. Compound predicates are routine here rather than the
    exotic case, which is why a case about one belongs in this file.

    One known limit, recorded rather than fixed: the walk finds its attributes
    over the whole corpus, so an attribute written inside a *raw* string --
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

    #: The five kinds, in the order the shipped table writes them.
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
    corpora are the real ones. What is under test here is a *rule* rather than a
    parser: the `tokio` scan is only wrong about the tree it reads, and a
    synthetic tree would make the case a test of its own fixture. `main` takes
    the documents as arguments for this reason, the way `scanned` takes
    `exists`.

    The assertions name the rules that must and must not have run rather than
    counting failures, so a case says what it holds and does not fail for a
    reason belonging to `containment:check`. Nothing here writes to the
    repository, and the only process state it touches is `sys.stdout` and
    `sys.stderr`, swapped by `contextlib.redirect_*`, which restores them.

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
    cost of that. Every absence assertion below is now either paired with a
    presence case or labelled in place as decorative, and there is one of the
    latter: the `"names no site"` check in the end-marker case, which no input
    here can falsify because a widened slice still holds every link. The class
    it names is held by the two surface cases instead. A new absence assertion
    joins that inventory or it does not go in.

    The rule that cost a case: `main` reads `crates/kynos/Cargo.toml` off
    `ROOT`, so the implicit-optional-dependency check below the grading is not
    injectable, and a case claiming it survives a missing grading header could
    only assert that it reported nothing. Holding it means making the manifest
    an argument too, which is a change to `main` nobody has needed yet.

    Left unheld deliberately, and on severity rather than on cost, because that
    is the part worth keeping: wrapping that rule in the grading guard makes it
    skipped only when `grading is None`, and that arises only when the grading
    header has been renamed -- which has already appended a loud failure, is
    returning 1, and says in its own message which rules stopped running. So the
    unheld mutant costs one rule quietly not running inside an already-red gate
    that enumerates them. Every mutant this file does hold left a *green* gate
    over a live defect. A mutant with no silent pass in it is a different class
    from one whose whole cost is a silent pass, and only the second kind is
    worth widening a signature for.

    `main`'s success path -- `return 0` and the report line -- is held by
    `containment:check` rather than here, which is the right allocation:
    running every rule over the intact tree is what that gate is, and repeating
    it here would make this file fail for its reasons.
    """

    ALLOWANCE = "| Site | Names | Why it is not in `server/` |"
    SURFACE = "### Public API surface"
    GRADING = "| Grade | Owes | Flags |"
    #: `architecture.md`'s count of hand-rolled `Stream` sites, read as written
    #: so a case does not depend on today's number.
    SITE_COUNT = re.compile(r"(\*\*One public row, )\w+( sites, and the count is the check\*\*)")

    def report(self, **documents):
        """`main`'s status and what it reported, with its own output held.

        A failure is one `containment: ` line plus the indented lines under it,
        joined: several of these messages name the offending paths below the
        sentence, and a case asserting which path was reported has to see them.
        """
        err, out = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
            status = gate.main(**documents)
        head, failures = "containment: ", []
        for line in err.getvalue().split("\n"):
            if line.startswith(head):
                failures.append(line[len(head) :])
            elif line.strip() and failures:
                failures[-1] += "\n" + line
        return status, failures

    def naming(self, failures, needle):
        return [failure for failure in failures if needle in failure]

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
        broken = re.sub(
            r"\*\*(\w+) rows, and the count is the check\.\*\*",
            "**Seven rows, and the count is the check.**",
            gate.ARCHITECTURE,
            count=1,
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "allowance table claims")), 1)

    def test_a_reworded_allowance_count_claim_is_reported(self):
        # `claimed`'s first branch. The sentence is reworded rather than
        # deleted, which is what a documentation edit does to it, and the count
        # word is left readable so the case cannot pass for the next branch's
        # reason.
        broken = re.sub(
            r"\*\*(\w+) rows, and the count is the check\.\*\*",
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
        broken = re.sub(
            r"\*\*(\w+) rows, and the count is the check\.\*\*",
            "**Nineteen rows, and the count is the check.**",
            gate.ARCHITECTURE,
            count=1,
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
        broken = gate.ARCHITECTURE.replace(
            "| `response/stream/sse.rs` | `tokio::time::{Instant, Sleep, sleep}` |",
            "| `x/y.rs` | `tokio::time::{Instant, Sleep, sleep}` |",
            1,
        )
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "named outside `server/`")), 1)
        self.assertIn(
            "crates/kynos/src/response/stream/sse.rs",
            self.naming(failures, "named outside `server/`")[0],
        )

    def test_a_renamed_surface_heading_skips_the_declaration_check(self):
        broken = gate.ARCHITECTURE.replace(self.SURFACE, "### The public surface", 1)
        broken = broken.replace(self.DECLARED_SITE, "crates/kynos/src/lib.rs")
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
        broken = gate.ARCHITECTURE.replace(self.DECLARED_SITE, "crates/kynos/src/lib.rs")
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
        broken = gate.PERFORMANCE.replace("`cookie`, ", "", 1)
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "does not grade")), 1)
        self.assertIn("cookie", self.naming(failures, "does not grade")[0])

    def test_a_graded_flag_the_crate_does_not_declare_is_reported(self):
        # The presence half of its `does not declare` absence.
        broken = gate.PERFORMANCE.replace("`cookie`", "`kooky`", 1)
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, "does not declare")), 1)
        self.assertIn("kooky", self.naming(failures, "does not declare")[0])

    def test_a_flag_graded_in_two_rows_is_reported(self):
        # The presence half of its `in more than one row` absence.
        broken = gate.PERFORMANCE.replace(
            self.AGGREGATE + " `default`, `full` |",
            self.AGGREGATE + " `default`, `full`, `cookie` |",
            1,
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
        broken = gate.PERFORMANCE.replace("`cookie`, ", "", 1).replace(
            "`decimal-big` |", "`decimal-big`, `cookie` |", 1
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
    #: `testing.md`'s count of off-path rows, read as written for the reason
    #: `SITE_COUNT` is: a case about the count rule must not depend on today's
    #: count.
    OFF_PATH_COUNT = re.compile(r"\*\*(\w+) rows, and the count is the check\.\*\*")

    def test_a_malformed_off_path_row_is_reported(self):
        # A cell lost with its pipe, which is what a hand-edited table does. The
        # row is refused before anything else reads it, so the row-count rule
        # fires beside this one; the shape failure is what is asserted.
        broken = gate.TESTING.replace(
            self.UUID_ROW, self.UUID_ELEMENT + self.UUID_NAMED_BY, 1
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
        broken = gate.TESTING.replace(
            self.UUID_ROW,
            self.UUID_ELEMENT
            + self.UUID_NAMED_BY
            + " `schema/impls/mod.rs` and `schema/impls/identifier.rs` |",
            1,
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
        broken = gate.TESTING.replace(
            self.UUID_ROW,
            self.UUID_ELEMENT
            + self.UUID_NAMED_BY
            + " `crates/kynos-opanapi/src/schema/impls/mod.rs` |",
            1,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "in a crate that does not exist")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos-opanapi/src/", reported[0])

    def test_an_unreadable_off_path_named_by_cell_is_reported(self):
        # The same residue, in the cell that says what names the element. A
        # spelling dropped for having lost its backticks reads exactly like a
        # row with one spelling that holds.
        broken = gate.TESTING.replace(
            self.UUID_ROW,
            self.UUID_ELEMENT + ' `uuid` and `feature = "uuid"` |' + self.UUID_SITES,
            1,
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
        broken = gate.TESTING.replace(
            self.UUID_ROW,
            self.UUID_ELEMENT + ' `uuidd`, `feature = "uuid"` |' + self.UUID_SITES,
            1,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "writes that spelling")
        self.assertEqual(len(reported), 1)
        self.assertIn("`uuidd`", reported[0])

    def test_an_off_path_element_named_outside_its_sites_is_reported(self):
        # The offender scan, which is the rule the second defect of #134 exists
        # to correct: the row's sites are narrowed to one file, so every other
        # file naming the element is a site a request may now reach it from.
        broken = gate.TESTING.replace(
            self.UUID_ROW,
            self.UUID_ELEMENT + self.UUID_NAMED_BY + " `schema/mod.rs` |",
            1,
        )
        status, failures = self.report(testing=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "is off the request path")
        self.assertEqual(len(reported), 1)
        self.assertIn("crates/kynos/src/schema/impls/identifier.rs", reported[0])

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
        broken = self.OFF_PATH_COUNT.sub(
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
        broken = self.OFF_PATH_COUNT.sub(
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
        written = self.OFF_PATH_COUNT.search(gate.TESTING).group(1)
        wrong = "Seven" if written != "Seven" else "Eight"
        broken = self.OFF_PATH_COUNT.sub(
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




if __name__ == "__main__":
    unittest.main()
