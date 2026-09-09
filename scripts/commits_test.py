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

So both halves are run here. `BothHalvesOverOneMerge` puts each verdict to
both, over the same commit, and that is where the two are held to agree. It is
not a universal, and cannot be: `TheAmendResidual` exists precisely to pin the
one state where the halves *disagree*, and the linked-worktree cases reach the
hook half alone because what they are about is where the guard looks, not what
convco says. Running only the hook half everywhere would leave the range
half's verdicts asserted in prose, and the range half is the one that moves
under maintenance: a `.convco` holding `merges: true` -- the very switch behind
`no_merge_commits` -- flips `convco check BASE..HEAD` from exit 0 to exit 1
over a merge subject while leaving the hook half untouched, and a `[tools]`
bump of convco can do the same. That is issue #132 with its sign flipped, and a
suite that runs one half cannot see it.

Nothing below forges a state file, and nothing below stands in for git. The
fixtures build real repositories and reach a real merge with `git merge`.

Whether the gate is armed at all is settled by its outcome rather than by
reading `hk.pkl`. `TheGateHkRuns` installs the pinned `hk run commit-msg` as a
throwaway repository's own `commit-msg` hook and drives real commits and a real
`git merge --no-ff` through it, so hk resolves this repository's `hk.pkl`,
decides for itself whether the `conventional-commit` step runs, and answers
with an exit code. One reading subsumes every way the step can be turned off --
renamed, deleted, moved under another hook, overridden by a merged duplicate
entry, disabled by a module-level `skip_steps` or `skip_hooks`, replaced by a
`shell`, a `prefix` or a `check` of `true`, turned into a fix-only run, or
disarmed by an `hk.local.pkl` -- because none of those survives being asked
what the hook actually did. The merge case in that class is the reported
symptom itself: before the fix, it is the `Not committing merge` the issue
opens with.

Running the real gate is hermetic because the git environment that hook hands
hk is the fixture's own, spelled absolutely. hk finds `hk.pkl` and `mise.toml`
by where it runs, so it runs at the project root -- and git spells the
environment it gives a hook relative to the work tree it invoked the hook
from, so `GIT_DIR` and the `GIT_INDEX_FILE` git exports itself are both
resolved before that move. Every git read the step then makes -- including the
`git rev-parse --git-path MERGE_HEAD` the exemption is spelled as -- follows
`GIT_DIR` to the throwaway repository instead. A developer with a merge of
their own in progress therefore runs this suite to the same answer as one with
none, and nothing is written into the repository being tested: the only file
the hook is handed is the fixture's own message.

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


def hermetic_environment():
    """The environment every fixture, every tool lookup and the gate itself run in.

    `HERMETIC` over the `GIT_*` scrub, and then every inherited `HK_*` name
    removed as well. The second half is not housekeeping either: `HK_SKIP_STEPS`
    and `HK_SKIP_HOOK` in somebody's shell each make `hk run commit-msg` exit 0
    having run nothing, and the subject of these cases is what this repository
    declares rather than what one machine's shell overrides. That escape hatch
    is committed nowhere and must not answer for the repository.
    `TheGateHkRuns.test_an_exported_hk_skip_does_not_reach_the_gate` holds it.
    """
    return {
        key: value
        for key, value in {**scrubbed_environment(), **HERMETIC}.items()
        if not key.startswith("HK_")
    }


