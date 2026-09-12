"""Tests that this repository's one rule about commit messages -- Conventional
Commits, merge commits exempt -- holds the same way in both instruments that
state it, over the input that made them disagree: a merge.

`commits:check` hands convco a range, where a merge is dropped by parent count.
`commits:message` hands convco one message, where there is no parent count, so
the task has to supply the exemption itself. `BothHalvesOverOneMerge` puts each
verdict to both halves over the same commit; `TheAmendResidual` pins the one
state where they are meant to disagree.

Nothing here forges a state file or stands in for git: the fixtures build real
repositories and reach a real merge. `TheGateHkRuns` installs the pinned
`hk run commit-msg` as a throwaway repository's own `commit-msg` hook, so
whether the step is armed is answered by an exit code rather than by a reading
of `hk.pkl`. One fixture is a linked worktree, where MERGE_HEAD lives under
`.git/worktrees/<name>/` and the guard still has to find it.

Run as `mise run commits:test`, or directly.
"""

import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# A run writes no `.pyc` into the tree it reads, and `.gitignore` covers the
# imports no task controls. The task passes `-B` for the same reason; this
# covers a direct `python3` run.
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parent.parent

# No user or system git configuration reaches these repositories, and identity
# is supplied rather than discovered. The inherited `GIT_*` names go too:
# `commits:test` runs from `hooks:pre-push`, where git has already exported an
# environment naming the repository being pushed, and any one of those names
# would redirect the fixture's own `git init` at it. `AmbientGitEnvironment`
# holds the scrub.
HERMETIC = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
    "GIT_AUTHOR_NAME": "Commit Gate Tests",
    "GIT_AUTHOR_EMAIL": "commit-gate-tests@invalid",
    "GIT_COMMITTER_NAME": "Commit Gate Tests",
    "GIT_COMMITTER_EMAIL": "commit-gate-tests@invalid",
    # Without this, a `git merge` that thought it was interactive would open an
    # editor and hang rather than fail. Not left to the pipes not being a tty.
    "GIT_MERGE_AUTOEDIT": "no",
}

MERGE_SUBJECT = "Merge remote-tracking branch 'origin/master' into topic\n"
CONVENTIONAL_SUBJECT = "fix(hooks): complete the merge by hand\n"
SQUASH_SUBJECT = "Squashed commit of the following:\n"

# convco's own words when a subject is not a Conventional Commit. Asserted on
# rather than a bare non-zero exit, which is also what an absent convco (127) or
# a broken task file produces -- each of which would pass a reject-case for a
# reason unrelated to the rule.
REJECTION = "first line doesn't match"


