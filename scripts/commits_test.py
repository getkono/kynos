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
under maintenance: a `.convco` holding
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

Whether hk would run that step at all is a different question, and it is put
to hk rather than inferred from the text of its configuration:
`HkWouldRunTheStep` holds `hk run commit-msg --plan --json` to reporting the
step `included`, and `hk config dump` to skipping no hook named `commit-msg`.
A step hk skips still hands the fixture above a command to install and pass a
merge through, while the hook a commit reaches runs nothing.

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

import json
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


# The characters a quoted value must not be able to contribute to a scan: two
# that open or close a block, and two that start a comment.
STRUCTURAL = "{}/*"


def masked_source(region):
    """`region` with comments blanked and quoted values neutralised, index for index.

    Every replacement is one character wide and newlines are preserved, so an
    offset into the mask is the same offset into `region`: a caller searches
    the mask and slices the source.

    The two kinds of span are treated differently on purpose. A comment is not
    code, so it is blanked entirely and nothing can be *found* in one. A
    quoted span is code -- in Pkl a block's own key is a quoted string -- so it
    stays findable, and only the characters that could open a block, close one
    or begin a comment are replaced. A value can then never be structure, and
    `["commit-msg"]` is still something `find` can locate.

    Block comments do not nest, and the first `*/` ends one. That is not a
    simplification: `/* a /* b */ { */` is a syntax error to Pkl itself, which
    `mise exec -- hk validate` rejects with `expected identifier, got LBrace`,
    so no file hk accepts can distinguish a nesting scanner from this one.
    """
    masked = []
    index = 0
    length = len(region)
    while index < length:
        pair = region[index : index + 2]
        if pair == "//":
            end = region.find("\n", index)
            end = length if end < 0 else end
            masked.append(" " * (end - index))
            index = end
        elif pair == "/*":
            closing = region.find("*/", index + 2)
            end = length if closing < 0 else closing + 2
            masked.append("".join(" " if c != "\n" else "\n" for c in region[index:end]))
            index = end
        elif region[index] == '"':
            masked.append('"')
            index += 1
            while index < length:
                character = region[index]
                if character == "\\":
                    masked.append("  ")
                    index += 2
                    continue
                masked.append(" " if character in STRUCTURAL else character)
                index += 1
                if character == '"':
                    break
        else:
            masked.append(region[index])
            index += 1
    return "".join(masked)


def braced_body(text, key, within=None):
    """The `{ ... }` that follows `key` in `text`, by matching braces.

    Brace-matched rather than pattern-matched, and `within` is a slice rather
    than a hint, because what the caller needs is *containment*: a step named
    somewhere in the file is not a step attached to the hook that runs it. An
    unanchored search cannot tell those apart, and the difference is the whole
    gate.

    The search and the scan both run over `masked_source`, which is what makes
    that true of the *lookup* and not only of the brace counting. Skipping
    comments inside the depth loop alone left both `find` calls reading raw
    text, so a step that existed only as a commented-out block was located and
    returned as a live declaration -- and the end-to-end fixture could not
    notice, because it installs as its hook the string this function read.
    A scan that finds nothing fails loudly; one that finds the wrong block
    reports green, and that is the failure this exists for.

    Returns None when `key` is absent from the code of the region searched.
    """
    region = text if within is None else within
    mask = masked_source(region)
    offset = mask.find(key)
    if offset < 0:
        return None
    opening = mask.find("{", offset + len(key))
    if opening < 0:
        return None

    depth = 0
    for index in range(opening, len(mask)):
        if mask[index] == "{":
            depth += 1
        elif mask[index] == "}":
            depth -= 1
            if depth == 0:
                return region[opening + 1 : index]
    return None


