"""Tests that every commit a series lands is held to the per-commit gates, over
real repositories rather than a reading of the configuration.

`OverARange` drives `mise run commits:gates` -- the task CI's job and the
`pre-push` hook both run -- at a throwaway repository whose own `mise.toml`
declares each gate as a stub that records the commit it ran at and fails on a
marker in the source. What each case asserts is which commits and gates ran,
and in which tree, so a gate that silently checked the tip, the checkout or
nothing fails here. `ThePrePushHookHkRuns` installs the pinned `hk run
pre-push` over this repository's own `hk.pkl` and asks a real `git push`
whether the gate is armed.

Run as `mise run commits:gates:test`, or directly.
"""

import json
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path

import commit_gates
from commits_test import hermetic_environment, hk_binary

ROOT = Path(__file__).resolve().parent.parent
GATE_NAMES = [name for name, _ in commit_gates.GATES]
SOURCE = "crates/a.rs"


def stub_tasks():
    """A `mise.toml` declaring each gate as a stub over `SOURCE`.

    Each logs `<gate> <commit>` to `$GATE_LOG`, read with `git rev-parse` in
    the directory it runs from, then fails where `SOURCE` holds `red <gate>`.
    """
    body = 'echo {name} $(git rev-parse HEAD) >> "$GATE_LOG"; ! grep -qx "red {name}" {source}'
    return "".join(
        f'[tasks."{name}"]\nrun = {json.dumps(body.format(name=name, source=SOURCE))}\n'
        for name in GATE_NAMES
    )


class TheGateTable(unittest.TestCase):
    """What the selection reads: the table itself, against the tasks it names."""

    def test_every_gate_is_a_task_mise_declares(self):
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        self.assertEqual([name for name in GATE_NAMES if name not in tasks], [])

    def test_a_source_change_selects_every_gate_and_a_document_none(self):
        self.assertEqual(commit_gates.selected(["crates/kynos/src/lib.rs"]), GATE_NAMES)
        self.assertEqual(commit_gates.selected(["README.md", "docs/nfr.md"]), [])

    def test_only_a_lockfile_change_leaves_the_fresh_msrv_resolution_out(self):
        self.assertEqual(
            commit_gates.selected(["Cargo.lock"]), ["lint", "docs:check", "api:check"]
        )


class RangeTestCase(unittest.TestCase):
    """A throwaway repository with a remote, its gates stubbed by `stub_tasks`."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.repository = self.directory / "repository"
        self.remote = self.directory / "remote.git"
        self.log = self.directory / "gates.log"
        self.env = {
            **hermetic_environment(),
            "GATE_LOG": str(self.log),
            "MISE_TRUSTED_CONFIG_PATHS": str(self.directory),
        }
        self.git("init", "-q", "--bare", str(self.remote), cwd=self.directory)
        self.repository.mkdir()
        self.git("-c", "init.defaultBranch=master", "init", "-q", ".")
        self.git("remote", "add", "origin", str(self.remote))
        self.populate()
        self.write(SOURCE, "green\n")
        self.git("add", ".")
        self.git("commit", "-q", "--no-verify", "-m", "chore: the base")
        self.base = self.head()
        self.git("push", "-q", "--no-verify", "origin", "master")

    def populate(self):
        """The base commit's files besides `SOURCE`: the stub gates."""
        self.write("mise.toml", stub_tasks())

    def git(self, *arguments, cwd=None):
        return subprocess.run(
            ["git", *arguments],
            cwd=cwd or self.repository,
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )

    def write(self, path, text):
        target = self.repository / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def head(self):
        return self.git("rev-parse", "HEAD").stdout.strip()

    def commit(self, path, text, subject):
        self.write(path, text)
        self.git("add", path)
        self.git("commit", "-q", "--no-verify", "-m", subject)
        return self.head()

    def gates(self, commit_range=None):
        """`mise run commits:gates` from this repository's root, at the fixture.

        `GIT_DIR` and `GIT_WORK_TREE` name the fixture the way git's own hook
        environment names a repository, which the script has to read once and
        then keep away from the gates it runs.
        """
        environment = {
            **self.env,
            "GIT_DIR": str(self.repository / ".git"),
            "GIT_WORK_TREE": str(self.repository),
        }
        if commit_range is not None:
            environment[commit_gates.RANGE_VARIABLE] = commit_range
        return subprocess.run(
            ["mise", "run", "--quiet", "commits:gates"],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
        )

    def ran(self):
        """`(gate, commit)` for each gate run, in order."""
        if not self.log.exists():
            return []
        return [tuple(line.split()) for line in self.log.read_text().splitlines()]

    def assertPassed(self, run):
        self.assertEqual(run.returncode, 0, f"stdout:\n{run.stdout}\nstderr:\n{run.stderr}")