def scrubbed_environment():
    """This process's environment with every inherited `GIT_*` name removed."""
    return {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


def hermetic_environment():
    """The environment every fixture, every tool lookup and the gate itself run in.

    `HERMETIC` over the `GIT_*` scrub, then every inherited `HK_*` and
    `CONVCO_*` name removed too: `HK_SKIP_STEPS`, `HK_SKIP_HOOK` and
    `CONVCO_MERGES` are each an escape hatch committed nowhere, and these cases
    are about what this repository declares rather than what a shell overrides.
    Whole namespaces rather than a list of names, which would go stale.
    `range_gate` sets the one name the suite needs, `CONVCO_RANGE`, back after.
    """
    return {
        key: value
        for key, value in {**scrubbed_environment(), **HERMETIC}.items()
        if not (key.startswith("HK_") or key.startswith("CONVCO_"))
    }


# The one place this suite reads a declaration out of another file. It is safe
# for a reason that does not generalise, so it is written down: both patterns
# anchor to the start of a line where they require `[` or `r`, so a commented-out
# `#[tasks."commits:message"]` or `# run = '''` cannot match, and the body they
# capture is shell in which a `#` line means the same to mise and to the fixture.
def declared_merge_guard():
    """The shell body `mise.toml` declares for `[tasks."commits:message"]`.

    Read rather than restated, because `LinkedWorktree` runs this body where
    `mise run` cannot put it -- cwd inside the repository under test, with no
    `GIT_DIR` exported -- and a restatement there would hold nothing.
    """
    text = (ROOT / "mise.toml").read_text()
    task = re.search(r'^\[tasks\."commits:message"\]\n(.*?)^\[', text, re.DOTALL | re.MULTILINE)
    if task is None:
        raise AssertionError("mise.toml declares no `commits:message` task")
    body = re.search(r"^run = '''\n(.*?)^'''", task.group(1), re.DOTALL | re.MULTILINE)
    if body is None:
        raise AssertionError("`commits:message` declares no multi-line `run` body")
    return body.group(1)


def convco_on_path():
    """`PATH` with mise's convco on it, for the one call that bypasses mise.

    `shell_gate` runs the task's body directly, so nothing has put the pinned
    convco anywhere; resolving it here keeps that call on the same binary every
    other case reaches through `mise run`.
    """
    located = subprocess.run(
        ["mise", "which", "convco"],
        cwd=ROOT,
        env=hermetic_environment(),
        capture_output=True,
        text=True,
        check=True,
    )
    return str(Path(located.stdout.strip()).parent) + os.pathsep + os.environ.get("PATH", "")


def hk_binary():
    """The pinned hk, resolved the way `convco_on_path` resolves convco.

    Asked of mise rather than of `PATH`, so the gate these cases drive is the
    `hk` version `[tools]` pins.
    """
    located = subprocess.run(
        ["mise", "which", "hk"],
        cwd=ROOT,
        env=hermetic_environment(),
        capture_output=True,
        text=True,
        check=True,
    )
    return located.stdout.strip()


class GateTestCase(unittest.TestCase):
    """A throwaway repository per test, and both halves of the rule over it."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repository = Path(directory.name)

        self.env = hermetic_environment()
        self.git("-c", "init.defaultBranch=main", "init", "-q", ".")

    # -- the repositories -------------------------------------------------

    def git(self, *arguments, cwd=None):
        """Run git inside the throwaway repository, failing loudly."""
        return subprocess.run(
            ["git", *arguments],
            cwd=cwd or self.repository,
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )

    def diverging_branches(self):
        """Two branches with a commit each after their common ancestor.

        Returns the ancestor, which is the base every range assertion is taken
        from. Empty commits: the merge has to be non-fast-forward, not
        conflicted, and the state the hook sees is the same either way.
        """
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the common ancestor")
        base = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("switch", "-q", "-c", "topic")
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the topic side")
        self.git("switch", "-q", "main")
        self.git("commit", "-q", "--allow-empty", "-m", "feat: the trunk side")
        return base

    def merge_head(self, cwd=None):
        """Whether a merge is in progress, asked the way the task asks it.

        `--git-path` rather than `.git/MERGE_HEAD`, because in a linked
        worktree the file is under `.git/worktrees/<name>/`. And a path test
        rather than `rev-parse --verify MERGE_HEAD`, which resolves a *ref*.
        """
        located = self.git("rev-parse", "--git-path", "MERGE_HEAD", cwd=cwd)
        return (Path(cwd or self.repository) / located.stdout.strip()).is_file()

    # -- the two halves of the rule ---------------------------------------

    def gate(self, message, git_dir=None, work_tree=None):
        """The hook half: `commits:message` over one message.

        Run from the project root, so mise resolves `mise.toml`, with `GIT_DIR`
        and `GIT_WORK_TREE` redirected at the throwaway repository.
        """
        return subprocess.run(
            ["mise", "run", "--quiet", "commits:message"],
            cwd=ROOT,
            env={
                **self.env,
                "GIT_DIR": str(git_dir or self.repository / ".git"),
                "GIT_WORK_TREE": str(work_tree or self.repository),
            },
            input=message,
            capture_output=True,
            text=True,
        )

    def shell_gate(self, message, cwd):
        """The hook half again, but on git's *discovery* path.

        Every other call exports `GIT_DIR`, so the guard is never asked to find
        the repository itself -- which is the whole difference between
        `--git-path` and the spellings it was chosen over. `mise run` executes a
        task in its config's directory, so the task itself cannot be put here;
        what runs is the `run` body read out of `mise.toml`, with no `GIT_DIR`
        and `cwd` where git actually invokes a `commit-msg` hook from.
        """
        return subprocess.run(
            ["sh", "-c", declared_merge_guard()],
            cwd=cwd,
            env={**self.env, "PATH": convco_on_path()},
            input=message,
            capture_output=True,
            text=True,
        )

    def range_gate(self, base, git_dir=None, work_tree=None):
        """The range half: `commits:check` over `base..HEAD`.

        The same task the `commits` CI job and `hooks:pre-push` run, through
        the `CONVCO_RANGE` branch it already carries.
        """
        return subprocess.run(
            ["mise", "run", "--quiet", "commits:check"],
            cwd=ROOT,
            env={
                **self.env,
                "GIT_DIR": str(git_dir or self.repository / ".git"),
                "GIT_WORK_TREE": str(work_tree or self.repository),
                "CONVCO_RANGE": f"{base}..HEAD",
            },
            capture_output=True,
            text=True,
        )

    # -- verdicts ---------------------------------------------------------

    def assertAccepted(self, result):
        self.assertEqual(
            result.returncode,
            0,
            f"the gate rejected the message\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )

    def assertRejected(self, result):
        """Rejected *by the rule*, not merely exited non-zero: a missing convco
        exits 127 and a broken task file exits 1."""
        self.assertNotEqual(
            result.returncode,
            0,
            f"the gate accepted the message\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )
        self.assertIn(
            REJECTION,
            result.stdout,
            "the gate exited non-zero without convco reporting on the subject; "
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )


class BothHalvesOverOneMerge(GateTestCase):
    """The rule, asserted on both instruments over the same commits.

    Every other class tests the hook half against some state; this one tests
    that the two halves reach the same verdict, which is the property the change
    exists to establish and the only one that catches a range-side regression.
    """

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()

    def test_both_halves_accept_the_same_merge(self):
        # The hook half, at the moment git would ask it.
        self.git("merge", "--no-commit", "--no-ff", "topic")
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")
        self.assertAccepted(self.gate(MERGE_SUBJECT))

        # The same commit, now written, put to the half that walks a range.
        self.git("commit", "-q", "--no-verify", "-m", MERGE_SUBJECT.strip())
        parents = self.git("rev-list", "--parents", "-n", "1", "HEAD").stdout.split()
        self.assertEqual(len(parents), 3, "the fixture did not write a two-parent commit")
        self.assertAccepted(self.range_gate(self.base))

    def test_an_exported_convco_override_does_not_reach_either_half(self):
        """`CONVCO_MERGES` in somebody's shell is not this repository's answer.

        It is the switch behind `no_merge_commits`: exported, it turns
        `convco check BASE..HEAD` from exit 0 to exit 1 over a merge subject.
        `hermetic_environment` strips the namespace, and this is the case that
        holds the strip on the range half.
        """
        exported = mock.patch.dict(os.environ, {"CONVCO_MERGES": "true"})
        exported.start()
        self.addCleanup(exported.stop)
        self.env = hermetic_environment()

        self.git("merge", "--no-commit", "--no-ff", "topic")
        self.assertAccepted(self.gate(MERGE_SUBJECT))
        self.git("commit", "-q", "--no-verify", "-m", MERGE_SUBJECT.strip())
        self.assertAccepted(self.range_gate(self.base))

    def test_both_halves_reject_the_same_one_parent_merge_subject(self):
        """A subject spelled `Merge ...` on a one-parent commit is not exempt.

        The exemption is keyed on the state, not the words. Asserted on both
        halves, since a fix widening one would be invisible in the other.
        """
        self.assertRejected(self.gate(MERGE_SUBJECT))
        self.git("commit", "-q", "--allow-empty", "--no-verify", "-m", MERGE_SUBJECT.strip())
        self.assertRejected(self.range_gate(self.base))

    def test_both_halves_accept_a_conventional_commit(self):
        """The baseline, on both halves, over the same commit.

        The subject put to the hook half is then written and walked by the range
        half, rather than the range half walking whatever the fixture left at
        HEAD -- which is what "over the same commit" has to mean here.
        """
        self.assertAccepted(self.gate(CONVENTIONAL_SUBJECT))
        self.git("commit", "-q", "--allow-empty", "--no-verify", "-m", CONVENTIONAL_SUBJECT.strip())
        self.assertAccepted(self.range_gate(self.base))


class TheGateHkRuns(GateTestCase):
    """The gate hk actually runs, asked what it did rather than what it declares.

    The hook installed below is `hk run commit-msg`, so hk reads this
    repository's own `hk.pkl`, resolves it after Pkl and hk have merged every
    source, decides for itself whether to run the step, and answers with an exit
    code `git commit` and `git merge` obey. Three readings of that exit code:

        no merge, a non-conforming subject   -> the commit is refused
        no merge, a Conventional subject     -> the commit is written
        a merge in progress, its own subject -> the merge commit is written

    That is one assertion about an outcome in place of an enumeration of causes,
    and it is why nothing here reads `hk.pkl`. Every way of turning the step off
    -- deleting it, moving it under another hook, dropping its redirect, a
    duplicate entry, `skip_steps`, `skip_hooks`, an `hk.local.pkl` -- turns the
    first reading green. A rename is not one of them, and this class is right to
    stay green through one: hk runs the step's `check` under any name. The other
    two readings stop the first from being satisfied by a gate refusing
    everything. The third is issue #132 itself: before the fix it prints
    `Not committing merge`.

    Nothing is installed into this clone: the hook is the *fixture's* own, and
    `GIT_DIR` is the fixture's, so the merge state the step reads is never the
    state of the repository the suite runs in.
    """

    def setUp(self):
        super().setUp()
        self.changes = 0
        self.diverging_branches_carrying_files()

        hook = self.repository / ".git" / "hooks" / "commit-msg"
        hook.parent.mkdir(parents=True, exist_ok=True)
        # The prologue is what makes this fixture hermetic. hk resolves
        # `hk.pkl` from where it runs, so it has to run at the project root --
        # and git spells the environment it hands a hook relative to the work
        # tree it invoked the hook from, so every git path has to be absolute
        # before that `cd`. In the real repository none of this is needed.
        #
        # `GIT_DIR` is the one the exemption reads, so a developer with a merge
        # of their own in progress gets the same answers as one without.
        #
        # `GIT_WORK_TREE` is what makes hk's file set a real commit's: unpinned,
        # the fixture's index is compared against the project root's files and
        # the set comes out empty.
        # `test_the_step_is_selected_over_the_files_the_commit_stages` holds it.
        #
        # `MISE_CONFIG_FILE` is the price of pinning the work tree: hk runs the
        # step's command from the work tree, where mise reports `no tasks
        # defined`. Naming this repository's `mise.toml` puts the task back --
        # though in `$HOME` rather than at `ROOT`, since a config named by that
        # variable does not set the task's directory the way a discovered one
        # does. Nothing asserted here depends on that directory; convco's own
        # config discovery is the residual, and its `CONVCO_*` names are
        # stripped in `hermetic_environment`.
        #
        # `GIT_INDEX_FILE` git exports relative to the fixture, and past the
        # `cd` that names the project root's index instead -- whose objects are
        # in another repository, so hk dies with `failed to get staged
        # statuses`. It fails that way only in a plain clone: in a linked
        # worktree `.git/index` cannot be opened at all and libgit2 falls back
        # to no index. So it is CI that holds this line, not the tree it is
        # written in.
        hook.write_text(
            "#!/bin/sh\n"
            'message="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"\n'
            'GIT_DIR="$(git rev-parse --absolute-git-dir)"\n'
            "export GIT_DIR\n"
            'GIT_WORK_TREE="$(git rev-parse --show-toplevel)"\n'
            "export GIT_WORK_TREE\n"
            'if [ -n "${GIT_INDEX_FILE:-}" ]; then\n'
            '  GIT_INDEX_FILE="$(cd "$(dirname "$GIT_INDEX_FILE")" && pwd)/'
            '$(basename "$GIT_INDEX_FILE")"\n'
            "  export GIT_INDEX_FILE\n"
            "fi\n"
            'MISE_CONFIG_FILE="' + str(ROOT / "mise.toml") + '"\n'
            "export MISE_CONFIG_FILE\n"
            'cd "' + str(ROOT) + '" || exit 1\n'
            'exec "' + hk_binary() + '" run commit-msg "$message"\n'
        )
        hook.chmod(0o755)

    def stage_a_change(self):
        """One Rust source and one Markdown document, staged.

        These cases drive real `git commit` and `git merge`, which need
        something to commit. The two files are also hk's file set for this hook
        -- the commit's staged files, the message file not among them -- which
        is what a `glob`, `types` or `exclude` key on the step is matched
        against, and what makes a narrowing value show up as a disarm.
        """
        self.changes += 1
        source = self.repository / f"change-{self.changes}.rs"
        source.write_text(f"pub fn change_{self.changes}() {{}}\n")
        document = self.repository / f"change-{self.changes}.md"
        document.write_text(f"# change {self.changes}\n")
        self.git("add", source.name, document.name)

    def diverging_branches_carrying_files(self):
        """`diverging_branches`, with a file on every commit and no range.

        No ancestor, since no case here walks a range; no empty commits, since
        the merge has to carry a file across for the step to select.
        """
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the common ancestor")
        self.git("switch", "-q", "-c", "topic")
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the topic side")
        self.git("switch", "-q", "main")
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the trunk side")

    def attempt_commit(self, subject, environment=None):
        """`git commit` over whatever is staged, put to the gate hk runs."""
        return subprocess.run(
            ["git", "commit", "-m", subject],
            cwd=self.repository,
            env=environment or self.env,
            capture_output=True,
            text=True,
        )

    def commit(self, subject, environment=None):
        """One ordinary commit, carrying a change, put to the gate hk runs."""
        self.stage_a_change()
        return self.attempt_commit(subject, environment)

    def assert_refused(self, attempt, before):
        self.assertNotEqual(
            attempt.returncode,
            0,
            f"the gate hk runs let the subject past\n"
            f"stdout:\n{attempt.stdout}\nstderr:\n{attempt.stderr}",
        )
        self.assertIn(
            REJECTION,
            attempt.stdout + attempt.stderr,
            "the gate hk runs exited non-zero without convco reporting on the "
            f"subject\nstdout:\n{attempt.stdout}\nstderr:\n{attempt.stderr}",
        )
        self.assertEqual(before, self.git("rev-parse", "HEAD").stdout.strip())

    def test_a_real_merge_completes_through_the_gate(self):
        """`git merge --no-ff` writes its own merge commit. Issue #132, closed.

        The exemption end to end, every link real: git writes MERGE_HEAD, runs
        `commit-msg`, hk runs the step, the step reads the file git wrote. The
        ordering the fix rests on is observed here rather than cited.
        """
        merged = subprocess.run(
            ["git", "merge", "--no-ff", "topic"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )
        self.assertEqual(
            merged.returncode,
            0,
            f"git merge could not write its merge commit\n"
            f"stdout:\n{merged.stdout}\nstderr:\n{merged.stderr}",
        )
        parents = self.git("rev-list", "--parents", "-n", "1", "HEAD").stdout.split()
        self.assertEqual(len(parents), 3, "the merge did not produce a two-parent commit")
        self.assertFalse(self.merge_head(), "the merge left a merge in progress")

    def test_the_gate_refuses_a_merge_subject_with_no_merge_in_progress(self):
        """The reading every disarm turns green, and the one that catches them.

        A one-parent commit carrying a merge-shaped subject has to be refused by
        convco's own words, with `HEAD` where it was.
        """
        self.assertFalse(self.merge_head(), "the fixture left a merge in progress")
        before = self.git("rev-parse", "HEAD").stdout.strip()
        self.assert_refused(self.commit("Merge branch 'nothing'"), before)

    def test_the_gate_accepts_a_conventional_subject(self):
        """The reading that stops the one above from passing for a gate that
        refuses everything -- a `check` of `false` rather than of `true`."""
        before = self.git("rev-parse", "HEAD").stdout.strip()
        written = self.commit(CONVENTIONAL_SUBJECT.strip())
        self.assertEqual(
            written.returncode,
            0,
            f"the gate hk runs refused a Conventional Commit\n"
            f"stdout:\n{written.stdout}\nstderr:\n{written.stderr}",
        )
        self.assertNotEqual(before, self.git("rev-parse", "HEAD").stdout.strip())

    def test_the_step_is_selected_over_the_files_the_commit_stages(self):
        """hk's file set here is a real commit's, which is its staged files.

        The set is what a `glob`, `types` or `exclude` key would be matched
        against, so a fixture whose set came out empty would answer the same for
        a value selecting everything as for one selecting nothing. Read out of
        hk's `HK_LOG=debug` line rather than inferred, because the exit code the
        rest of this class reads cannot see it: hk runs the step over an empty
        set as readily as a full one.
        """
        before = self.git("rev-parse", "HEAD").stdout.strip()
        attempt = self.commit(
            "Merge branch 'nothing'", environment={**self.env, "HK_LOG": "debug"}
        )
        self.assert_refused(attempt, before)
        selected = re.search(r"^DEBUG files: \{(.*)\}$", attempt.stderr, re.MULTILINE)
        self.assertIsNotNone(
            selected,
            f"hk logged no file set to read\nstderr:\n{attempt.stderr}",
        )
        for name in (f"change-{self.changes}.rs", f"change-{self.changes}.md"):
            self.assertIn(
                name,
                selected.group(1),
                "hk selected none of the files this commit staged, so this "
                "fixture answers for a `glob`, `types` or `exclude` key at "
                f"every value\nhk selected: {{{selected.group(1)}}}",
            )

    def test_the_gate_refuses_over_a_commit_staging_one_unfamiliar_file(self):
        """The gate must not depend on what the commit in front of it stages.

        hk skips a step when no staged file matches its selection, so a value
        narrower than a commit disarms the gate for that commit and leaves it
        armed for the next. The reading above cannot see that: it stages a `.rs`
        and a `.md`. This commit stages one file nothing globs, so any narrowing
        value lets its subject through and fails here, while `glob = "*"` stays
        green because it arms the gate for every commit.
        """
        before = self.git("rev-parse", "HEAD").stdout.strip()
        unfamiliar = self.repository / "change-unfamiliar.gate-fixture"
        unfamiliar.write_text("one staged file that no ordinary glob names\n")
        self.git("add", unfamiliar.name)
        self.assert_refused(self.attempt_commit("Merge branch 'nothing'"), before)

    def test_an_exported_hk_skip_does_not_reach_the_gate(self):
        """`HK_SKIP_STEPS` in somebody's shell is not this repository's answer.

        Exported into `hk run commit-msg`, it and `HK_SKIP_HOOK` each make it
        exit 0 having run nothing. `hermetic_environment` strips the namespace,
        and this is the case that holds the strip.
        """
        exported = mock.patch.dict(os.environ, {"HK_SKIP_STEPS": "conventional-commit"})
        exported.start()
        self.addCleanup(exported.stop)
        before = self.git("rev-parse", "HEAD").stdout.strip()
        self.assert_refused(
            self.commit("Merge branch 'nothing'", environment=hermetic_environment()), before
        )


class LinkedWorktree(GateTestCase):
    """The guard has to be worktree-correct, because this repository is worktrees.

    MERGE_HEAD is under `.git/worktrees/<name>/` here, not `.git/`. A guard
    spelled against `.git/MERGE_HEAD` reports "no merge" throughout a real merge
    while every plain-`git init` fixture in this file stays green.
    """

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()
        self.git("worktree", "add", "-q", "--detach", str(self.repository / "linked"), "main")
        self.linked = self.repository / "linked"
        self.linked_git_dir = self.git(
            "rev-parse", "--absolute-git-dir", cwd=self.linked
        ).stdout.strip()

    def test_the_guard_finds_the_repository_itself(self):
        """The guard on git's discovery path, with no `GIT_DIR` supplied.

        Every case but this one and its control exports `GIT_DIR`, so the guard
        is never asked to *find* the repository. Without this case
        `[ -f "$GIT_DIR/MERGE_HEAD" ]` passes the suite, while in the position
        git actually runs a `commit-msg` hook from it exempts nothing.
        """
        self.git("merge", "--no-commit", "--no-ff", "topic", cwd=self.linked)
        self.assertTrue(self.merge_head(cwd=self.linked), "no merge state in the linked worktree")
        self.assertAccepted(self.shell_gate(MERGE_SUBJECT, cwd=self.linked))

    def test_the_discovered_guard_still_checks_with_no_merge(self):
        """The control: discovery must not become a blanket exemption."""
        self.assertRejected(self.shell_gate(MERGE_SUBJECT, cwd=self.linked))

    def test_a_merge_in_a_linked_worktree_is_exempt(self):
        self.git("merge", "--no-commit", "--no-ff", "topic", cwd=self.linked)
        self.assertTrue(self.merge_head(cwd=self.linked), "no merge state in the linked worktree")
        self.assertFalse(
            (self.linked / ".git" / "MERGE_HEAD").exists(),
            "the plain-repository spelling would have found this merge, so the fixture proves nothing",
        )
        self.assertAccepted(
            self.gate(MERGE_SUBJECT, git_dir=self.linked_git_dir, work_tree=self.linked)
        )

    def test_a_merge_subject_is_still_checked_in_a_linked_worktree(self):
        self.assertFalse(self.merge_head(cwd=self.linked))
        self.assertRejected(
            self.gate(MERGE_SUBJECT, git_dir=self.linked_git_dir, work_tree=self.linked)
        )


class RefNamedMergeHead(GateTestCase):
    """A ref called MERGE_HEAD is not a merge, and must not be read as one.

    `git rev-parse --verify --quiet MERGE_HEAD` -- the spelling this task
    carried first -- resolves a *ref*, so a branch, tag or bare
    `refs/MERGE_HEAD` makes it exit 0 with no merge anywhere. The gate would
    then exempt a one-parent commit that `commits:check` rejects: a silent,
    total loss of the gate, opposite to what it was written to fix.
    """

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()

    def assert_still_checked(self):
        self.assertFalse(self.merge_head(), "a ref was mistaken for a merge in progress")
        self.assertRejected(self.gate(MERGE_SUBJECT))

    def test_a_branch_named_merge_head_does_not_exempt(self):
        self.git("branch", "MERGE_HEAD")
        self.assert_still_checked()

    def test_a_tag_named_merge_head_does_not_exempt(self):
        self.git("tag", "MERGE_HEAD")
        self.assert_still_checked()

    def test_a_bare_ref_named_merge_head_does_not_exempt(self):
        self.git("update-ref", "refs/MERGE_HEAD", "HEAD")
        self.assert_still_checked()


class NoMergeInProgress(GateTestCase):
    """Every other commit, which is the case the gate exists for."""

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()
        self.assertFalse(self.merge_head(), "the fixture left a merge in progress")

    def test_a_squash_merge_leaves_no_merge_head_and_stays_checked(self):
        """`git merge --squash` is not a merge as far as this rule is concerned.

        It writes SQUASH_MSG and no MERGE_HEAD, and the commit it leads to has
        one parent -- which `commits:check` does check. So its default message
        has to be rejected here, or the hook would pass what CI fails.
        """
        self.git("merge", "--squash", "topic")
        self.assertFalse(self.merge_head(), "a squash merge wrote MERGE_HEAD")
        squashed = self.git("rev-parse", "--git-path", "SQUASH_MSG").stdout.strip()
        self.assertTrue(
            (self.repository / squashed).is_file(), "the fixture did not reach a squashed state"
        )
        self.assertRejected(self.gate(SQUASH_SUBJECT))
        # `--allow-empty`, because the squashed side is an empty commit: what
        # is under test is the message on a one-parent commit, not a diff.
        self.git("commit", "-q", "--no-verify", "--allow-empty", "-m", SQUASH_SUBJECT.strip())
        parents = self.git("rev-list", "--parents", "-n", "1", "HEAD").stdout.split()
        self.assertEqual(len(parents), 2, "a squash merge produced more than one parent")
        self.assertRejected(self.range_gate(self.base))


class TheAmendResidual(GateTestCase):
    """The one divergence this fix cannot reach, pinned so it cannot move unseen.

    Amending or rewording an existing merge commit runs the hook with MERGE_HEAD
    already gone, over a commit that still has two parents: the hook half
    rejects it, the range half exempts it. `docs/nfr.md` and `mise.toml` both
    state this. If this case fails because the hook half now *accepts*, the
    residual is closed and both documents want editing; if it fails the other
    way, the exemption has been lost somewhere.
    """

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()
        self.git("merge", "--no-commit", "--no-ff", "topic")
        self.git("commit", "-q", "--no-verify", "-m", MERGE_SUBJECT.strip())
        self.assertFalse(self.merge_head(), "the merge is still in progress")

    def test_the_range_half_exempts_the_written_merge(self):
        self.assertAccepted(self.range_gate(self.base))

    def test_the_hook_half_rejects_the_same_merge_being_amended(self):
        """The divergence itself. `--no-verify` is the escape, and is documented."""
        self.assertRejected(self.gate(MERGE_SUBJECT))


class AmbientGitEnvironment(GateTestCase):
    """The fixtures must not follow a git environment the caller exported.

    `commits:test` runs from `hooks:pre-push`, where git has already exported an
    environment naming the repository being pushed; a fixture that inherited it
    would `git init` and `git commit` into somebody's working tree while they
    pushed it. Four variables rather than one, because the scrub's claim is over
    the whole namespace: `GIT_OBJECT_DIRECTORY` and `GIT_INDEX_FILE` each leave
    their own trace in the decoy. The assertions are on the fixture rather than
    through `gate()`, which sets `GIT_DIR` outright and would pass under no
    scrub at all.
    """

    def setUp(self):
        decoy = tempfile.TemporaryDirectory()
        self.addCleanup(decoy.cleanup)
        self.decoy = Path(decoy.name)
        self.decoy_index = self.decoy / "index-that-should-never-be-written"
        subprocess.run(
            ["git", "-c", "init.defaultBranch=main", "init", "-q", "."],
            cwd=self.decoy,
            env=hermetic_environment(),
            capture_output=True,
            check=True,
        )

        exported = mock.patch.dict(
            os.environ,
            {
                "GIT_DIR": str(self.decoy / ".git"),
                "GIT_WORK_TREE": str(self.decoy),
                "GIT_OBJECT_DIRECTORY": str(self.decoy / ".git" / "objects"),
                "GIT_INDEX_FILE": str(self.decoy_index),
            },
        )
        exported.start()
        self.addCleanup(exported.stop)

        super().setUp()
        self.base = self.diverging_branches()
        self.git("merge", "--no-commit", "--no-ff", "topic")

    def decoy_git(self, *arguments):
        return subprocess.run(
            ["git", *arguments],
            cwd=self.decoy,
            env=hermetic_environment(),
            capture_output=True,
            text=True,
        )

    def test_an_exported_git_environment_reaches_none_of_the_fixture(self):
        self.assertNotEqual(
            self.decoy_git("rev-parse", "--verify", "--quiet", "HEAD").returncode,
            0,
            "the fixture wrote commits into the repository the ambient GIT_DIR names",
        )
        self.assertNotEqual(
            self.decoy_git("cat-file", "-e", self.git("rev-parse", "HEAD").stdout.strip()).returncode,
            0,
            "the fixture wrote objects into the directory the ambient "
            "GIT_OBJECT_DIRECTORY names",
        )
        self.assertFalse(
            self.decoy_index.exists(),
            "the fixture wrote the index the ambient GIT_INDEX_FILE names",
        )
        # The fourth reading, and the only one that depends on the fixture's
        # *refs* rather than on the decoy's contents.
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")


if __name__ == "__main__":
    unittest.main()