# Every place this suite reads a declaration out of `hk.pkl` or `mise.toml`,
# and what makes each one safe. The list is here because this hazard has
# recurred -- `//` comments, then `/* */`, then `braced_body`'s two lookups,
# then the `check` regex below -- and each repair covered the sites it happened
# to know about. A reader adding a read belongs in this list.
#
# Each read is named and none is numbered, because the numbering is the part
# that went wrong: a read was inserted in the middle of the list, every read
# after it shifted, and a docstring citing two of them by position went on
# pointing at the pair it used to name -- one of which had become the read it
# was saying was unlike itself. A name survives an insertion, a deletion and a
# reordering; an ordinal survives none of the three, and the prose that cites
# it fails silently rather than loudly.
#
#   hk.pkl, via `declared_commit_msg_check`
#     the hook lookup -- `["commit-msg"]`, searched over `masked_source`
#     the step lookup -- `["conventional-commit"]`, searched over `masked_source`
#     the check lookup -- that step's `check = "..."`, searched over `masked_source`
#
#   mise.toml, via `declared_merge_guard`
#     the task header -- `[tasks."commits:message"]`, safe by construction
#     the task body -- that task's `run = '''`, safe by construction
#
# Every read here is about *what* the step runs. Whether hk runs it is not
# read out of `hk.pkl` at all -- `HkWouldRunTheStep` asks hk -- so no entry
# above has that as its subject.
#
# The task header and the task body are safe for a reason that does not
# generalise, so it is written down rather than assumed: a TOML comment begins
# with `#`, and both patterns anchor to the start of a line at a position where
# they require `[` or `r`. A commented-out `#[tasks."commits:message"]` or
# `# run = '''` cannot match. What the body then captures is verbatim, which is
# correct twice over: a `#` line inside `run = '''...'''` is shell to mise and
# shell to the fixture alike, so there is nothing there to mask.
def declared_check(step):
    """The `check` command a step body declares, ignoring any commented ones.

    A function of its own so it can be tested over a fragment: the property
    that matters here -- a commented `check` above a live one is not the
    declaration -- is invisible from the real `hk.pkl`, which has no commented
    `check` to trip over. Held that way, the whole read would be covered only
    by editing the repository's own configuration.

    Located on the mask, sliced from the source: the mask neutralises `{` and
    `}` inside a value, so the match's own text would come back with
    `{{commit_msg_file}}` blanked away.
    """
    check = re.search(r'check\s*=\s*"(.*?)"\s*$', masked_source(step), re.MULTILINE)
    if check is None:
        raise AssertionError("the `conventional-commit` step declares no live `check`")
    return step[check.start(1) : check.end(1)]


def declared_commit_msg_check():
    """The `check` command `hk.pkl` declares for the `conventional-commit` step.

    Read rather than restated, so the fixture that runs a real `git merge`
    through a real hook runs what the repository actually installs. Delete the
    step, rename it, move it out of the `commit-msg` hook, drop its
    `< {{commit_msg_file}}` redirect, and the end-to-end case stops passing
    instead of going on asserting a command no hook would run.

    Moving it out is the one an unanchored search misses, and it is the one
    that disarms the gate most completely: a `conventional-commit` step
    declared under `pre-push` runs nothing at commit time, while still being
    findable by name anywhere in the file.

    Every lookup here is about *what* the step runs. hk decides *whether* it
    runs, and a step hk skips still hands this function a command the
    end-to-end fixture will install, run a real merge through, and pass on --
    so that question is put to hk in `HkWouldRunTheStep` rather than inferred
    from the keys this step happens to declare.

    Every lookup runs over `masked_source`, including the one for the
    `check` line itself. Commenting a line out and writing its replacement
    below is the ordinary shape of a configuration edit, and this function
    decides the command the end-to-end fixture installs -- so a `check` read
    out of a comment is a command that fixture runs while hk runs something
    else, and the case cannot tell. The match is located on the mask and the
    text is sliced from the source, because the mask neutralises `{` and `}`
    inside a value: read off the mask, the command comes back with
    `{{commit_msg_file}}` blanked away.
    """
    text = (ROOT / "hk.pkl").read_text()
    hook = braced_body(text, '["commit-msg"]')
    if hook is None:
        raise AssertionError("hk.pkl declares no `commit-msg` hook")
    step = braced_body(text, '["conventional-commit"]', within=hook)
    if step is None:
        raise AssertionError("hk.pkl's `commit-msg` hook declares no `conventional-commit` step")
    return declared_check(step)


