"""Tests for the parsers `containment.py` reads its tables and sources with, and
for the one rule stated over them whose own failure is silence.

Most of what is under test here takes text and returns a corpus, a set of paths
or a list of patterns. The rules stated over those are not tested: they read the
real tree, and running them is what `containment:check` is.

Two rules are the exception, and both are here for the same property.
`off_path_coverage` compares two documents and reads a grade out of one by
name, so a name that has gone empties the compared set rather than the table --
the failure mode a parser has, in a rule. `cargo_config_failures` reads a
configuration file that nothing else in this repository observes, and the
mistake it exists to catch is one cargo itself reports as a warning over a
successful build, or does not report at all. Both have their inputs stated
below rather than read off disk, since what is under test is what the rule does
with a document or a config and not what this repository's own happen to say.

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

    Structural rather than behavioural, and said so: the behavioural half is
    `Section` above, which is what fails when a heading is renamed. These two
    say where the rules live, which is what makes that guard's own comment true.
    """

    def test_the_module_holds_no_failure_list_at_import(self):
        self.assertFalse(hasattr(gate, "failures"))

    def test_every_rule_is_reachable_as_main(self):
        self.assertTrue(callable(getattr(gate, "main", None)))


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
    repository writes. Sixty-three `all(`/`any(` sites and eighteen `not(`
    sites make compound predicates the norm here rather than the exotic case.
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

    def test_a_gate_inside_a_string_literal_is_not_one(self):
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
    the rule goes on reporting against text it was written to exclude. Every
    case below fails against exactly that mutation and passes against the code
    as written.

    The documents are this repository's own with one marker removed, and the
    corpora are the real ones. What is under test here is a *rule* rather than a
    parser: the `tokio` scan is only wrong about the tree it reads, and a
    synthetic tree would make the case a test of its own fixture. `main` takes
    the documents as arguments for this reason, the way `scanned` takes
    `exists`.

    The assertions name the rules that must and must not have run rather than
    counting failures, so a case says what it holds and does not fail for a
    reason belonging to `containment:check`. Nothing here writes to the
    repository or to module state.
    """

    ALLOWANCE = "| Site | Names | Why it is not in `server/` |"
    SURFACE = "### Public API surface"
    GRADING = "| Grade | Owes | Flags |"
    #: `architecture.md`'s count of hand-rolled `Stream` sites, read as written
    #: so a case does not depend on today's number.
    SITE_COUNT = re.compile(r"(\*\*One public row, )\w+( sites, and the count is the check\*\*)")

    def report(self, **documents):
        """`main`'s status and what it reported, with its own output held."""
        err, out = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
            status = gate.main(**documents)
        head = "containment: "
        return status, [
            line[len(head) :] for line in err.getvalue().split("\n") if line.startswith(head)
        ]

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

    def test_a_renamed_surface_heading_skips_the_declaration_check(self):
        broken = gate.ARCHITECTURE.replace(self.SURFACE, "### The public surface", 1)
        status, failures = self.report(architecture=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, self.SURFACE)), 1)
        self.assertEqual(self.naming(failures, "names no site"), [])

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
        self.assertEqual(self.naming(failures, "names no site"), [])

    def test_a_renamed_grading_header_skips_every_rule_over_the_grading(self):
        broken = gate.PERFORMANCE.replace(self.GRADING, "| Grade | Owes | Flag |", 1)
        status, failures = self.report(performance=broken)
        self.assertEqual(status, 1)
        self.assertEqual(len(self.naming(failures, self.GRADING)), 1)
        for signature in ("does not grade", "does not declare", "in more than one row", "no row of"):
            self.assertEqual(self.naming(failures, signature), [], signature)

    def test_the_manifest_rule_below_the_grading_still_runs(self):
        broken = gate.PERFORMANCE.replace(self.GRADING, "| Grade | Owes | Flag |", 1)
        status, failures = self.report(performance=broken)
        # `implicit` reads the manifest alone and is skipped by nothing here.
        self.assertEqual(status, 1)
        self.assertEqual(self.naming(failures, "named by no `dep:`"), [])

    def test_a_reported_failure_exits_nonzero(self):
        broken = gate.PERFORMANCE.replace(self.GRADING, "| Grade | Owes | Flag |", 1)
        status, failures = self.report(performance=broken)
        self.assertTrue(failures)
        self.assertNotEqual(status, 0)


if __name__ == "__main__":
    unittest.main()
