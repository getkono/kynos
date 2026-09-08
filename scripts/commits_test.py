"""Tests for the rule the commit-message gate states, over the one input that
makes the two halves of that rule disagree: a merge.

This repository states "commits follow Conventional Commits, merge commits
exempt" twice, through two instruments that cannot see the same thing.
`commits:check` hands convco a range, and convco drops a merge from the walk by
parent count -- `revwalk.filter(|c| c.parent_count() <= 1)`. `commits:message`
hands convco one message on stdin, and a bare message carries no parent count,
so the exemption cannot exist on that path unless the task supplies it. Nor can
convco be told to: the `--from-stdin` branch of `convco check` returns before
`--ignore-message-pattern` is ever consulted.

What is under test here is therefore wiring, not a parser: whether the hook-side
half of the rule agrees with the range-side half about the same commit. That is
a regression of the silent kind -- both halves keep exiting zero over every
commit anyone writes by hand, and the disagreement surfaces only the next time
somebody merges, at which point the fix that looks available is `--no-verify`.

Nothing below forges a state file. Each case builds a real repository in a
temporary directory and puts it in a real merge with `git merge --no-commit
--no-ff`, which is the state git itself is in when it runs the commit-msg hook:
`write_merge_heads` precedes `run_commit_hook(..., "commit-msg", ...)` in
`builtin/merge.c::prepare_to_commit`, and MERGE_HEAD's lines are the extra
parents of the commit about to be written.

The gate is invoked as the artefact it actually is, `mise run --quiet
commits:message`, from the project root so mise resolves `mise.toml`, with
`GIT_DIR` and `GIT_WORK_TREE` redirected at the throwaway repository.
Redirecting git by environment rather than by working directory is what lets
those two facts hold at once.

The squash case is here to keep the exemption from widening into a check on the
shape of the message. `git merge --squash` writes no MERGE_HEAD, runs no
commit-msg hook of its own, and produces a one-parent commit that
`commits:check` and CI do check -- so the message its later `git commit` carries
must stay checked here too.

Run it as `mise run commits:test`, or directly. There is no Python test runner
in this repository and `unittest` needs none.
"""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# A `.pyc` written beside the scripts would be an untracked directory in every
# working tree that ran these tests, and `.gitignore` has no entry for one. The
# task passes `-B` for the same reason; this covers a direct `python3` run.
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parent.parent

# Hermetic by construction: no user or system git configuration reaches these
# repositories, and identity is supplied rather than discovered, so a machine
# with no `user.email` set runs them the same as one that has.
#
# The inherited `GIT_*` variables go too, and that one is not defensive
# housekeeping. `commits:test` runs from `hooks:pre-push`, which git invokes
# with a git environment already exported -- `GIT_INDEX_FILE`, `GIT_PREFIX` and
# `GIT_EXEC_PATH` are all present in a hook today, and `GIT_DIR`,
# `GIT_WORK_TREE`, `GIT_OBJECT_DIRECTORY` and `GIT_COMMON_DIR` are exported by
# other git entry points and by anyone who exports them by hand. Any one of
# them redirects the fixture's own `git init` and `git commit` at whatever
# repository it names, which for that hook is the repository being pushed. A
# test that writes commits into the tree that invoked it is the failure this
# scrub exists to make impossible, rather than to make unlikely.
HERMETIC = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
    "GIT_AUTHOR_NAME": "Commit Gate Tests",
    "GIT_AUTHOR_EMAIL": "commit-gate-tests@invalid",
    "GIT_COMMITTER_NAME": "Commit Gate Tests",
    "GIT_COMMITTER_EMAIL": "commit-gate-tests@invalid",
}

MERGE_SUBJECT = "Merge remote-tracking branch 'origin/master' into topic\n"
CONVENTIONAL_SUBJECT = "fix(hooks): complete the merge by hand\n"
SQUASH_SUBJECT = "Squashed commit of the following:\n"