def declared_merge_guard():
    """The shell body `mise.toml` declares for `[tasks."commits:message"]`.

    Read for the reason the hook command is read out of `hk.pkl`: the case
    below runs the guard where `mise run` cannot put it, and running a
    restatement there would hold nothing.

    No mask here, and that is the task header and the task body of the list
    above rather than an oversight: both patterns anchor where a TOML
    comment's `#` would have to be, so neither can match a commented-out line,
    and the captured body is shell in which a `#` line means the same thing to
    mise and to the fixture.
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


def hk_binary():
    """The pinned hk, resolved the way `convco_on_path` resolves convco.

    Asked of mise rather than of `PATH`, so the answers below come from the
    `hk` version `[tools]` pins -- which is the one whose plan format and
    whose settings these assertions were measured against.
    """
    located = subprocess.run(["mise", "which", "hk"], cwd=ROOT, capture_output=True, text=True)
    if located.returncode != 0:
        raise AssertionError(f"mise cannot resolve hk: {located.stderr}")
    return located.stdout.strip()


def hk_answer(*arguments):
    """One hk subcommand's JSON answer about this repository's configuration.

    Run from the project root, because that is where hk finds `hk.pkl`, and
    under an environment with every `GIT_*` and `HK_*` name removed. The
    subject of these questions is what this repository declares: `HK_SKIP_HOOK`
    in somebody's shell is that machine's escape hatch and is not committed
    anywhere, and the global and system git configuration go to `os.devnull`
    because hk merges git config into its settings and a `[hk]` section in a
    `~/.gitconfig` must not answer for the repository.

    Every failure is loud. An hk that exits non-zero, or stdout that is not
    the document hk documents, leaves the question unanswered -- and an
    unanswered question about whether a gate is armed must not read as yes.
    """
    environment = {
        key: value
        for key, value in {**scrubbed_environment(), **HERMETIC}.items()
        if not key.startswith("HK_")
    }
    asked = subprocess.run(
        [hk_binary(), *arguments],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
    )
    spelled = "hk " + " ".join(arguments)
    if asked.returncode != 0:
        raise AssertionError(
            f"`{spelled}` exited {asked.returncode}\n"
            f"stdout:\n{asked.stdout}\nstderr:\n{asked.stderr}"
        )
    try:
        return json.loads(asked.stdout)
    except json.JSONDecodeError as unreadable:
        raise AssertionError(
            f"`{spelled}` did not answer with JSON: {unreadable}\n"
            f"stdout:\n{asked.stdout}\nstderr:\n{asked.stderr}"
        ) from unreadable


def commit_msg_plan():
    """hk's plan for the `commit-msg` hook, over a message nothing reads.

    `hk run commit-msg` requires the message file git hands its hook, and
    `--plan` prints what would run instead of running it, so the file's
    contents reach nothing. It holds the merge subject anyway, because that is
    the message this whole suite is about.
    """
    with tempfile.TemporaryDirectory() as scratch:
        message = Path(scratch) / "COMMIT_EDITMSG"
        message.write_text(MERGE_SUBJECT)
        return hk_answer("run", "commit-msg", "--plan", "--json", str(message))


def planned_status(plan, step):
    """The status hk's plan gives `step`, refusing a document it cannot read.

    Defensive about the shape and silent about none of it: hk owns this format
    and may change it, and every shape this cannot read is one where "the step
    is included" would be an answer nothing measured.

    A plan holding no step of that name is an error rather than an absence,
    because that is what deleting the step, moving it to another hook and
    renaming it each look like -- and the renamed case has a step whose status
    a laxer reading would return.
    """
    steps = plan.get("steps")
    if not isinstance(steps, list):
        raise AssertionError(f"hk's plan carries no list of steps: {plan!r}")
    for planned in steps:
        if isinstance(planned, dict) and planned.get("name") == step:
            status = planned.get("status")
            if not isinstance(status, str):
                raise AssertionError(f"hk's plan gives `{step}` no status: {planned!r}")
            return status
    named = [planned.get("name") for planned in steps if isinstance(planned, dict)]
    raise AssertionError(f"hk's plan holds no step named `{step}`; it names {named}")


def skipped_hooks(configuration):
    """The hooks hk's effective configuration skips.

    An absent setting is an error and not an empty list. The safe answer to
    this question is "none", so a shape the setting cannot be found in -- hk
    renaming it, or dropping it from the dump -- is the one shape that would
    pass silently for as long as it lasted.
    """
    if "skip_hooks" not in configuration:
        raise AssertionError(
            "hk's effective configuration carries no `skip_hooks`: the setting "
            f"this reads has been renamed or removed, and its keys are "
            f"{sorted(configuration)}"
        )
    return configuration["skip_hooks"]


class BracedBody(unittest.TestCase):
    """The scanner `hk.pkl`'s containment promise rests on, over its own inputs.

    Nothing else here reaches the regions `braced_body` masks: today's
    `hk.pkl` has balanced braces in its values and its comments alike, so a
    scanner that masked none of them would pass every other case in this file.
    That is the shape `containment_test.py` already tests its own scanner for,
    and the reason `mise.toml` gives for running that suite beside its gate --
    a gate is only as good as the parser under it, and a parser that breaks is
    what makes the gate pass silently.

    The fragments below are written for this file rather than captured. Each
    is the minimum that reaches a branch: enough pkl to be recognisable, and a
    brace where a scanner would trip.
    """

    def test_a_block_is_returned_without_its_own_braces(self):
        self.assertEqual(braced_body('["a"] {inside}', '["a"]'), "inside")

    def test_a_nested_block_does_not_end_the_outer_one(self):
        """The end is asserted, not just the contents.

        Returning at the first `}` -- the implementation `braced_body` exists
        to replace -- yields a body that still holds `["b"]` and still lacks
        `["c"]`, so the two containment assertions alone leave it green.
        """
        body = braced_body('["a"] {\n  ["b"] { x = 1 }\n}\n["c"] { y = 2 }', '["a"]')
        self.assertIn('["b"]', body)
        self.assertIn("x = 1 }", body)
        self.assertNotIn('["c"]', body)

    def test_an_open_brace_in_a_comment_does_not_extend_the_block(self):
        """A's hazard. That block's comments discuss `{{commit_msg_file}}`."""
        body = braced_body('["a"] {\n  // prose mentioning a { brace\n}\n["b"] { y = 2 }', '["a"]')
        self.assertNotIn('["b"]', body)

    def test_an_open_brace_in_a_block_comment_does_not_extend_the_block(self):
        """Pkl has `/* ... */` as well as `//`, and the same prose lives in both."""
        body = braced_body('["a"] {\n  /* prose with a { brace */\n}\n["b"] { y = 2 }', '["a"]')
        self.assertNotIn('["b"]', body)

    def test_a_block_comment_marker_inside_a_value_is_not_a_comment(self):
        """A glob such as `"**/*.rs"` carries `/*`, and this repository uses that shape."""
        body = braced_body('["a"] {\n  g = "**/*.rs"\n  x = 1\n}\n["b"] { y = 2 }', '["a"]')
        self.assertIn("x = 1", body)
        self.assertNotIn('["b"]', body)

    def test_a_closing_brace_in_a_comment_does_not_end_the_block(self):
        body = braced_body('["a"] {\n  // prose mentioning a } brace\n  x = 1\n}', '["a"]')
        self.assertIn("x = 1", body)

    def test_a_brace_in_a_quoted_value_does_not_close_the_block(self):
        """Unbalanced on purpose. Balanced braces in a value hold nothing:
        a scanner that counted them would return the same body."""
        body = braced_body('["a"] {\n  c = "run < }"\n  x = 1\n}', '["a"]')
        self.assertIn("x = 1", body)

    def test_a_comment_marker_inside_a_quoted_value_is_not_a_comment(self):
        body = braced_body('["a"] {\n  c = "https://example.invalid"\n  x = 1\n}', '["a"]')
        self.assertIn("x = 1", body)

    def test_an_escaped_quote_does_not_end_a_quoted_value(self):
        body = braced_body('["a"] {\n  c = "a \\" }"\n  x = 1\n}', '["a"]')
        self.assertIn("x = 1", body)

    def test_a_key_that_exists_only_in_a_comment_is_not_found(self):
        """The search runs over the mask too, not just the brace counting.

        Commenting a block out is how a step is disabled, so a commented one
        must not read as a declaration. When only the depth loop skipped
        comments, this returned the commented block's body and the end-to-end
        fixture could not object -- it installs whatever this returns.
        """
        # A live sibling follows, so a search over raw text does not merely
        # return None here -- it finds the commented key and then the *next*
        # real brace, and hands back a body belonging to something else.
        text = '["a"] {\n  // ["b"] { c = "commented" }\n  ["c"] { c = "other" }\n}'
        self.assertIsNone(braced_body(text, '["b"]', within=braced_body(text, '["a"]')))

    def test_a_commented_brace_between_a_key_and_its_block_is_not_the_opening(self):
        """The step lookup runs over the mask for the reason the hook lookup does."""
        # Asserted exactly: a raw lookup opens at the *commented* brace, and
        # the body it returns still holds `live` -- it just starts too early.
        text = '["a"] {\n  ["b"] /* { */ { c = "live" }\n}'
        body = braced_body(text, '["b"]', within=braced_body(text, '["a"]'))
        self.assertEqual(body, ' c = "live" ')

    def test_a_live_key_is_found_past_a_commented_copy_of_itself(self):
        """And the mask must not hide the real one behind the commented one."""
        text = '["a"] {\n  // ["b"] { c = "commented" }\n  ["b"] { c = "live" }\n}'
        body = braced_body(text, '["b"]', within=braced_body(text, '["a"]'))
        self.assertIn("live", body)
        self.assertNotIn("commented", body)

    def test_a_commented_check_is_not_the_declaration(self):
        """The check lookup, over a fragment the real `hk.pkl` cannot provide.

        Commenting a line out and writing its replacement below is the
        ordinary shape of a configuration edit, and this is the read that
        decides what the end-to-end fixture installs -- so a `check` taken
        from a comment is a command that fixture runs while hk runs another.
        """
        step = (
            '                // check = "mise run --quiet commits:message < {{f}}"\n'
            '                check = "true"\n'
        )
        self.assertEqual(declared_check(step), "true")

    def test_a_live_check_is_read_whole_past_a_commented_one(self):
        step = (
            '                // check = "the old command"\n'
            '                check = "mise run --quiet commits:message < {{f}}"\n'
        )
        self.assertEqual(declared_check(step), "mise run --quiet commits:message < {{f}}")

    def test_a_step_with_only_a_commented_check_declares_none(self):
        step = '                // check = "mise run --quiet commits:message < {{f}}"\n'
        with self.assertRaises(AssertionError):
            declared_check(step)

    def test_a_masked_match_can_be_sliced_back_out_of_the_source(self):
        """Why the caller locates on the mask and slices the source.

        The mask neutralises `{` and `}` inside a value, so reading a match's
        group off the mask returns a command with `{{commit_msg_file}}`
        blanked away. `declared_commit_msg_check` takes the offsets from the
        mask and the characters from the source for exactly this reason.
        """
        source = 'check = "run < {{f}}"'
        mask = masked_source(source)
        self.assertEqual(len(mask), len(source))
        found = re.search(r'check\s*=\s*"(.*?)"\s*$', mask)
        masked = mask[found.start(1) : found.end(1)]
        self.assertNotIn("{", masked)
        self.assertNotIn("}", masked)
        self.assertEqual(source[found.start(1) : found.end(1)], "run < {{f}}")

    def test_a_key_the_region_does_not_hold_is_absent(self):
        """What containment is for: a sibling block's step is not this one's."""
        text = '["a"] {\n  ["step"] { x = 1 }\n}\n["b"] {\n  ["other"] { y = 2 }\n}'
        self.assertIsNone(braced_body(text, '["other"]', within=braced_body(text, '["a"]')))
        self.assertIsNotNone(braced_body(text, '["step"]', within=braced_body(text, '["a"]')))

    def test_an_unterminated_block_is_absent_rather_than_truncated(self):
        self.assertIsNone(braced_body('["a"] {\n  x = 1\n', '["a"]'))


class HkWouldRunTheStep(unittest.TestCase):
    """Whether hk runs the step, asked of hk rather than inferred from its file.

    Every read this suite makes out of `hk.pkl` is about *what* the
    `conventional-commit` step runs: the end-to-end fixture installs that
    command and drives a real merge through it. None of them says *whether* hk
    would run the step at all, and a step hk skips still hands that fixture a
    command to install, run and pass on while the real hook runs nothing.

    Inferring the answer from the file's text does not work, and the reason is
    structural rather than a matter of covering more keys. Pkl merges object
    entries that share a key and hk applies the merged result, so a second
    `["conventional-commit"]` entry carrying `step_condition = "false"`, or a
    second `["commit-msg"]` entry beside this hook's, is authoritative to hk
    and invisible to any scan that resolves a key to its first occurrence. hk
    also reads settings that are not in the step at all -- module-level
    `skip_steps` and `skip_hooks` -- which no reading of a step can see.

    So hk is asked, twice, because one question does not answer the other.
    `hk run commit-msg --plan --json` reports per step whether this hook would
    include it, which is where a skipped step, a renamed one, a step moved to
    another hook and both merged-entry shapes surface. `hk config dump`
    reports the settings hk merged from every source, which is where a skipped
    *hook* surfaces -- under `skip_hooks = List("commit-msg")` the plan still
    reports this step `included` while `hk run commit-msg` exits 0 having run
    nothing.
    """

    def test_hk_plans_to_run_the_conventional_commit_step(self):
        """hk's own answer for this repository, over this repository's file."""
        self.assertEqual(
            planned_status(commit_msg_plan(), "conventional-commit"),
            "included",
            "hk does not plan to run the `conventional-commit` step of the "
            "`commit-msg` hook, so the command the fixtures read out of "
            "`hk.pkl` is one no commit would run",
        )

    def test_hk_skips_no_hook_named_commit_msg(self):
        """The half of the same question the plan cannot see."""
        self.assertNotIn(
            "commit-msg",
            skipped_hooks(hk_answer("config", "dump")),
            "hk's effective configuration skips the `commit-msg` hook, so no "
            "step declared under it runs whatever the plan for it says",
        )

    def test_a_step_hk_plans_to_skip_is_reported_as_skipped(self):
        """Invisible from this repository's own `hk.pkl`, which is not disarmed.

        The document below is hk 1.53.0's, emitted in a throwaway export with
        `step_condition = "false"` inserted into the real step: `hk validate`
        green, the `check` line untouched, and `hk run commit-msg` exit 0 over
        a merge subject having run no step.
        """
        skipped = {
            "hook": "commit-msg",
            "runType": "check",
            "steps": [
                {
                    "name": "conventional-commit",
                    "status": "skipped",
                    "orderIndex": 0,
                    "reasons": [
                        {
                            "kind": "condition_false",
                            "detail": "step_condition evaluated to false: false",
                        }
                    ],
                    "fileCount": 0,
                }
            ],
        }
        self.assertEqual(planned_status(skipped, "conventional-commit"), "skipped")

    def test_a_plan_naming_no_such_step_is_an_error(self):
        """Deleting the step, moving it out of the hook, or renaming it.

        Measured on hk 1.53.0: with the step deleted, and again with it moved
        under `pre-push`, the plan for `commit-msg` is `"steps": []`; renamed,
        it carries the new name alone. Reading a status off whichever step is
        present would report `included` for the renamed case, which is the one
        shape here that has a step to read.
        """
        for plan in (
            {"hook": "commit-msg", "steps": []},
            {
                "hook": "commit-msg",
                "steps": [{"name": "conventional-commit-renamed", "status": "included"}],
            },
        ):
            with self.assertRaises(AssertionError):
                planned_status(plan, "conventional-commit")

    def test_a_configuration_carrying_no_skip_hooks_is_an_error(self):
        """A renamed setting must not read as "nothing is skipped".

        The safe answer to this question is an empty list, so a shape the
        setting cannot be found in is the one shape that would pass silently
        for as long as it lasted.
        """
        with self.assertRaises(AssertionError):
            skipped_hooks({"skip_steps": []})


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
