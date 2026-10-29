"""Tests for the rules `tooling.py` holds the repository's tooling to.

Each rule here is a rule rather than a parser, and is tested for the reason
`containment_test.py` gives its rules: its failure mode is silence. Cargo
reports a misspelt config key as a warning over a successful build, and a
misspelt table not at all; a script written in newer grammar runs for whoever
wrote it; a task that lost its `python` pin runs under whatever interpreter the
machine has. Every gate in `mise run check` stays green over each of them.

The inputs are stated rather than read off disk, since what is under test is
what a rule does with a config and not what this repository's own says today.
`Main` is the exception, as it is in `containment_test.py`: it hands `main`
this repository's own files with one detail rewritten, which `main` takes as
arguments for that purpose, to hold that each helper is still called.

Run it as `mise run tooling:test`, or directly. There is no Python test runner
in this repository and `unittest` needs none.
"""

import contextlib
import io
import sys
import unittest
from pathlib import Path

# Before the import below: a run, through the task or directly, writes no
# `.pyc` into the tree it reads.
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))

import tooling as gate  # noqa: E402  (the path insert must come first)


class CargoConfig(unittest.TestCase):
    """What `.cargo/config.toml` must declare, and what it may not declare.

    The silence this rule exists for is cargo's. A key that has lost a letter
    is `warning: unused config key` and an exit status of zero; a profile
    *table* that has lost one is not reported at all. Both were run against a
    scratch package before the rule was written, and both finished
    `unoptimized + debuginfo`. So every case below is a config the real tree
    does not contain and every gate in `mise run check` compiles happily over.
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


class PythonFloor(unittest.TestCase):
    """Which scripts are reported as having raised the grammar floor.

    A file written in newer grammar runs perfectly for whoever wrote it and is
    a `SyntaxError` before any rule or any case runs for everybody else, which
    is the shape of #134's import-time defect one layer down. Nothing in this
    repository reported it until this rule, and the only record that the floor
    had already moved once is a commit message.

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
        # failure and it is `tooling:test` that stops running, not the one
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
    # runtime half -- that `import tomllib` at the head of `tooling.py` needs
    # 3.11 whatever the grammar allows -- is held by nothing here and by
    # nothing anywhere else either: it is held by the gate running under the
    # pin, where a pin below 3.11 fails at that import before a rule runs.
    # `tooling.py`'s own comment on `PYTHON_FLOOR` records that.


class PythonPins(unittest.TestCase):
    """Whether `mise.toml` still pins the floor `tooling.py` declares.

    The other half of `PythonFloor`, and the half that was written in nine
    places with nothing holding them together. Its silence is the sharpest
    here: setting `PYTHON_FLOOR` two minors above every pin printed `every rule
    holds` and left every case green, and so did a ninth task under `scripts/`
    carrying no pin at all.

    `Main` below reaches the real `mise.toml`.
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

    def failures(self, *tasks, scripts=("probe.py",), floor=(3, 11)):
        return gate.python_pin_failures(self.config(*tasks), set(scripts), floor)

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

    def test_an_inline_interpreter_is_named(self):
        failures = self.failures(("probe", "python3 -c 'print(1)'", None))
        self.assertEqual(len(failures), 1)
        self.assertIn("something other than a file in scripts/", failures[0])
        self.assertIn("probe", failures[0])

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
    """That `main` still calls each helper, over this repository's own files.

    A helper held to its own cases says nothing about whether anything calls
    it: silencing one `failures += helper(...)` line left the gate printing
    that every rule holds with the whole rule disconnected. So each case below
    hands `main` one of this repository's files with one detail rewritten and
    asserts the failure that helper writes. The success path is held too, since
    every other case filters failures by a needle and a rule over-reporting on
    the real tree is invisible to all of them.
    """

    def report(self, **given):
        """`main`'s status and what it reported, with its own output held."""
        err, out = io.StringIO(), io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(out):
            status = gate.main(**given)
        head, failures = "tooling: ", []
        for line in err.getvalue().split("\n"):
            if line.startswith(head):
                failures.append(line[len(head) :])
            elif line.strip() and failures:
                failures[-1] += "\n" + line
        return status, failures

    def rewriting(self, document, anchor, replacement):
        """`document` with `anchor` rewritten, refusing a no-op."""
        if anchor not in document:
            self.fail(f"this fixture no longer anchors on: {anchor!r}")
        return document.replace(anchor, replacement)

    def naming(self, failures, needle):
        return [failure for failure in failures if needle in failure]

    def test_this_repository_holds_every_rule(self):
        self.assertEqual(self.report(), (0, []))

    def test_a_misspelled_dev_profile_table_is_reported(self):
        broken = self.rewriting(
            gate.CARGO_CONFIG.read_text(),
            '[profile.dev.package."*"]',
            '[profile.dev.pakcage."*"]',
        )
        status, failures = self.report(cargo_config=broken)
        self.assertEqual(status, 1)
        self.assertEqual(
            len(self.naming(failures, 'no longer declares `profile.dev.package."*".debug`')),
            1,
        )

    def test_a_script_written_above_the_declared_python_floor_is_reported(self):
        # A PEP 695 `type` statement rather than the `match` statement
        # `PythonFloor` uses, because the floor here is the real one and 3.11
        # accepts `match`. Which of the rule's two sentences comes back depends
        # on the interpreter -- 3.12 and later name the version, the pinned
        # 3.11 can parse it at no version and says so -- so what is asserted is
        # the half both of them write.
        floor = ".".join(str(part) for part in gate.PYTHON_FLOOR)
        scripts = [
            (path.relative_to(gate.ROOT).as_posix(), path.read_text())
            for path in sorted(gate.SCRIPTS.glob("*.py"))
        ]
        raised = "scripts/cost_features.py"
        self.assertIn(raised, dict(scripts))
        scripts = [
            (path, text + "\ntype Raised = int\n" if path == raised else text)
            for path, text in scripts
        ]
        status, failures = self.report(scripts=scripts)
        self.assertEqual(status, 1)
        needle = f"{raised} does not parse at Python {floor}"
        self.assertEqual(len(self.naming(failures, needle)), 1)

    def test_a_script_task_that_stops_pinning_the_python_floor_is_reported(self):
        pin = 'tools = { python = "3.11.16" }\n'
        broken = self.rewriting(
            gate.MISE_CONFIG.read_text(),
            pin + 'run = "python3 scripts/commits_test.py"',
            'run = "python3 scripts/commits_test.py"',
        )
        status, failures = self.report(mise=broken)
        self.assertEqual(status, 1)
        reported = self.naming(failures, "pins no `python`")
        self.assertEqual(len(reported), 1)
        self.assertIn("commits:test", reported[0])


if __name__ == "__main__":
    unittest.main()
