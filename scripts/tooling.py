"""Tooling checks: what the repository's own tooling is configured to do.

Two rule sets over files no other gate reads: `.cargo/config.toml`, which
cargo reads on every invocation and checks only by warning, and the Python
floor `scripts/` is written to, with the `python` pins in `mise.toml` that make
it true of what runs. Neither reads Rust or a table in `docs/`, which is what
`containment.py` is for.
"""

import ast
import re
import sys
import tomllib
from pathlib import Path

# Relative to the script, so a run from any directory checks the same tree.
ROOT = Path(__file__).resolve().parent.parent

# --- The dev build profile ---------------------------------------------------
# `.cargo/config.toml` holds the dev-profile keys behind `nfr.md`'s Workspace
# footprint. Cargo only warns on a misspelt key and ignores a misspelt table, so
# key presence is checked here; values are left to review.
CARGO_CONFIG = ROOT / ".cargo/config.toml"
# Each key as a path through the parsed document, with its display spelling.
FOOTPRINT_KEYS = [
    (("profile", "dev", "debug"), "profile.dev.debug"),
    (("profile", "dev", "package", "*", "debug"), 'profile.dev.package."*".debug'),
]
# The only top-level tables allowed; tool settings belong in a mise task's `env`.
CONFIG_TABLES = {"profile"}


def declares(config, key):
    """Whether the parsed `config` declares the whole of `key`."""
    node = config
    for part in key:
        if not isinstance(node, dict) or part not in node:
            return False
        node = node[part]
    return True


def cargo_config_failures(text):
    """What is wrong with `.cargo/config.toml`, whose contents are `text`.

    `text` is `None` for a file that is not there. Unparseable TOML is
    reported rather than raised.
    """
    if text is None:
        return [
            ".cargo/config.toml is gone, and it is the whole of the dev build "
            "profile: without it a worktree's `target/` returns to roughly four "
            "times the size nfr.md's Workspace table records, with every other "
            "gate here green. Restore it, or move that row in the same commit"
        ]
    try:
        config = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        return [f".cargo/config.toml is not readable TOML: {error}"]

    problems = [
        f".cargo/config.toml no longer declares `{spelling}`, which is one of "
        "the two keys the dev build footprint nfr.md's Workspace table records "
        "rests on. Cargo will not say so: an unused key is a warning over a "
        "build that exits zero, and a misspelled `[profile.<name>]` table is "
        "not reported at all. Restore the key, or move that row in the same "
        "commit"
        for key, spelling in FOOTPRINT_KEYS
        if not declares(config, key)
    ]

    if foreign := sorted(set(config) - CONFIG_TABLES):
        problems.append(
            "`.cargo/config.toml` declares something other than the dev build "
            "profile it exists for. Cargo reads this file on every invocation "
            "under the repository, so what is written here runs on every "
            "machine and in every CI job and no gate reports it -- which is "
            "why the file is held to one table rather than reviewed. Put the "
            "setting in the `env` of the mise.toml task that needs it, or "
            "widen this rule and say in the same commit what the table is "
            "for:\n    " + "\n    ".join(foreign)
        )
    return problems


# --- The Python floor --------------------------------------------------------
# The grammar `scripts/` is held to, matched by every script task's `python`
# pin in mise.toml (`python_pin_failures`). `import tomllib` at the head of this
# file needs 3.11 whatever the grammar allows; that runtime floor is held only
# by running the gate under the pin, so raise this by hand with the stdlib.
PYTHON_FLOOR = (3, 11)
SCRIPTS = ROOT / "scripts"
MISE_CONFIG = ROOT / "mise.toml"


def refused_at(source, path, version):
    """The `SyntaxError` parsing `source` at `version` raises, or `None`."""
    try:
        ast.parse(source, filename=path, feature_version=version)
    except SyntaxError as error:
        return error
    return None


def python_floor_failures(scripts, floor=PYTHON_FLOOR):
    """Which of `scripts` stopped parsing at `floor`, and what each one needs.

    `scripts` is `(path, source)` pairs. A failing file is reported with the
    first minor version up to the running interpreter's that parses it, or as
    unparseable at all of them.
    """
    stated, running = f"3.{floor[1]}", f"3.{sys.version_info.minor}"
    problems = []
    for path, source in scripts:
        refused = refused_at(source, path, floor)
        if refused is None:
            continue
        needs = next(
            (
                f"3.{minor}"
                for minor in range(floor[1] + 1, sys.version_info.minor + 1)
                if refused_at(source, path, (3, minor)) is None
            ),
            None,
        )
        stops = f"line {refused.lineno} is where it stops: {refused.msg}"
        if needs is None:
            problems.append(
                f"{path} does not parse at Python {stated}, which is the floor "
                "scripts/ is held to, and it parses at no version up to this "
                f"interpreter's own ({running}) either -- {stops}. Either the "
                "file is broken, or it is written in grammar newer than the "
                "interpreter running this gate, which the `python` pin on the "
                f"script tasks in mise.toml fixes at {stated}. Repair the line, "
                "or raise the pin and this floor in the same commit"
            )
        else:
            problems.append(
                f"{path} does not parse at Python {stated}, which is the floor "
                f"scripts/ is held to: it needs {needs} -- {stops}. Every task "
                f"that runs this file pins {stated}, so the interpreter CI reads "
                "it under is not the one that wrote it. Write the line in "
                f"grammar {stated} accepts, or raise the pin on the script tasks "
                "in mise.toml and this floor in the same commit"
            )
    return problems