def scrubbed_environment():
    """This process's environment with every inherited `GIT_*` name removed."""
    return {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


class GateTestCase(unittest.TestCase):
    """A throwaway repository per test, and the gate run against its state."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repository = Path(directory.name)

        self.env = {**scrubbed_environment(), **HERMETIC}
        self.git("-c", "init.defaultBranch=main", "init", "-q", ".")

    def git(self, *arguments):
        """Run git inside the throwaway repository, failing loudly."""
        return subprocess.run(
            ["git", *arguments],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )

    def gate(self, message):
        """Run `commits:message` from the project root over the throwaway repository.

        The task is the unit under test, so it is run the way `hk.pkl` runs it,
        with the message on stdin. `GIT_DIR` and `GIT_WORK_TREE` are what make
        the task read the throwaway repository's state while mise still resolves
        the project's own `mise.toml`.
        """
        return subprocess.run(
            ["mise", "run", "--quiet", "commits:message"],
            cwd=ROOT,
            env={
                **self.env,
                "GIT_DIR": str(self.repository / ".git"),
                "GIT_WORK_TREE": str(self.repository),
            },
            input=message,
            capture_output=True,
            text=True,
        )

    def diverging_branches(self):
        """Two branches with a commit each after their common ancestor.

        Empty commits, because what the merge has to be is non-fast-forward,
        not conflicted: the state the hook sees is the same either way, and a
        conflict would need file contents that say nothing about the rule.
        """
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the common ancestor")
        self.git("switch", "-q", "-c", "topic")
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the topic side")
        self.git("switch", "-q", "main")
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the trunk side")

    def merge_head(self):
        """Whether a merge is in progress, asked the way the task asks it.

        `git rev-parse` rather than a path test, because MERGE_HEAD does not
        always live at `.git/MERGE_HEAD` -- in a linked worktree it is under
        `.git/worktrees/<name>/`, which is where this repository's own merges
        happen. A fixture that agreed with the task only for the plain
        repository it builds would be asserting less than it appears to.
        """
        probed = subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", "MERGE_HEAD"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )
        return probed.returncode == 0

    def assertAccepted(self, result):
        self.assertEqual(
            result.returncode,
            0,
            f"the gate rejected the message\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )

    def assertRejected(self, result):
        self.assertNotEqual(
            result.returncode,
            0,
            f"the gate accepted the message\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )


class MergeInProgress(GateTestCase):
    """The state git is in when it runs commit-msg for a merge commit."""

    def setUp(self):
        super().setUp()
        self.diverging_branches()
        self.git("merge", "--no-commit", "--no-ff", "topic")
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")

    def test_a_merge_subject_passes_while_a_merge_is_in_progress(self):
        """The reported bug. `commits:check` exempts this commit; so must the hook."""
        self.assertAccepted(self.gate(MERGE_SUBJECT))

    def test_a_conventional_subject_passes_while_a_merge_is_in_progress(self):
        """A message written by hand during a merge is still a valid message.

        The exemption skips the check; it does not have to reject what the
        check would have accepted.
        """
        self.assertAccepted(self.gate(CONVENTIONAL_SUBJECT))


class NoMergeInProgress(GateTestCase):
    """Every other commit, which is the case the gate exists for."""

    def setUp(self):
        super().setUp()
        self.diverging_branches()
        self.assertFalse(self.merge_head(), "the fixture left a merge in progress")

    def test_a_merge_subject_fails_with_no_merge_in_progress(self):
        """The guard on the fix: the exemption is keyed on the state, not the words.

        A commit whose author merely wrote `Merge ...` is a one-parent commit
        that `convco check` walks in a range and rejects, so rejecting it here
        is what keeps the two halves agreeing.
        """
        self.assertRejected(self.gate(MERGE_SUBJECT))

    def test_a_conventional_subject_passes_with_no_merge_in_progress(self):
        """The baseline: the gate still does its job."""
        self.assertAccepted(self.gate(CONVENTIONAL_SUBJECT))

    def test_a_squash_merge_leaves_no_merge_head_and_stays_checked(self):
        """`git merge --squash` is not a merge as far as this rule is concerned.

        It writes SQUASH_MSG and no MERGE_HEAD, runs no commit-msg hook of its
        own, and the commit it leads to has one parent -- which `commits:check`
        and the `commits` CI job check. So its default message has to be
        rejected here, or the hook would pass what CI fails.
        """
        self.git("merge", "--squash", "topic")
        self.assertFalse(self.merge_head(), "a squash merge wrote MERGE_HEAD")
        squash_message = self.git("rev-parse", "--git-path", "SQUASH_MSG").stdout.strip()
        self.assertTrue(
            (self.repository / squash_message).exists(),
            "the fixture did not reach a squashed state",
        )
        self.assertRejected(self.gate(SQUASH_SUBJECT))


class AmbientGitEnvironment(GateTestCase):
    """The fixtures must not follow a git environment the caller exported.

    This is the case that makes the scrub above a rule rather than a habit.
    `commits:test` runs from `hooks:pre-push`, which git invokes with a git
    environment already in place, and the repository that environment names is
    the one being pushed. A fixture that inherited it would `git init` and
    `git commit` into somebody's working tree while they pushed it.
    """

    def setUp(self):
        decoy = tempfile.TemporaryDirectory()
        self.addCleanup(decoy.cleanup)
        self.decoy = Path(decoy.name)
        subprocess.run(
            ["git", "-c", "init.defaultBranch=main", "init", "-q", "."],
            cwd=self.decoy,
            env={**scrubbed_environment(), **HERMETIC},
            capture_output=True,
            check=True,
        )

        exported = mock.patch.dict(
            os.environ,
            {"GIT_DIR": str(self.decoy / ".git"), "GIT_WORK_TREE": str(self.decoy)},
        )
        exported.start()
        self.addCleanup(exported.stop)

        super().setUp()
        self.diverging_branches()
        self.git("merge", "--no-commit", "--no-ff", "topic")

    def decoy_has_commits(self):
        probed = subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", "HEAD"],
            cwd=self.decoy,
            env={**scrubbed_environment(), **HERMETIC},
            capture_output=True,
            text=True,
        )
        return probed.returncode == 0

    def test_an_exported_git_dir_reaches_neither_the_fixture_nor_the_gate(self):
        self.assertFalse(
            self.decoy_has_commits(),
            "the fixture wrote into the repository the ambient GIT_DIR names",
        )
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")
        self.assertAccepted(self.gate(MERGE_SUBJECT))


if __name__ == "__main__":
    unittest.main()