# The one place this suite reads a declaration out of another file, and what
# makes it safe. `LinkedWorktree` runs the `commits:message` body where
# `mise run` cannot put it -- cwd inside the repository under test, no `GIT_DIR`
# exported -- and running a restatement there would hold nothing.
#
# Nothing is read out of `hk.pkl`. What that file declares is no longer any of
# this suite's business: `TheGateHkRuns` hands hk the message and reads the exit
# code, so hk resolves its own configuration and every disarm shows up in the
# answer rather than in a scan. The scanner that used to make those reads safe
# -- a comment- and string-aware mask, a brace matcher, and the class of cases
# that held the two of them to it -- went with them.
#
# The read below is safe for a reason that does not generalise, so it is
# written down rather than assumed: a TOML comment begins with `#`, and both
# patterns anchor to the start of a line at a position where they require `[`
# or `r`. A commented-out `#[tasks."commits:message"]` or `# run = '''` cannot
# match. What the body then captures is verbatim, which is correct twice over:
# a `#` line inside `run = '''...'''` is shell to mise and shell to the fixture
# alike, so there is nothing there to mask.
def declared_merge_guard():
    """The shell body `mise.toml` declares for `[tasks."commits:message"]`.

    Read rather than restated, because `LinkedWorktree` runs this body where
    `mise run` cannot put it -- cwd inside the repository under test, with no
    `GIT_DIR` exported -- and running a restatement there would hold nothing.

    No mask here, and that is the comment above rather than an oversight: both
    patterns anchor where a TOML comment's `#` would have to be, so neither can
    match a commented-out line, and the captured body is shell in which a `#`
    line means the same thing to mise and to the fixture.
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

    Resolved under `hermetic_environment` for the reason everything else runs
    under it, and failing through `check=True` rather than through a raise of
    its own: a hand-written raise that no case reaches is one more thing that
    can be switched off without anything noticing, and `CalledProcessError`
    carries the same command and the same exit code.
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

    Asked of mise rather than of `PATH`, so the gate the cases below drive is
    the `hk` version `[tools]` pins -- which is the one whose behaviour these
    assertions were measured against. Under `hermetic_environment` and failing
    through `check=True`, for the two reasons `convco_on_path` gives.
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


class TheGateHkRuns(GateTestCase):
    """The gate hk actually runs, asked what it did rather than what it declares.

    Issue #132 is `git merge` printing `Not committing merge; use 'git commit'
    to complete the merge.` The ordering the whole fix rests on -- that git
    writes MERGE_HEAD before it runs `commit-msg` -- was once supported by a
    reading of `builtin/merge.c`. Here it is an observation: a real merge,
    through the real gate, in a repository that exists for the length of one
    case.

    The hook installed below is `hk run commit-msg`, so hk reads this
    repository's own `hk.pkl`, resolves whatever the `commit-msg` hook declares
    after Pkl has merged every entry that shares a key and after every setting
    hk merges from every source, decides for itself whether to run the step,
    and answers with an exit code that `git commit` and `git merge` obey. Three
    readings of that one exit code are what this class is:

        no merge, a non-conforming subject   -> the commit is refused
        no merge, a Conventional subject     -> the commit is written
        a merge in progress, its own subject -> the merge commit is written

    That is one assertion about an outcome in place of an enumeration of
    causes, and it is why nothing here reads `hk.pkl` at all. Every way of
    turning the step off is a way of turning the first reading green: renaming
    the step, deleting it, moving it under another hook, dropping its
    `< {{commit_msg_file}}` redirect, a `shell` or a `prefix` of `true`, a
    `check` of `true` on a merged duplicate `["conventional-commit"]` entry, a
    second `["commit-msg"]` hook entry, a module-level `skip_steps` or
    `skip_hooks`, a fix-only run, an `hk.local.pkl` in the project root, or a
    key hk has not shipped yet. A scan of the file's text can be blind to any
    of those; the exit code is blind to none of them, because it is the result
    and they are the causes.

    The second and third readings are what stop the first from being satisfied
    by a gate that refuses everything, which is the failure mode a
    disarm-detector has instead of the one it replaced.

    Nothing is installed into this clone and no `--local` git config is written
    anywhere: the hook is the *fixture's* own `.git/hooks/commit-msg`, and it
    is deleted with the fixture. `GIT_DIR` is the fixture's too, so the merge
    state hk's step reads is the fixture's merge state and never the state of
    the repository the suite is being run in.
    """

    def setUp(self):
        super().setUp()
        self.changes = 0
        self.diverging_branches_carrying_files()

        hook = self.repository / ".git" / "hooks" / "commit-msg"
        hook.parent.mkdir(parents=True, exist_ok=True)
        # The prologue is this fixture's boundary, and it is what makes the
        # fixture hermetic rather than what makes it pass. hk resolves `hk.pkl`
        # and `mise.toml` from where it runs, so it has to run at the project
        # root -- and every git path has to be absolute before that `cd`,
        # because git spells the environment it hands a hook relative to the
        # work tree it invoked the hook from. In the real repository none of
        # this is needed: hk already runs from the root, and that root is the
        # repository being committed to.
        #
        # Absolutising is the whole reason this is hermetic, and the three
        # names carry different weight:
        #
        # `GIT_DIR` is the one the exemption reads. The step's guard is
        # `git rev-parse --git-path MERGE_HEAD`, and under the fixture's
        # `GIT_DIR` that resolves inside the fixture, so somebody running
        # `mise run check` in the middle of a merge of their own gets the same
        # three answers as somebody running it on a clean tree.
        #
        # `GIT_WORK_TREE` is deliberately *not* exported, and that is the one
        # asymmetry here. git does not export one, so the work tree of the
        # fixture's `GIT_DIR` stays the directory hk runs in -- the project
        # root -- which is exactly what the fixture needs: hk runs a step's
        # command from the repository root it resolves, and the step's command
        # is `mise run`, which has to land where `mise.toml` is. Pinning the
        # work tree to the fixture instead makes hk run the step in the
        # fixture, where mise reports `no tasks defined`. What the mismatched
        # pair costs is that hk's own staged-status read compares the
        # fixture's index against the project root's files; what it buys is
        # that every git read the *step* makes follows `GIT_DIR` home.
        #
        # `GIT_INDEX_FILE` git *does* export, as `.git/index` relative to the
        # fixture. Past the `cd` that spelling names the project root's index
        # instead, and hk reads its staged status for the fixture's repository
        # out of it before it runs any step. Measured on hk 1.53.0: the objects
        # that index refers to are in the other repository, so hk dies with
        # `failed to get staged statuses ... NotFound (-3)` and every case here
        # fails. It only ever failed in a plain clone, which is what CI checks
        # out: in a linked worktree `.git` is a file rather than a directory,
        # `.git/index` cannot be opened at all, and libgit2 falling back to no
        # index is what kept this class green where it is developed. That
        # asymmetry is the residual, and it points one way: a regression of
        # this line is caught in a plain clone and passes in a worktree, so it
        # is CI that holds it rather than the tree it is written in.
        hook.write_text(
            "#!/bin/sh\n"
            'message="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"\n'
            'GIT_DIR="$(git rev-parse --absolute-git-dir)"\n'
            "export GIT_DIR\n"
            'if [ -n "${GIT_INDEX_FILE:-}" ]; then\n'
            '  GIT_INDEX_FILE="$(cd "$(dirname "$GIT_INDEX_FILE")" && pwd)/'
            '$(basename "$GIT_INDEX_FILE")"\n'
            "  export GIT_INDEX_FILE\n"
            "fi\n"
            'cd "' + str(ROOT) + '" || exit 1\n'
            'exec "' + hk_binary() + '" run commit-msg "$message"\n'
        )
        hook.chmod(0o755)

    def stage_a_change(self):
        """One Rust source and one Markdown document, staged.

        A commit needs something to commit, and these cases drive real
        `git commit` and `git merge` invocations, so the fixture supplies it.

        What the staged files do *not* do is exercise hk's file selection, and
        that wants saying because it reads as though they should. Measured on
        hk 1.53.0 under `HK_LOG=debug`: over a `commit-msg` hook the file set
        hk builds is the message file alone -- `files: {"MSG"}`, the staged
        sources absent -- and it runs this step over `0 files`. A `glob`,
        `types` or `exclude` key on the step therefore narrows that set to
        nothing at *every* value, `glob = "*"` included, and hk skips the step
        reporting `all files deleted before execution`.

        The residual, so this class's claim is not read as larger than it is:
        those three keys are caught here on the fixture's file set rather than
        on a real commit's, where the message file is one hk resolves against
        the directory it runs in. `hk.pkl` records the same where the keys
        would be declared.
        """
        self.changes += 1
        source = self.repository / f"change-{self.changes}.rs"
        source.write_text(f"pub fn change_{self.changes}() {{}}\n")
        document = self.repository / f"change-{self.changes}.md"
        document.write_text(f"# change {self.changes}\n")
        self.git("add", source.name, document.name)

    def diverging_branches_carrying_files(self):
        """`diverging_branches`, with a file on every commit and no range.

        The ancestor is not returned because no case here walks a range, and
        the sides are not empty because the merge has to carry a file across
        for the step to have anything to select.
        """
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the common ancestor")
        self.git("switch", "-q", "-c", "topic")
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the topic side")
        self.git("switch", "-q", "main")
        self.stage_a_change()
        self.git("commit", "-q", "--no-verify", "-m", "feat: the trunk side")

    def commit(self, subject, environment=None):
        """One ordinary commit, carrying a change, put to the gate hk runs."""
        self.stage_a_change()
        return subprocess.run(
            ["git", "commit", "-m", subject],
            cwd=self.repository,
            env=environment or self.env,
            capture_output=True,
            text=True,
        )

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

        The exemption, end to end: git writes MERGE_HEAD, runs `commit-msg`,
        hk runs the step, the step reads the file git wrote, and the merge
        commit exists. Every link is real, and the ordering the fix rests on is
        observed here rather than cited.
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

        A one-parent commit carrying a merge-shaped subject has to be refused,
        by convco's own words, with `HEAD` where it was. A step hk does not run
        -- for any of the reasons in this class's docstring -- fails here and
        nowhere else.
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

        hk builds a `commit-msg` step's file set from the repository the hook
        was invoked for, and that set is the commit's staged and modified
        files -- the message file is not in it. The set is what a `glob`, a
        `types` or an `exclude` key on the step would be matched against, so a
        fixture whose set comes out empty answers for all three at every
        value: it reports `glob = "*"`, which selects everything staged and
        disarms nothing, exactly as it reports a `glob` matching no file,
        which disarms the gate outright.

        Read out of hk's own `HK_LOG=debug` line rather than inferred,
        because the exit code the rest of this class reads cannot see it: with
        no file-selecting key declared, hk runs the step over an empty set as
        readily as over a full one, and the gate refuses the subject either
        way.
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

    def test_an_exported_hk_skip_does_not_reach_the_gate(self):
        """`HK_SKIP_STEPS` in somebody's shell is not this repository's answer.

        Measured on hk 1.53.0: exported into `hk run commit-msg`, both
        `HK_SKIP_STEPS=conventional-commit` and `HK_SKIP_HOOK=commit-msg` make
        it exit 0 over a merge subject having run nothing. That escape hatch is
        committed nowhere, so `hermetic_environment` strips the whole `HK_*`
        namespace before any fixture sees it -- and this is the case that holds
        the strip, since every other case here would pass without it.
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
        # The merge state is the fourth reading, and the only one that depends
        # on the fixture's *refs* rather than on the decoy's contents.
        self.assertTrue(self.merge_head(), "the fixture did not reach a merge state")


if __name__ == "__main__":
    unittest.main()