def python_pin_failures(text, scripts, floor=PYTHON_FLOOR):
    """What `mise.toml`'s `python` pins do not hold, `text` being that file.

    Every task running a `scripts/` file must pin one `python` equal to
    `floor`; no task may run inline `python3`. `scripts` is the
    paths (relative to `scripts/`) the floor rule read, so a task running a
    file outside that set fails. A script no task runs is allowed. Unparseable
    TOML is reported rather than raised.
    """
    try:
        config = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        return [f"mise.toml is not readable TOML: {error}"]

    stated = f"3.{floor[1]}"
    problems, run_by, pinned, unpinned, inline = [], {}, {}, [], []
    for name, task in sorted(config.get("tasks", {}).items()):
        run = task.get("run")
        body = "\n".join(
            part
            for part in ([run] if isinstance(run, str) else run or [])
            if isinstance(part, str)
        )
        named = set(re.findall(r"scripts/(\S+\.py)", body))
        pin = task.get("tools", {}).get("python")
        for script in named:
            run_by.setdefault(script, []).append(name)
        if named and pin is None:
            unpinned.append(name)
        elif named:
            pinned.setdefault(pin, []).append(name)
        elif pin is not None:
            problems.append(
                f"the mise task `{name}` pins a `python` and runs no file from "
                "scripts/, so the pin installs an interpreter for a task that "
                "opens none and repeats a floor it does not stand under. Drop the "
                "pin, or give the task the script it was added for"
            )
        elif "python3" in body:
            inline.append(name)

    if unpinned:
        problems.append(
            "a mise task runs a file from scripts/ and pins no `python`, so what "
            "it runs is whatever interpreter the machine has rather than the "
            f"{stated} floor tooling.py declares. The pin is the only half of "
            "that number that is true of anything anybody runs; add "
            "`tools = { python = ... }` at the version its siblings carry:\n    "
            + "\n    ".join(unpinned)
        )

    if len(pinned) > 1:
        problems.append(
            "the mise tasks that run a file from scripts/ pin more than one "
            "`python`, so which interpreter the floor names depends on which task "
            "a reader opened. Raising the floor is one commit moving every pin and "
            "`PYTHON_FLOOR` together, and this is what reports that it was not "
            "one commit:\n    "
            + "\n    ".join(
                f"{version}: " + ", ".join(sorted(names))
                for version, names in sorted(pinned.items())
            )
        )

    for version, names in sorted(pinned.items()):
        read = re.match(r"(\d+)\.(\d+)", version)
        if read is None:
            problems.append(
                f'mise.toml pins `python = "{version}"` for the tasks that run '
                "scripts/ and nothing here can read a minor version out of it, so "
                "the pin cannot be compared against the floor it is half of. Pin a "
                f"version, at {stated}:\n    " + "\n    ".join(sorted(names))
            )
        elif (int(read[1]), int(read[2])) != floor:
            problems.append(
                f"mise.toml pins python {version} for the tasks that run scripts/ "
                f"and tooling.py declares a floor of {stated}. These are one "
                "number written twice: a floor nothing runs at is a claim nobody "
                "keeps, and an interpreter nothing declares is what the floor rule "
                "exists to refuse. Move both in the same commit:\n    "
                + "\n    ".join(sorted(names))
            )

    if unread := sorted(set(run_by) - set(scripts)):
        problems.append(
            "a mise task runs a file under scripts/ that the floor rule never "
            "read, so a file something runs is held to no grammar at all. Either "
            "the task names a file that is not there, or the floor rule's own file "
            "set has narrowed and this is the only thing that says so:\n    "
            + "\n    ".join(
                f"{script}: run by " + ", ".join(sorted(run_by[script]))
                for script in unread
            )
        )

    if inline:
        problems.append(
            "a mise task runs `python3` over something other than a file in "
            "scripts/, so it is held to no declared floor and nothing here can say "
            "what it should be pinned to. Move the code into scripts/ where the "
            "floor rule reads it:\n    " + "\n    ".join(inline)
        )

    return problems


def main(cargo_config=None, scripts=None, mise=None):
    """Run every rule over this repository, and report what does not hold.

    Rules run here, never at import. Each argument defaults to this
    repository's own file, read when `main` runs, and may be replaced by a
    test: `cargo_config` and `mise` as text, `scripts` as `(path, source)`
    pairs with `path` relative to the repository root.
    """
    if cargo_config is None and CARGO_CONFIG.is_file():
        cargo_config = CARGO_CONFIG.read_text()
    if scripts is None:
        scripts = [
            (path.relative_to(ROOT).as_posix(), path.read_text())
            for path in sorted(SCRIPTS.glob("*.py"))
        ]
    mise = MISE_CONFIG.read_text() if mise is None else mise

    failures = cargo_config_failures(cargo_config)
    failures += python_floor_failures(scripts)
    failures += python_pin_failures(
        mise, {path.removeprefix("scripts/") for path, _ in scripts}
    )

    for failure in failures:
        print(f"tooling: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(f"tooling: {len(scripts)} scripts, every rule holds")
    return 0


# Importing runs no rule; `tooling_test.py` relies on that.
if __name__ == "__main__":
    sys.exit(main())
