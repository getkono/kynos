"""Tests for the four parsers `containment.py` reads its tables and sources with.

Everything under test here takes text and returns a corpus, a set of paths or a
list of patterns. The rules stated over them are not tested: those read the real
tree, and running them is what `containment:check` is.

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


class Scanned(unittest.TestCase):
    """The trees a row's own sites put it in reach of."""

    def test_a_row_with_no_crate_qualified_site_scans_its_home_scope(self):
        sites = gate.allowed_sites("`router/dispatch.rs`, `router/describe.rs`")
        self.assertEqual(gate.scanned(sites), ["crates/kynos/src/"])

    def test_a_crate_qualified_site_opens_that_crate_to_the_scan(self):
        sites = gate.allowed_sites(
            "`router/dispatch.rs`, `crates/kynos-openapi/src/emit/mod.rs`"
        )
        self.assertEqual(
            gate.scanned(sites),
            ["crates/kynos-openapi/src/", "crates/kynos/src/"],
        )

    def test_a_crate_path_outside_a_src_tree_opens_nothing(self):
        self.assertEqual(
            gate.scanned({"crates/kynos-openapi/Cargo.toml"}), ["crates/kynos/src/"]
        )


if __name__ == "__main__":
    unittest.main()
