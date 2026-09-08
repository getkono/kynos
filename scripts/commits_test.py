"""Tests that this repository's one rule about commit messages holds the same
way in both of the instruments that state it, over the input that made them
disagree: a merge.

The rule is "Conventional Commits, merge commits exempt", and it is stated
twice, through instruments that cannot see the same thing.

`commits:check` hands convco a range, and convco drops a merge from the walk by
parent count -- `revwalk.filter(|c| c.parent_count() <= 1)`, driven by
`no_merge_commits: !config.merges`. `commits:message` hands convco one message
on stdin, and a bare message carries no parent count, so the exemption cannot
exist on that path unless the task supplies it. Nor can convco be told to: the
`--from-stdin` branch of `convco check` returns before
`--ignore-message-pattern` is ever consulted.

So both halves are run here, over the same commit, and every case that asserts
one half's verdict on a merge asserts the other's too. Running only the hook
half would leave the range half's verdicts asserted in prose, and the range
half is the one that moves under maintenance: a `.convco` holding
`merges: true` -- the very switch behind `no_merge_commits` -- flips
`convco check BASE..HEAD` from exit 0 to exit 1 over a merge subject while
leaving the hook half untouched, and a `[tools]` bump of convco can do the
same. That is issue #132 with its sign flipped, and a suite that runs one half
cannot see it.

Nothing below forges a state file, and nothing below stands in for git. The
fixtures build real repositories, reach a real merge with `git merge`, and one
of them installs the `commit-msg` hook command that `hk.pkl` declares -- read
out of `hk.pkl` rather than restated here, so that deleting the step, renaming
it, moving it out of the `commit-msg` hook, or dropping its
`< {{commit_msg_file}}` redirect fails these tests -- and then runs a real
`git merge --no-ff` through it. That case is the reported symptom itself:
before the fix, it is the `Not committing merge` the issue opens with. Its
boundary is written down at the fixture: the command is wrapped in a two-line
prologue the real hook does not have, so it proves git's ordering and not the
environment hk supplies.

One fixture is a linked worktree, because this repository is worked in linked
worktrees and MERGE_HEAD lives under `.git/worktrees/<name>/` there. The
task's guard has to be worktree-correct, and only a linked-worktree fixture
holds it to that. One case in it runs the guard with no `GIT_DIR` supplied at
all, which is the only place git's repository *discovery* is exercised --
every other call exports one, and under an exported `GIT_DIR` the weaker
spellings this guard was chosen over pass too.

Run it as `mise run commits:test`, or directly. There is no Python test runner
in this repository and `unittest` needs none.
"""

import os
import re
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
# them redirects the fixture's own `git init` and `git commit` at whatever it
# names, which for that hook is the repository being pushed. A test that writes
# into the tree that invoked it is the failure this scrub exists to make
# impossible, rather than to make unlikely. `AmbientGitEnvironment` holds it.
HERMETIC = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
    "GIT_AUTHOR_NAME": "Commit Gate Tests",
    "GIT_AUTHOR_EMAIL": "commit-gate-tests@invalid",
    "GIT_COMMITTER_NAME": "Commit Gate Tests",
    "GIT_COMMITTER_EMAIL": "commit-gate-tests@invalid",
    # `git merge` opens an editor for its message when it thinks it is
    # interactive. It does not think so here, because these pipes are not a
    # terminal -- but that is a property of how the suite happens to be run,
    # and the cases that complete a real merge would hang rather than fail if
    # it ever stopped holding. Stated, so it is not left to be inferred.
    "GIT_MERGE_AUTOEDIT": "no",
}

MERGE_SUBJECT = "Merge remote-tracking branch 'origin/master' into topic\n"
CONVENTIONAL_SUBJECT = "fix(hooks): complete the merge by hand\n"
SQUASH_SUBJECT = "Squashed commit of the following:\n"

# convco's own words when a subject is not a Conventional Commit. Asserted on
# rather than a bare non-zero exit, because a non-zero exit is also what an
# absent convco (127), a broken task file or a mise failure produce -- and each
# of those would let both reject-cases pass for a reason unrelated to the rule.
REJECTION = "first line doesn't match"


