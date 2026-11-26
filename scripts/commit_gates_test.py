"""Tests that every commit a series lands is held to the per-commit gates, over
real repositories rather than a reading of the configuration.

`OverARange` drives `mise run commits:gates` -- the task CI's job and the
`pre-push` hook both run -- at a throwaway repository whose own `mise.toml`
declares each gate as a stub that records the commit it ran at and fails on a
marker in the source. What each case asserts is which commits and gates ran,
and in which tree, so a gate that silently checked the tip, the checkout or
nothing fails here. `AfterAPushEvent` passes the range the way CI's job does,
in a clone, over a forced push and a new ref. `ThePrePushHook` installs the
hooks through `mise run hooks:install` and asks a real `git push` -- of the
current branch, of another one, and of a delete -- whether the gate is armed
over the refs pushed. `TheInstallCheck` asks `check-install` about a hook
missing, foreign, unexecutable and installed, through a linked worktree and
`core.hooksPath`, and under CI.

Run as `mise run commits:gates:test`, or directly.
"""

import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

import commit_gates
from commits_test import hermetic_environment

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

    def gates(self, *arguments, repository=None):
        """`mise run commits:gates -- arguments` from this repository's root, at
        the fixture, or at `repository`.

        `GIT_DIR` and `GIT_WORK_TREE` name the fixture the way git's own hook
        environment names a repository, which the script has to read once and
        then keep away from the gates it runs.
        """
        repository = repository or self.repository
        environment = {
            **self.env,
            "GIT_DIR": str(repository / ".git"),
            "GIT_WORK_TREE": str(repository),
        }
        return subprocess.run(
            ["mise", "run", "--quiet", "commits:gates", "--", *arguments],
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
        self.assertPassed(self.gates("range", self.base, "HEAD"))
        self.assertEqual(
            self.ran(),
            [(gate, first) for gate in GATE_NAMES] + [(gate, second) for gate in GATE_NAMES],
        )

    def test_a_commit_red_mid_series_fails_though_the_tip_is_green(self):
        red = self.commit(SOURCE, "red docs:check\n", "feat: red")
        self.commit(SOURCE, "green\n", "fix: green again")
        run = self.gates("range", self.base, "HEAD")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn(f"{red[:12]} feat: red fails `docs:check`", run.stderr)
        self.assertEqual(self.ran(), [("lint", red), ("docs:check", red)])

    def test_a_commit_changing_no_gate_input_runs_no_gate(self):
        self.commit("README.md", "words\n", "docs: words")
        self.assertPassed(self.gates("range", self.base, "HEAD"))
        self.assertEqual(self.ran(), [])

    def test_a_merge_commit_is_not_checked(self):
        self.git("switch", "-q", "-c", "topic")
        topic = self.commit(SOURCE, "topic\n", "feat: topic")
        self.git("switch", "-q", "master")
        self.commit("README.md", "trunk\n", "docs: trunk")
        # Against its first parent the merge changes `SOURCE`, so only the
        # merge rule keeps it out.
        self.git("merge", "-q", "--no-ff", "--no-edit", "topic")
        self.assertPassed(self.gates("range", self.base, "HEAD"))
        self.assertEqual({commit for _, commit in self.ran()}, {topic})

    def test_what_the_checkout_holds_uncommitted_reaches_no_gate(self):
        green = self.commit(SOURCE, "green\nmore\n", "feat: green")
        self.write(SOURCE, "red lint\n")
        self.write("crates/untracked.rs", "red lint\n")
        self.assertPassed(self.gates("range", self.base, "HEAD"))
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
        self.assertNotEqual(self.gates("range", self.base, "HEAD").returncode, 0)
        listed = self.git("worktree", "list", "--porcelain").stdout
        self.assertEqual(listed.count("worktree "), 1, listed)

    def test_an_empty_range_is_refused_rather_than_defaulted(self):
        # What a CI expression gone wrong passes; the default there names no
        # commit, so reading it as the default would pass having checked none.
        self.commit(SOURCE, "red lint\n", "feat: red")
        run = self.gates("range", "", "")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn("usage:", run.stderr)
        self.assertEqual(self.ran(), [])

    def test_a_mistyped_old_revision_is_refused_rather_than_narrowed(self):
        self.commit(SOURCE, "red lint\n", "feat: red")
        run = self.gates("range", "no-such-branch", "HEAD")
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn("`no-such-branch` names no commit here", run.stderr)
        self.assertEqual(self.ran(), [])


class AfterAPushEvent(RangeTestCase):
    """`range OLD NEW` as CI's job passes a master push's `before` and `after`,
    in a clone of the remote, which is what the job's checkout is."""

    def clone(self):
        checkout = self.directory / "checkout"
        # `--no-local`: a local clone copies every object, a replaced one too.
        self.git("clone", "-q", "--no-local", str(self.remote), str(checkout), cwd=self.directory)
        return checkout

    def test_a_forced_push_checks_what_it_put_over_the_replaced_commit(self):
        replaced = self.commit(SOURCE, "replaced\n", "feat: replaced")
        self.git("push", "-q", "--no-verify", "origin", "master")
        self.git("reset", "-q", "--hard", self.base)
        first = self.commit(SOURCE, "first\n", "feat: first")
        second = self.commit(SOURCE, "second\n", "feat: second")
        self.git("push", "-q", "--no-verify", "--force", "origin", "master")
        # As a host serves a commit no ref reaches any longer.
        self.git("config", "uploadpack.allowAnySHA1InWant", "true", cwd=self.remote)
        checkout = self.clone()
        run = self.gates("range", replaced, second, repository=checkout)
        self.assertPassed(run)
        self.assertEqual(
            self.ran(),
            [(gate, first) for gate in GATE_NAMES] + [(gate, second) for gate in GATE_NAMES],
        )

    def test_an_old_commit_nobody_serves_leaves_only_the_tip_checked(self):
        self.commit(SOURCE, "first\n", "feat: first")
        second = self.commit(SOURCE, "second\n", "feat: second")
        self.git("push", "-q", "--no-verify", "origin", "master")
        run = self.gates("range", "1" * 40, second, repository=self.clone())
        self.assertPassed(run)
        self.assertIn("only", run.stdout)
        self.assertEqual(self.ran(), [(gate, second) for gate in GATE_NAMES])

    def test_a_new_ref_leaves_only_the_tip_checked(self):
        self.commit(SOURCE, "first\n", "feat: first")
        second = self.commit(SOURCE, "second\n", "feat: second")
        self.git("push", "-q", "--no-verify", "origin", "master")
        run = self.gates("range", "0" * 40, second, repository=self.clone())
        self.assertPassed(run)
        self.assertEqual(self.ran(), [(gate, second) for gate in GATE_NAMES])


class ThePrePushHook(RangeTestCase):
    """A real `git push`, refused or published by the hook `hooks:install` writes.

    The fixture carries this repository's `hk.pkl` verbatim and the
    `commits:gates` and `hooks:install` tasks as `mise.toml` declares them, and
    installs its hooks by running the latter, so a hook left uninstalled,
    pointed at another task, or reading `HEAD` rather than the refs git names
    turns a case below the wrong colour.
    """

    def populate(self):
        declared = tomllib.loads((ROOT / "mise.toml").read_text())
        tasks = declared["tasks"]
        stanzas = [f"[tools]\nhk = {json.dumps(declared['tools']['hk'])}\n", stub_tasks()]
        for name in ("commits:gates", "hooks:install", "hooks:check"):
            stanzas.append(
                f'[tasks."{name}"]\n'
                f"run = {json.dumps(tasks[name]['run'])}\n"
                f"tools = {{ python = {json.dumps(tasks[name]['tools']['python'])} }}\n"
            )
        self.write("mise.toml", "".join(stanzas))
        shutil.copy(ROOT / "hk.pkl", self.repository / "hk.pkl")
        self.write("scripts/commit_gates.py", (ROOT / "scripts/commit_gates.py").read_text())
        # Discovery stops here, so no `mise.toml` above the fixture merges in.
        self.env["MISE_CEILING_PATHS"] = str(self.directory)
        subprocess.run(
            ["mise", "run", "--quiet", "hooks:install"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            check=True,
        )

    def push(self, *refspecs):
        return subprocess.run(
            ["git", "push", "origin", *(refspecs or ["master"])],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )

    def remote_head(self, branch="master"):
        ref = f"refs/heads/{branch}"
        listed = self.git("for-each-ref", "--format=%(objectname)", ref, cwd=self.remote)
        return listed.stdout.strip() or None

    def topic(self, text, subject):
        """A `topic` branch off the base holding one commit, then back on master."""
        self.git("switch", "-q", "-c", "topic", self.base)
        sha = self.commit(SOURCE, text, subject)
        self.git("switch", "-q", "master")
        return sha

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

    def test_a_branch_pushed_from_elsewhere_is_checked_rather_than_head(self):
        red = self.topic("red lint\n", "feat: red topic")
        self.commit(SOURCE, "green\nhead\n", "feat: green head")
        push = self.push("topic")
        self.assertNotEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertIsNone(self.remote_head("topic"))
        self.assertEqual(self.ran(), [("lint", red)])

    def test_red_work_on_head_does_not_hold_back_another_branch(self):
        green = self.topic("green topic\n", "feat: green topic")
        self.commit(SOURCE, "red lint\n", "wip: red head")
        push = self.push("topic")
        self.assertEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertEqual(self.remote_head("topic"), green)
        self.assertEqual(self.ran(), [(gate, green) for gate in GATE_NAMES])

    def test_a_delete_publishes_nothing_to_check(self):
        self.git("push", "-q", "--no-verify", "origin", "master:topic")
        self.commit(SOURCE, "red lint\n", "wip: red head")
        push = self.push(":topic")
        self.assertEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertIsNone(self.remote_head("topic"))
        self.assertEqual(self.ran(), [])

    def test_install_refuses_to_replace_a_hook_it_did_not_write(self):
        hook = self.repository / ".git" / "hooks" / "pre-push"
        hook.write_text("#!/bin/sh\nexit 0\n")
        run = subprocess.run(
            ["mise", "run", "--quiet", "hooks:install"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(run.returncode, 0, run.stdout)
        self.assertIn("is not this repository's hook", run.stderr)
        self.assertEqual(hook.read_text(), "#!/bin/sh\nexit 0\n")

    def test_the_check_is_silent_over_the_hook_install_wrote(self):
        environment = {key: value for key, value in self.env.items() if key != "CI"}
        run = subprocess.run(
            ["mise", "run", "--quiet", "hooks:check"],
            cwd=self.repository,
            env=environment,
            capture_output=True,
            text=True,
        )
        self.assertEqual((run.returncode, run.stdout, run.stderr), (0, "", ""))


class TheInstallCheck(unittest.TestCase):
    """`check-install` in a throwaway repository: what it warns about, always
    exiting 0, and where it says nothing."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        # Resolved, as git prints the paths the warnings name.
        self.directory = Path(directory.name).resolve()
        self.repository = self.directory / "repository"
        self.repository.mkdir()
        self.env = {key: value for key, value in hermetic_environment().items() if key != "CI"}
        self.git("init", "-q", ".")

    def git(self, *arguments, cwd=None):
        return subprocess.run(
            ["git", *arguments],
            cwd=cwd or self.repository,
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )

    def hook(self):
        return self.repository / ".git" / "hooks" / "pre-push"

    def check(self, cwd=None, **extra):
        """`commit_gates.py check-install` from `cwd`; its stderr, once it exited 0."""
        run = subprocess.run(
            [sys.executable, str(ROOT / "scripts" / "commit_gates.py"), "check-install"],
            cwd=cwd or self.repository,
            env={**self.env, **extra},
            capture_output=True,
            text=True,
        )
        self.assertEqual((run.returncode, run.stdout), (0, ""), run.stderr)
        return run.stderr

    def install(self, cwd=None):
        subprocess.run(
            [sys.executable, str(ROOT / "scripts" / "commit_gates.py"), "install"],
            cwd=cwd or self.repository,
            env=self.env,
            capture_output=True,
            check=True,
        )

    def test_a_missing_hook_names_the_task_that_installs_it(self):
        warning = self.check()
        self.assertIn("no pre-push hook", warning)
        self.assertIn("`mise run hooks:install`", warning)

    def test_a_foreign_hook_is_told_apart_from_a_missing_one(self):
        self.hook().parent.mkdir(parents=True, exist_ok=True)
        self.hook().write_text("#!/bin/sh\nexit 0\n")
        self.hook().chmod(0o755)
        warning = self.check()
        self.assertIn("is not this repository's pre-push hook", warning)
        self.assertIn("move it aside", warning)

    def test_the_hook_install_wrote_is_silent(self):
        self.install()
        self.assertEqual(self.check(), "")

    def test_a_marked_hook_git_cannot_execute_warns(self):
        self.install()
        self.hook().chmod(0o644)
        self.assertIn("is not executable", self.check())

    def test_ci_is_silent_over_a_missing_hook(self):
        self.assertEqual(self.check(CI="true"), "")

    def test_ci_set_to_zero_or_any_case_of_false_still_warns(self):
        for value in ("0", "false", "FALSE"):
            with self.subTest(CI=value):
                self.assertIn("no pre-push hook", self.check(CI=value))

    def test_outside_a_repository_is_silent(self):
        outside = self.directory / "outside"
        outside.mkdir()
        self.assertEqual(self.check(cwd=outside, GIT_CEILING_DIRECTORIES=str(self.directory)), "")

    def test_a_linked_worktree_reads_the_hook_its_common_directory_holds(self):
        self.git("commit", "-q", "--allow-empty", "-m", "chore: base")
        linked = self.directory / "linked"
        self.git("worktree", "add", "-q", "--detach", str(linked))
        self.assertIn(str(self.hook()), self.check(cwd=linked))
        self.install(cwd=linked)
        self.assertEqual(self.check(cwd=linked), "")
        self.assertEqual(self.check(), "")

    def test_core_hooks_path_is_where_the_hook_is_read(self):
        self.install()
        self.git("config", "core.hooksPath", "elsewhere")
        warning = self.check()
        self.assertIn(str(self.repository / "elsewhere" / "pre-push"), warning)
        self.install()
        self.assertEqual(self.check(), "")


if __name__ == "__main__":
    unittest.main()
