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

Which commits it checks is the command line's (`USAGE`):

- no argument: every commit `HEAD` reaches that no remote-tracking branch does,
  what `git push` of the current branch would publish;
- `range OLD NEW`: what `NEW` has that `OLD` has not, as CI's `Per-commit
  gates` job passes a pull request's base and head or a push's before and
  after (`OLD` is fetched when absent; see `between`);
- `pre-push REMOTE URL`: the refs git names on stdin, as the hook passes them
  (`pushed`), so `git push origin other-branch` checks `other-branch` and not
  `HEAD`;
- `install`: writes that hook; `mise run hooks:install` runs it;
- `check-install`: warns, never failing, where git would not run that hook
  (`hook_warning`); `mise run check` runs it as `hooks:check`.
"""

import fnmatch
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

USAGE = "usage: commit_gates.py [range OLD NEW | pre-push REMOTE URL | install | check-install]"
#: What a push publishes from `HEAD`: nothing a remote already has.
DEFAULT_RANGE = ("HEAD", "--not", "--remotes")
#: A full object name, SHA-1 or SHA-256; git spells "no object" as all zeros.
OBJECT_NAME = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
#: What marks the `pre-push` hook as `install`'s, so a rerun may replace it.
HOOK_MARKER = "# Installed by `mise run hooks:install`"
HOOK = f"""#!/bin/sh
{HOOK_MARKER}: every commit the push publishes,
# checked out on its own through the per-commit gates (scripts/commit_gates.py).
exec mise run --quiet commits:gates -- pre-push "$@"
"""

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


def is_null(name):
    """Whether `name` is git's all-zero object name: no commit at all."""
    return set(name) == {"0"}


def present(root, revision, environment):
    """Whether `revision` names a commit this repository holds."""
    probe = subprocess.run(
        ["git", "-C", str(root), "cat-file", "-e", f"{revision}^{{commit}}"],
        env=environment,
        capture_output=True,
    )
    return probe.returncode == 0


def pushed(lines, held):
    """The `git log` specs a push publishes, one per ref it updates.

    `lines` is the pre-push hook's stdin, `<local ref> <local sha> <remote ref>
    <remote sha>` per ref, and `held(sha)` says whether a commit is here. A
    delete (an all-zero local sha) publishes nothing. An update publishes what
    neither the remote commit nor any remote-tracking branch has, so master
    commits merged in, which the ref's remote commit never reached, are not
    gated again; a new ref, or one whose remote commit is not here (a forced
    push over commits never fetched), publishes what no remote-tracking branch
    has, the same as the default range.
    """
    specs = []
    for line in lines:
        fields = line.split()
        if not fields:
            continue
        if len(fields) != 4:
            raise SystemExit(f"commit_gates: `{line}` is not a pre-push ref line")
        _, local, _, remote = fields
        if is_null(local):
            continue
        if is_null(remote) or not held(remote):
            specs.append([local, "--not", "--remotes"])
        else:
            specs.append([local, "--not", remote, "--remotes"])
    return specs


def between(old, new, held, fetch):
    """The `git log` spec for what `new` has that `old` has not.

    `old` is what CI's event names as already checked: a pull request's base,
    or the commit a push replaced. A push's may be all zeros, where the ref is
    new, or missing from the checkout, where the push was forced over it; a
    missing object name is fetched once with `fetch(old)`, since a host keeps a
    replaced commit for a while. Where neither yields it, only `new`'s own tip
    is checked, rather than nothing or every commit `new` reaches, and a notice
    says so.
    """
    if not is_null(old):
        if held(old):
            return [f"{old}..{new}"]
        if not OBJECT_NAME.fullmatch(old):
            # A revision typed wrong, not a commit a push replaced.
            raise SystemExit(f"commit_gates: `{old}` names no commit here")
        if fetch(old) and held(old):
            return [f"{old}..{new}"]
    print(f"commit_gates: `{old}` is not here, so only `{new}` itself is checked", flush=True)
    return [f"{new}^!"]


def commits(root, specs, environment):
    """`(sha, subject)` for each non-merge commit in any of `specs`, oldest
    first within each, and each commit once."""
    series, seen = [], set()
    for spec in specs:
        arguments = ("log", "--reverse", "--no-merges", "--format=%H%x09%s", *spec, "--")
        for line in git(root, *arguments, environment=environment).splitlines():
            sha, subject = line.split("\t", 1)
            if sha not in seen:
                seen.add(sha)
                series.append((sha, subject))
    return series


def install(root, environment):
    """Write the `pre-push` hook, refusing to replace one it did not write.

    Where git reads hooks, `core.hooksPath` included. Not left to hk: hk's own
    `pre-push` reads the pushed refs itself and hands its steps none of them.
    """
    path = git(root, "rev-parse", "--git-path", "hooks/pre-push", environment=environment)
    hook = root / path.strip()
    if hook.exists() and HOOK_MARKER not in hook.read_text(errors="replace"):
        raise SystemExit(f"commit_gates: {hook} is not this repository's hook; move it aside first")
    hook.parent.mkdir(parents=True, exist_ok=True)
    hook.write_text(HOOK)
    hook.chmod(0o755)
    print(f"commit_gates: installed {hook}")


def hook_warning(root, environment):
    """Why git would not run `install`'s `pre-push` hook in `root`, or `None`.

    Read where `install` writes it, so `core.hooksPath` and a linked worktree's
    common directory resolve alike. A hook missing, written by something else,
    or not executable each skips the per-commit gates on every push, silently:
    the first is what a checkout whose hooks predate `install` has.
    """
    path = git(root, "rev-parse", "--git-path", "hooks/pre-push", environment=environment)
    hook = root / path.strip()
    if not hook.is_file():
        return (
            f"commit_gates: no pre-push hook at {hook}, so a push skips the per-commit "
            "gates; run `mise run hooks:install`"
        )
    if HOOK_MARKER not in hook.read_text(errors="replace"):
        return (
            f"commit_gates: {hook} is not this repository's pre-push hook, so a push "
            "skips the per-commit gates; move it aside and run `mise run hooks:install`"
        )
    if not os.access(hook, os.X_OK):
        return (
            f"commit_gates: {hook} is not executable, so git skips it and a push skips "
            "the per-commit gates; run `mise run hooks:install`"
        )
    return None


def check_install(environment):
    """Print `hook_warning` for the repository the working directory is in.

    Silent under CI, whose checkout has no hooks to install, and outside a
    repository, as a source tarball is. Advisory: the gates it warns about are
    CI's backstop too, so a missing hook is no reason to fail `check`.
    """
    if environment.get("CI", "").lower() not in ("", "0", "false"):
        return
    probe = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], env=environment, capture_output=True, text=True
    )
    if probe.returncode != 0:
        return
    warning = hook_warning(Path(probe.stdout.strip()), scrubbed(environment))
    if warning:
        print(warning, file=sys.stderr)


def selection(root, arguments, stdin, environment):
    """The `git log` specs `arguments` name, per `USAGE`."""

    def held(revision):
        return present(root, revision, environment)

    def fetch(sha):
        command = ["git", "-C", str(root), "fetch", "--quiet", "--no-tags", "origin", sha]
        return subprocess.run(command, env=environment).returncode == 0

    match arguments:
        case []:
            return [list(DEFAULT_RANGE)]
        case ["range", old, new] if old.strip() and new.strip():
            return [between(old, new, held, fetch)]
        case ["pre-push", _remote, _url]:
            return pushed(stdin.read().splitlines(), held)
        case _:
            raise SystemExit(USAGE)


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
    if sys.argv[1:] == ["check-install"]:
        check_install(os.environ)
        return 0
    environment = scrubbed(os.environ)
    # Resolved under the inherited environment, which is what names the
    # repository from a hook; everything after addresses it by path.
    root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
        ).stdout.strip()
    )
    arguments = sys.argv[1:]
    if arguments == ["install"]:
        install(root, environment)
        return 0
    specs = selection(root, arguments, sys.stdin, environment)
    series = commits(root, specs, environment)
    if not series:
        named = "; ".join(" ".join(spec) for spec in specs) or "the push"
        print(f"commit_gates: no commit in `{named}` to check")
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