def scrubbed_environment():
    """This process's environment with every inherited `GIT_*` name removed."""
    return {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


def braced_body(text, key, within=None):
    """The `{ ... }` that follows `key` in `text`, by matching braces.

    Brace-matched rather than pattern-matched, and `within` is a slice rather
    than a hint, because what the caller needs is *containment*: a step named
    somewhere in the file is not a step attached to the hook that runs it. An
    unanchored search cannot tell those apart, and the difference is the whole
    gate. Double-quoted regions are skipped so a `{{commit_msg_file}}` inside
    a value cannot close the block that holds it.

    Returns None when `key` is absent from the region searched.
    """
    region = text if within is None else within
    offset = region.find(key)
    if offset < 0:
        return None
    opening = region.find("{", offset + len(key))
    if opening < 0:
        return None

    depth = 0
    quoted = False
    for index in range(opening, len(region)):
        character = region[index]
        if character == '"':
            quoted = not quoted
        elif quoted:
            continue
        elif character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return region[opening + 1 : index]
    return None


def declared_commit_msg_check():
    """The `check` command `hk.pkl` declares for the `conventional-commit` step.

    Read rather than restated, so the fixture that runs a real `git merge`
    through a real hook runs what the repository actually installs. Delete the
    step, rename it, move it out of the `commit-msg` hook, or drop its
    `< {{commit_msg_file}}` redirect, and the end-to-end case stops passing
    instead of going on asserting a command no hook would run.

    The third of those is the one an unanchored search misses, and it is the
    one that disarms the gate most completely: a `conventional-commit` step
    declared under `pre-push` runs nothing at commit time, while still being
    findable by name anywhere in the file.
    """
    text = (ROOT / "hk.pkl").read_text()
    hook = braced_body(text, '["commit-msg"]')
    if hook is None:
        raise AssertionError("hk.pkl declares no `commit-msg` hook")
    step = braced_body(text, '["conventional-commit"]', within=hook)
    if step is None:
        raise AssertionError("hk.pkl's `commit-msg` hook declares no `conventional-commit` step")
    check = re.search(r'check\s*=\s*"(.*?)"\s*$', step, re.MULTILINE)
    if check is None:
        raise AssertionError("the `conventional-commit` step declares no `check`")
    return check.group(1)


def declared_merge_guard():
    """The shell body `mise.toml` declares for `[tasks."commits:message"]`.

    Read for the reason the hook command is read out of `hk.pkl`: the case
    below runs the guard where `mise run` cannot put it, and running a
    restatement there would hold nothing.
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
    convco anywhere; resolving it here keeps that call on the same binary
    every other case reaches through `mise run`.
    """
    located = subprocess.run(
        ["mise", "which", "convco"], cwd=ROOT, capture_output=True, text=True
    )
    if located.returncode != 0:
        raise AssertionError(f"mise cannot resolve convco: {located.stderr}")
    return str(Path(located.stdout.strip()).parent) + os.pathsep + os.environ.get("PATH", "")


class GateTestCase(unittest.TestCase):
    """A throwaway repository per test, and both halves of the rule over it."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repository = Path(directory.name)

        self.env = {**scrubbed_environment(), **HERMETIC}
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
        from. Empty commits, because what the merge has to be is
        non-fast-forward, not conflicted: the state the hook sees is the same
        either way, and a conflict would need file contents that say nothing
        about the rule.
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
        rather than `git rev-parse --verify MERGE_HEAD`, because that resolves
        a *ref*: a branch or tag of that name answers it with no merge in
        progress at all. `RefNamedMergeHead` holds the task to the same.
        """
        located = self.git("rev-parse", "--git-path", "MERGE_HEAD", cwd=cwd)
        return (Path(cwd or self.repository) / located.stdout.strip()).is_file()

    # -- the two halves of the rule ---------------------------------------

    def gate(self, message, git_dir=None, work_tree=None):
        """The hook half: `commits:message` over one message.

        Run from the project root, so mise resolves `mise.toml`, with `GIT_DIR`
        and `GIT_WORK_TREE` redirected at the throwaway repository -- which is
        what lets both facts hold at once.
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

        Every other call exports `GIT_DIR`, which is what lets `mise run`
        resolve `mise.toml` from the project root while the task reads a
        throwaway repository. It also means the guard is never asked to find
        the repository itself -- and finding it is the whole difference
        between `--git-path` and the spellings it was chosen over.

        `mise run` executes a task in its config's directory rather than the
        caller's, so there is no way to put the task itself in this position.
        What runs here is the `run` body `mise.toml` declares, read out of it,
        with `GIT_DIR` and `GIT_WORK_TREE` absent and `cwd` inside the
        repository under test -- which is the position git actually invokes a
        `commit-msg` hook from.
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

        The same task the `commits` CI job and `hooks:pre-push` run, driven
        through the `CONVCO_RANGE` branch it already carries for exactly this.
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
        """Rejected *by the rule*, not merely exited non-zero.

        A missing convco exits 127 and a broken task file exits 1, and either
        would let this pass without the rule being consulted at all.
        """
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

    This is the case that makes `commits:test`'s name true. Every other class
    below tests the hook half against some state; this one tests that the two
    halves reach the same verdict, which is the property the whole change
    exists to establish and the only one that catches a regression on the
    range side.
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

    def test_both_halves_reject_the_same_one_parent_merge_subject(self):
        """A subject spelled `Merge ...` on a one-parent commit is not exempt.

        This is the guard that keeps the exemption keyed on the state rather
        than on the words, and it is asserted on both halves because a fix that
        widened one of them would be invisible in the other.
        """
        self.assertRejected(self.gate(MERGE_SUBJECT))
        self.git("commit", "-q", "--allow-empty", "--no-verify", "-m", MERGE_SUBJECT.strip())
        self.assertRejected(self.range_gate(self.base))

    def test_both_halves_accept_a_conventional_commit(self):
        """The baseline, on both halves, over the same commit.

        The subject put to the hook half is then written and walked by the
        range half, rather than the range half walking whatever the fixture
        happened to leave at HEAD -- which is what "over the same commit"
        has to mean for every case in this class if it is to mean it for any.
        """
        self.assertAccepted(self.gate(CONVENTIONAL_SUBJECT))
        self.git("commit", "-q", "--allow-empty", "--no-verify", "-m", CONVENTIONAL_SUBJECT.strip())
        self.assertAccepted(self.range_gate(self.base))



class RealMergeThroughARealHook(GateTestCase):
    """The reported symptom, observed rather than cited.

    Issue #132 is `git merge` printing `Not committing merge; use 'git commit'
    to complete the merge.` The ordering the whole fix rests on -- that git
    writes MERGE_HEAD before it runs `commit-msg` -- was supported by a reading
    of `builtin/merge.c`. Here it is an observation: a real merge, through the
    real hook command, in a repository that exists for eight lines.

    The hook runs the command `hk.pkl` declares, read out of `hk.pkl`. It is
    written into the *fixture's* `.git/hooks`, which is that repository's own
    and is deleted with it. Nothing is installed into this clone, and no
    `--local` git config is written anywhere.
    """

    def setUp(self):
        super().setUp()
        self.base = self.diverging_branches()

        hook = self.repository / ".git" / "hooks" / "commit-msg"
        hook.parent.mkdir(parents=True, exist_ok=True)
        # The two prologue lines are this fixture's boundary, and they are what
        # makes it pass. `mise run` resolves `mise.toml` from its config's
        # directory, so the command has to run from the project root -- and
        # once it does, `GIT_DIR` has to be carried in, because git does not
        # export one to a `commit-msg` hook. In the real repository those two
        # lines are unnecessary: hk already runs from the root, and that root
        # is the repository being committed to.
        #
        # So what this case proves is git's ordering -- that MERGE_HEAD is
        # written before `commit-msg` runs -- over the command `hk.pkl`
        # actually declares. What it does not prove is that the step works in
        # the environment hk hands it, since two lines of that environment are
        # supplied here. `LinkedWorktree.test_the_guard_finds_the_repository_itself`
        # is what covers the guard with no `GIT_DIR` supplied at all.
        command = declared_commit_msg_check().replace("{{commit_msg_file}}", '"$message"')
        hook.write_text(
            "#!/bin/sh\n"
            'GIT_DIR="$(git rev-parse --absolute-git-dir)"\n'
            "export GIT_DIR\n"
            'message="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"\n'
            'cd "' + str(ROOT) + '" || exit 1\n'
            "exec " + command + "\n"
        )
        hook.chmod(0o755)

    def test_a_real_merge_completes_through_the_hook(self):
        """`git merge --no-ff` writes its own merge commit. Issue #132, closed."""
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

        # And the half that walks a range agrees about what the hook let past.
        self.assertAccepted(self.range_gate(self.base))

    def test_the_hook_still_refuses_an_ordinary_commit(self):
        """The control. Without it the case above passes for a dead hook.

        A one-parent commit carrying a merge-shaped subject has to be refused,
        and `HEAD` has to be where it was.
        """
        before = self.git("rev-parse", "HEAD").stdout.strip()
        refused = subprocess.run(
            ["git", "commit", "--allow-empty", "-m", "Merge branch 'nothing'"],
            cwd=self.repository,
            env=self.env,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(refused.returncode, 0, "the hook let a one-parent merge subject past")
        self.assertIn(REJECTION, refused.stdout + refused.stderr)
        self.assertEqual(before, self.git("rev-parse", "HEAD").stdout.strip())


class LinkedWorktree(GateTestCase):
    """The guard has to be worktree-correct, because this repository is worktrees.

    In a linked worktree MERGE_HEAD is under `.git/worktrees/<name>/`, not
    `.git/`. A guard spelled against `.git/MERGE_HEAD` reports "no merge"
    throughout a real merge here, and every plain-`git init` fixture in this
    file would stay green while it did.
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

        Every other case exports `GIT_DIR`, because that is what lets
        `mise run` resolve `mise.toml` from the project root while the task
        reads a throwaway repository. The cost is that the guard is never
        asked to *find* the repository -- and finding it is the whole
        difference between `--git-path` and the spellings it was chosen over.
        Without this case, `[ -f "$GIT_DIR/MERGE_HEAD" ]` passes the suite,
        while in the position git actually runs a `commit-msg` hook from --
        cwd at the worktree top, `GIT_DIR` unset -- it exempts nothing.
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
    carried first -- resolves a *ref*, so a branch, a tag or a bare
    `refs/MERGE_HEAD` of that name makes it exit 0 with no merge anywhere. The
    gate would then exempt a one-parent commit that `commits:check` and the
    `commits` CI job reject, which is a silent and total loss of the gate in
    the direction opposite to the one it was written to fix.
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

    def test_a_conventional_subject_passes_with_no_merge_in_progress(self):
        self.assertAccepted(self.gate(CONVENTIONAL_SUBJECT))

    def test_a_squash_merge_leaves_no_merge_head_and_stays_checked(self):
        """`git merge --squash` is not a merge as far as this rule is concerned.

        It writes SQUASH_MSG and no MERGE_HEAD, runs no commit-msg hook of its
        own, and the commit it leads to has one parent -- which `commits:check`
        and the `commits` CI job do check. So its default message has to be
        rejected here, or the hook would pass what CI fails.
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

    Amending or rewording an existing merge commit runs the hook with
    MERGE_HEAD already gone, over a commit that still has two parents. The
    hook half rejects it; the range half exempts it. `docs/nfr.md` and
    `mise.toml` both state this, and until this case existed nothing held it --
    so a change closing it, or widening it, would have left both documents
    silently wrong.

    If this case fails because the hook half now *accepts*, the residual is
    closed and both documents want editing. If it fails the other way, the
    exemption has been lost somewhere and the range half is what to read next.
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

    This is the case that makes the scrub a rule rather than a habit.
    `commits:test` runs from `hooks:pre-push`, which git invokes with a git
    environment already in place, and the repository that environment names is
    the one being pushed. A fixture that inherited it would `git init` and
    `git commit` into somebody's working tree while they pushed it.

    Four variables are exported, not one, because the scrub's claim is over the
    whole `GIT_*` namespace and a guard that only exports `GIT_DIR` would stay
    green if the scrub were narrowed to name two variables by hand.
    `GIT_OBJECT_DIRECTORY` and `GIT_INDEX_FILE` each leave their own trace:
    the first sends the fixture's commit objects to the decoy, and the second
    creates a file that does not otherwise exist.

    `mock.patch.dict` mutates this process's own `os.environ`, which is safe
    because `unittest` runs cases serially in one thread; it is undone by
    `addCleanup` whatever the case does.

    What holds the scrub is the three decoy assertions and the merge state,
    and nothing else here could: `gate()` sets `GIT_DIR` and `GIT_WORK_TREE`
    outright, so putting a message through it would pass under any scrub at
    all, including none. The subject of this class is the fixture's
    environment, so the fixture is what it asserts on.
    """

    def setUp(self):
        decoy = tempfile.TemporaryDirectory()
        self.addCleanup(decoy.cleanup)
        self.decoy = Path(decoy.name)
        self.decoy_index = self.decoy / "index-that-should-never-be-written"
        subprocess.run(
            ["git", "-c", "init.defaultBranch=main", "init", "-q", "."],
            cwd=self.decoy,
            env={**scrubbed_environment(), **HERMETIC},
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
            env={**scrubbed_environment(), **HERMETIC},
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
        # The merge state is the fourth reading, and the only one that depends
        # on the fixture's *refs* rather than on the decoy's contents.
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")


if __name__ == "__main__":
    unittest.main()
