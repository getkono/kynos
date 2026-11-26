"""Per-commit gates: each commit in a range, checked out on its own, through
the gates CI otherwise runs only at a branch's tip.

A series lands commit by commit, so a commit that is red mid-series stays red
in history even once a later one fixes it. `pre-commit` cannot catch the gates
named here: it is kept fast, and it runs over the working tree rather than over
the commit, which holds untracked files whenever hk's stash leaves them behind
(`hk.pkl`). Each commit is checked out instead into one detached worktree under
`target/`, so a gate sees exactly the tree the commit records.

A gate runs on a commit only where the commit changes one of its inputs, since
the commit before it already passed it over the same inputs. Merge commits are
not checked; the tip a merge produces is what CI's own jobs check. `test` is not
a gate here: a bug fix's red-test commit fails it by design (CONTRIBUTING.md).

The range is `KYNOS_COMMIT_RANGE`, as `git log` reads it, or by default every
commit `HEAD` reaches that no remote-tracking branch does -- what a push is
about to publish. Run as `mise run commits:gates`; the `pre-push` hook and CI's
`Per-commit gates` job run it.
"""

import fnmatch
import os
import shutil
import subprocess
import sys
from pathlib import Path

RANGE_VARIABLE = "KYNOS_COMMIT_RANGE"
#: What a push publishes from `HEAD`: nothing a remote already has.
DEFAULT_RANGE = ("HEAD", "--not", "--remotes")

#: What every Cargo build reads, and the task definitions and tool pins behind
#: each gate. Over-inclusion only costs a run; omission passes a red commit.
#: `fnmatch`'s `*` crosses `/`, so `crates/*` is everything under `crates/`.
BUILD_INPUTS = (
    "crates/*",
    "Cargo.toml",
    ".cargo/config.toml",
    "mise.toml",
    "mise.lock",
)

#: Each gate, as the `mise run` task it is, with the paths it reads. In the
#: order they run: Clippy compiles first, since a commit that does not build
#: fails every gate after it for the same reason.
GATES = (
    ("lint", BUILD_INPUTS + ("Cargo.lock", "clippy.toml")),
    ("docs:check", BUILD_INPUTS + ("Cargo.lock",)),
    ("api:check", BUILD_INPUTS + ("Cargo.lock", "docs/architecture.md", "scripts/public_api.py")),
    # The consumer it builds resolves afresh, so the lockfile is no input.
    ("msrv:check", BUILD_INPUTS),
)


def selected(paths, gates=GATES):
    """The names of `gates` whose inputs any of `paths` matches, in order."""
    return [
        name
        for name, inputs in gates
        if any(fnmatch.fnmatchcase(path, glob) for path in paths for glob in inputs)
    ]


def scrubbed(environment):
    """`environment` with every `GIT_*` name removed.

    A hook runs under git's own environment, which names the repository the
    hook was invoked from; inherited by a command run inside the worktree, it
    would point that command back at the checkout rather than at the commit.
    """
    return {key: value for key, value in environment.items() if not key.startswith("GIT_")}


def git(root, *args, environment):
    """`git -C root args`, its stdout, raising with git's own words on failure."""
    run = subprocess.run(
        ["git", "-C", str(root), *args], env=environment, capture_output=True, text=True
    )
    if run.returncode != 0:
        raise SystemExit(f"commit_gates: `git {' '.join(args)}` failed:\n{run.stderr}")
    return run.stdout


def revision_range(environment):
    """The `git log` arguments naming the commits to check.

    An exported but empty `KYNOS_COMMIT_RANGE` is refused rather than read as
    the default: in CI that is a workflow expression gone wrong, and the default
    there names no commit, which would pass having checked nothing.
    """
    spelled = environment.get(RANGE_VARIABLE)
    if spelled is None:
        return list(DEFAULT_RANGE)
    if not spelled.split():
        raise SystemExit(f"commit_gates: {RANGE_VARIABLE} is set and names no range")
    return spelled.split()


def commits(root, spec, environment):
    """`(sha, subject)` for each non-merge commit in `spec`, oldest first."""
    arguments = ("log", "--reverse", "--no-merges", "--format=%H%x09%s", *spec, "--")
    listed = git(root, *arguments, environment=environment)
    return [tuple(line.split("\t", 1)) for line in listed.splitlines() if line]


def changed(root, sha, environment):
    """The paths `sha` changes against its parent, or adds as a root commit."""
    arguments = ("diff-tree", "--root", "--no-commit-id", "--name-only", "-r", "-z", sha)
    listed = git(root, *arguments, environment=environment)
    return [path for path in listed.split("\0") if path]


def gate_environment(root, tree, environment):
    """What each gate runs under inside `tree`.

    Cargo shares the checkout's target directory unless one is exported, so the
    dependencies one commit built serve the next. mise is stopped at `tree`, so
    the checkout's own `mise.toml` above it does not merge into the commit's,
    and trusts `tree`, whose configuration is the commit's own.
    """
    gated = scrubbed(environment)
    gated.setdefault("CARGO_TARGET_DIR", str(root / "target"))
    gated["MISE_CEILING_PATHS"] = str(tree.parent)
    trusted = [str(tree), *filter(None, gated.get("MISE_TRUSTED_CONFIG_PATHS", "").split(":"))]
    gated["MISE_TRUSTED_CONFIG_PATHS"] = ":".join(trusted)
    return gated


def remove_tree(root, tree, environment):
    """Drop `tree` and its registration, whatever a previous run left."""
    subprocess.run(
        ["git", "-C", str(root), "worktree", "remove", "--force", str(tree)],
        env=environment,
        capture_output=True,
    )
    shutil.rmtree(tree, ignore_errors=True)
    git(root, "worktree", "prune", environment=environment)


def main():
    environment = scrubbed(os.environ)
    # Resolved under the inherited environment, which is what names the
    # repository from a hook; everything after addresses it by path.
    root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
        ).stdout.strip()
    )
    spec = revision_range(os.environ)
    series = commits(root, spec, environment)
    if not series:
        print(f"commit_gates: no commit in `{' '.join(spec)}` to check")
        return 0

    tree = root / "target" / "commit-gates" / "tree"
    remove_tree(root, tree, environment)
    tree.parent.mkdir(parents=True, exist_ok=True)
    git(root, "worktree", "add", "--detach", "--quiet", str(tree), "HEAD", environment=environment)
    gated = gate_environment(root, tree, environment)
    try:
        for sha, subject in series:
            gates = selected(changed(root, sha, environment))
            label = f"{sha[:12]} {subject}"
            if not gates:
                print(f"commit_gates: {label}: changes no gate's inputs", flush=True)
                continue
            git(tree, "checkout", "--detach", "--quiet", "--force", sha, environment=environment)
            git(tree, "clean", "-ffdq", environment=environment)
            for gate in gates:
                print(f"commit_gates: {label}: {gate}", flush=True)
                if subprocess.run(["mise", "run", gate], cwd=tree, env=gated).returncode != 0:
                    print(
                        f"commit_gates: {label} fails `{gate}`. Reproduce it with "
                        f"`git switch --detach {sha[:12]} && mise run {gate}`; history "
                        "that is not yet pushed can still be fixed up",
                        file=sys.stderr,
                    )
                    return 1
    finally:
        remove_tree(root, tree, environment)
    print(f"commit_gates: {len(series)} commit(s) pass their gates")
    return 0


if __name__ == "__main__":
    sys.exit(main())