class OverARange(RangeTestCase):
    def test_each_gate_runs_in_the_tree_its_own_commit_records(self):
        first = self.commit(SOURCE, "green one\n", "feat: one")
        second = self.commit(SOURCE, "green two\n", "feat: two")
        self.assertPassed(self.gates(f"{self.base}..HEAD"))
        self.assertEqual(
            self.ran(),
            [(gate, first) for gate in GATE_NAMES] + [(gate, second) for gate in GATE_NAMES],
        )

    def test_a_commit_red_mid_series_fails_though_the_tip_is_green(self):
        red = self.commit(SOURCE, "red docs:check\n", "feat: red")
        self.commit(SOURCE, "green\n", "fix: green again")
        run = self.gates(f"{self.base}..HEAD")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn(f"{red[:12]} feat: red fails `docs:check`", run.stderr)
        self.assertEqual(self.ran(), [("lint", red), ("docs:check", red)])

    def test_a_commit_changing_no_gate_input_runs_no_gate(self):
        self.commit("README.md", "words\n", "docs: words")
        self.assertPassed(self.gates(f"{self.base}..HEAD"))
        self.assertEqual(self.ran(), [])

    def test_a_merge_commit_is_not_checked(self):
        self.git("switch", "-q", "-c", "topic")
        topic = self.commit(SOURCE, "topic\n", "feat: topic")
        self.git("switch", "-q", "master")
        self.commit("README.md", "trunk\n", "docs: trunk")
        # Against its first parent the merge changes `SOURCE`, so only the
        # merge rule keeps it out.
        self.git("merge", "-q", "--no-ff", "--no-edit", "topic")
        self.assertPassed(self.gates(f"{self.base}..HEAD"))
        self.assertEqual({commit for _, commit in self.ran()}, {topic})

    def test_what_the_checkout_holds_uncommitted_reaches_no_gate(self):
        green = self.commit(SOURCE, "green\nmore\n", "feat: green")
        self.write(SOURCE, "red lint\n")
        self.write("crates/untracked.rs", "red lint\n")
        self.assertPassed(self.gates(f"{self.base}..HEAD"))
        self.assertEqual(self.ran(), [(gate, green) for gate in GATE_NAMES])
        self.assertEqual((self.repository / SOURCE).read_text(), "red lint\n")
        self.assertTrue((self.repository / "crates/untracked.rs").exists())

    def test_the_default_range_is_what_a_push_would_publish(self):
        self.commit(SOURCE, "pushed\n", "feat: pushed")
        self.git("push", "-q", "--no-verify", "origin", "master")
        unpushed = self.commit(SOURCE, "unpushed\n", "feat: unpushed")
        self.assertPassed(self.gates())
        self.assertEqual(self.ran(), [(gate, unpushed) for gate in GATE_NAMES])

    def test_the_worktree_is_removed_after_a_failure(self):
        self.commit(SOURCE, "red lint\n", "feat: red")
        self.assertNotEqual(self.gates(f"{self.base}..HEAD").returncode, 0)
        listed = self.git("worktree", "list", "--porcelain").stdout
        self.assertEqual(listed.count("worktree "), 1, listed)

    def test_an_exported_empty_range_is_refused_rather_than_defaulted(self):
        self.commit(SOURCE, "red lint\n", "feat: red")
        run = self.gates("")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn("names no range", run.stderr)
        self.assertEqual(self.ran(), [])


class ThePrePushHookHkRuns(RangeTestCase):
    """A real `git push`, refused or published by the hook hk runs.

    The fixture carries this repository's `hk.pkl` verbatim and the
    `commits:gates` task as `mise.toml` declares it, so turning the step off,
    narrowing it to a glob a source change misses, or pointing it at another
    task turns the refusal below green.
    """

    def populate(self):
        task = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]["commits:gates"]
        self.write(
            "mise.toml",
            stub_tasks()
            + '[tasks."commits:gates"]\n'
            + f"run = {json.dumps(task['run'])}\n"
            + f"tools = {{ python = {json.dumps(task['tools']['python'])} }}\n",
        )
        shutil.copy(ROOT / "hk.pkl", self.repository / "hk.pkl")
        self.write("scripts/commit_gates.py", (ROOT / "scripts/commit_gates.py").read_text())
        hook = self.repository / ".git" / "hooks" / "pre-push"
        hook.parent.mkdir(parents=True, exist_ok=True)
        hook.write_text(f'#!/bin/sh\nexec "{hk_binary()}" run pre-push "$@"\n')
        hook.chmod(0o755)
        # Discovery stops here, so no `mise.toml` above the fixture merges in.
        self.env["MISE_CEILING_PATHS"] = str(self.directory)

    def push(self):
        return subprocess.run(
            ["git", "push", "origin", "master"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )

    def remote_head(self):
        return self.git("rev-parse", "master", cwd=self.remote).stdout.strip()

    def test_a_push_carrying_a_commit_red_mid_series_is_refused(self):
        red = self.commit(SOURCE, "red api:check\n", "feat: red")
        self.commit(SOURCE, "green\n", "fix: green again")
        push = self.push()
        self.assertNotEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertEqual(self.remote_head(), self.base)
        self.assertIn(("api:check", red), self.ran())

    def test_a_push_of_green_commits_is_published(self):
        green = self.commit(SOURCE, "green one\n", "feat: green")
        push = self.push()
        self.assertEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertEqual(self.remote_head(), green)
        self.assertEqual(self.ran(), [(gate, green) for gate in GATE_NAMES])


if __name__ == "__main__":
    unittest.main()
