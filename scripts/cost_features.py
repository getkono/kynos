"""Per-feature cost sweep: what each feature costs a linked artifact and in IR.

[`performance.md`](../docs/performance.md)'s taxonomy files two sweep kinds --
a binary delta over `.text` and a codegen delta over monomorphized IR -- and
says of both that they "build the same fixture at each feature and compare
artifacts". This is the driver for that, and `crates/kynos/cost/fixture.rs` is
the fixture. Because that fixture never *uses* an optional feature, each row
answers one question: what does merely enabling F cost a program that does not
use F? `lib.rs` claims enabling a feature is additive, and this is the first
thing in the repository that could falsify it.

The two halves run under different profiles, on purpose. The binary half is
`--release`, because that is the artifact that ships and `lto = "fat",
codegen-units = 1` gives a build with no cross-CGU nondeterminism. The codegen
half runs the dev profile, because `cargo llvm-lines` reads pre-link IR and a
fat-LTO release build deletes the very monomorphizations the codegen delta
exists to count.

One limit of the codegen half is worth stating rather than discovering:
`cargo llvm-lines --example` reports the IR of the *example* crate, so it sees
framework generics as the fixture instantiates them rather than generic-free
library code compiled into the rlib. That is the intended scope --
`performance.md` says both kinds build the same fixture.

It also means the sign can invert, which a subset limitation alone would not
predict, so read a negative row carefully. rustc shares generic instantiations
out of upstream rlibs, so a feature that enlarges the dependency graph can move
instantiations off the example crate and *reduce* this number without deleting
any work at all: **a negative codegen row is a relocation, not a saving**.
`test-util` is the worked example. It gates one module the fixture never names
and still reports -3982 lines, because `core::str::pattern::simd_contains` and
`hashbrown`'s resize paths stop being instantiated here and start being
instantiated upstream -- neither is Kynos code and neither stopped existing.

Measuring the whole graph instead would need `-Z share-generics=off`, which is
nightly-only and out of scope here. The `.text` half is the number without this
confound, which is one reason both halves exist.

`cargo hack` is deliberately not the driver, though the feature list copies
`features:targets`' shape exactly. cargo-hack has no hook between builds and
every build overwrites `target/release/examples/cost_fixture`, so a sweep it
drove could not attribute an artifact to the feature that produced it: it would
measure the last build once per point. Driving the loop here also means nothing
in this task rewrites a member manifest, which is why it may share a job with
neither `features:check` nor anything that follows it.

A script rather than a shell pipeline for `containment.py`'s reason: this
parses two tools' output, diffs two committed tables and emits Markdown, and
every one of those is where a pipeline gets it quietly wrong.

Exit codes follow the `semver` CI job's rule. Zero whenever a measurement was
made, whatever it says -- no threshold is applied here and none is recorded
anywhere, per [`nfr.md`](../docs/nfr.md#thresholds), which sets a ceiling from a
first recorded measurement and never guesses one. Non-zero only when a
measurement could not be made at all: a build that did not compile, a missing
`llvm-size`, an ambient `RUSTFLAGS`, an output with no `(TOTAL)` in it.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# From the script's own location rather than the working directory, so running
# it by hand from a crate directory sweeps the same tree mise does.
ROOT = Path(__file__).resolve().parent.parent
COST = ROOT / "crates/kynos/cost"

# `features:targets`' exclusions, for its reasons: `server` and `tls` need an
# HTTP protocol, `time` and `decimal` are umbrellas that do not compile alone,
# and `full` would pull in everything and sweep nothing partial. `openapi31` is
# excluded on top of those because it is the pinned baseline every other point
# is measured against, so a point for it alone is the baseline row.
EXCLUDED = frozenset(
    {"default", "server", "tls", "time", "decimal", "full", "openapi31"}
)
BASELINE = "openapi31"
# Parenthesised the way `cargo llvm-lines` parenthesises `(TOTAL)`, so a reader
# of the table cannot mistake it for a feature name.
ALL_FEATURES = "(all-features)"
TOP = 5

# `cargo llvm-lines` prints its total as two bare integers and every other row
# with a share and a running share beside each -- `1608 (3.2%,  3.2%)     1
# (0.1%,  0.1%)  <matchit::tree::Node<usize>>::insert` -- so both parentheses
# are optional here. A name may hold spaces, commas and angle brackets, which
# is why it is taken as the rest of the line rather than as a field.
LLVM_LINES_ROW = re.compile(
    r"^\s*(\d+)(?:\s*\([^)]*\))?\s+(\d+)(?:\s*\([^)]*\))?\s+(\S.*?)\s*$"
)
# `kynos[a28856ce07048254]::router::describe` -- the bracketed part is the
# stable crate id, which is a hash of the crate's *enabled features* among
# other things. It therefore differs between two points of this very sweep, so
# leaving it in would make every function look new at every feature and
# attribution would report nothing but noise.
#
# Bounded two ways, because unbounded it also eats a slice of a float type:
# `[f64]` is entirely hex digits, so `<[f64] as core::fmt::Debug>::fmt` and its
# `f32` twin would both collapse to `< as core::fmt::Debug>::fmt` and one would
# silently overwrite the other in the table.
#
# The lookbehind is the sharp half: a crate id always follows the crate's name,
# and a slice bracket never does -- it follows `<`, `&`, `(` or a comma. The
# length bound is the blunt half. It is a range rather than exactly sixteen
# because a `StableCrateId` is a 64-bit value printed without leading-zero
# padding, so it is *usually* sixteen digits and sometimes fewer: one sweep's
# output holds 9592 of sixteen digits and 2332 of fifteen. Pinning it at
# sixteen would leave every `alloc` and `hashbrown` id in place, which is the
# noise the stripping exists to remove.
DISAMBIGUATOR = re.compile(r"(?<=\w)\[[0-9a-f]{8,16}\]")

BINARY_TSV = "binary.tsv"
BINARY_HEADER = """\
# Binary delta. `.text` of the `cost_fixture` example, built as
#   cargo build -p kynos --release --example cost_fixture \\
#     --no-default-features --features openapi31[,<feature>]
# Written by `mise run cost:record`; compared by `mise run cost:features`.
#
# The compared column is `delta`: the difference against the openapi31 build in
# the same run, never the absolute. `performance.md#thresholds` is the reason --
# relations outlive absolutes, and an absolute moves on a toolchain bump that
# changed nothing about Kynos. `text` is recorded beside it as context and is
# not compared.
#
# No ceiling is set here or anywhere else. `nfr.md#thresholds` sets one from a
# first recorded measurement, reviewed as a change to that document; this file
# is that first measurement, not the ceiling.
#
# toolchain: {toolchain}
# host: {host}
# baseline: {baseline} bytes
"""

CODEGEN_TSV = "codegen.tsv"
CODEGEN_HEADER = """\
# Codegen delta. Monomorphized LLVM IR for the `cost_fixture` example, counted
# as
#   cargo llvm-lines --package kynos --example cost_fixture --color never \\
#     --sort lines --no-default-features --features openapi31[,<feature>]
# Written by `mise run cost:record`; compared by `mise run cost:features`.
#
# The dev profile, not `--release`, and that is not an oversight: `llvm-lines`
# reads pre-link IR, and a fat-LTO release build deletes the monomorphizations
# this exists to count.
#
# The compared columns are the two deltas, for `binary.tsv`'s reason. Per-
# function attribution is deliberately not recorded here: monomorphized names
# churn with every generic signature and a file of them would be a diff
# generator rather than a baseline. Attribution is report-only, in
# `cost-report.md`.
#
# No ceiling is set here or anywhere else -- see `nfr.md#thresholds`.
#
# toolchain: {toolchain}
# host: {host}
# baseline: {baseline} lines
"""


def fail(message, code=1):
    """Report that a measurement could not be made, and stop."""
    print(f"cost: {message}", file=sys.stderr)
    sys.exit(code)


def capture(command, env=None):
    """Run `command`, returning its stdout; a non-zero exit is a failure."""
    printed = " ".join(str(part) for part in command)
    try:
        result = subprocess.run(
            command, cwd=ROOT, env=env, capture_output=True, text=True, check=False
        )
    except FileNotFoundError:
        return fail(f"`{command[0]}` is not on PATH; run this through mise")
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        return fail(f"`{printed}` exited {result.returncode}")
    return result.stdout


def sweep_env():
    """The environment every build in the sweep runs under.

    An ambient `RUSTFLAGS` silently changes every number recorded here, and a
    trend built on that is noise rather than a trend, so it is refused rather
    than overridden -- overriding it would discard a flag the caller meant.
    """
    for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"):
        if os.environ.get(name):
            fail(
                f"{name} is set. It changes every number this sweep records "
                "without appearing in any of them. Unset it and run again."
            )
    return {**os.environ, "CARGO_INCREMENTAL": "0", "CARGO_TERM_COLOR": "never"}


def toolchain():
    """The rustc version string and host triple the numbers belong to."""
    report = capture(["rustc", "-vV"])
    host = next(
        line.removeprefix("host: ")
        for line in report.splitlines()
        if line.startswith("host: ")
    )
    return report.splitlines()[0].removeprefix("rustc "), host


def llvm_size(host):
    """`llvm-size` from the pinned toolchain, never the runner's binutils.

    Deliberately no fallback to GNU `size`. The two disagree about what they
    print, and switching instruments partway through a trend is exactly the
    noise a trend must not have, so an absent tool is a failure that names what
    to install.
    """
    sysroot = capture(["rustc", "--print", "sysroot"]).strip()
    path = Path(sysroot) / "lib" / "rustlib" / host / "bin" / "llvm-size"
    if not path.is_file():
        fail(
            f"{path} is missing. It ships with the `llvm-tools-preview` "
            "component, which `mise.toml` pins; `mise install` restores it. "
            "This sweep does not fall back to binutils `size`.",
            2,
        )
    return path


def points():
    """The feature sets swept, baseline first and `--all-features` last.

    Read off disk rather than restated here, for `publish:check`'s reason: a
    feature added to `Cargo.toml` is swept the day it is added rather than the
    day someone remembers this list.
    """
    metadata = json.loads(
        capture(["cargo", "metadata", "--no-deps", "--format-version", "1"])
    )
    package = next(p for p in metadata["packages"] if p["name"] == "kynos")
    yield BASELINE, ["--no-default-features", "--features", BASELINE]
    for feature in sorted(set(package["features"]) - EXCLUDED):
        yield feature, ["--no-default-features", "--features", f"{BASELINE},{feature}"]
    yield ALL_FEATURES, ["--all-features"]


def measure_binary(size, flags, env):
    """`.text` of the fixture built at one point.

    `.text` rather than the file size because it excludes the two sections that
    move without the code moving: symbol names, and the panic-message paths in
    `.rodata` that change when a file is renamed.
    The path comes from cargo rather than from `target/release/examples/`,
    because that guess is wrong wherever `CARGO_TARGET_DIR` or a
    `.cargo/config.toml` moves the build. Guessing it is the one way this
    sweep could report a wrong number as a right one: every build would land
    elsewhere, `llvm-size` would read one stale binary twenty-five times, and
    a table of zeroes would exit 0 saying nothing had drifted.
    """
    built = capture(
        [
            "cargo", "build", "-p", "kynos", "--release",
            "--example", "cost_fixture",
            "--message-format=json-render-diagnostics", *flags,
        ],
        env,
    )
    artifact = None
    for line in built.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("executable")
            and message.get("target", {}).get("name") == "cost_fixture"
        ):
            artifact = message["executable"]
    if artifact is None:
        return fail("cargo reported no `cost_fixture` executable")
    for line in capture([str(size), "-A", artifact]).splitlines():
        fields = line.split()
        if len(fields) >= 2 and fields[0] == ".text":
            return int(fields[1])
    return fail(f"llvm-size printed no `.text` section for {artifact}")


def parse_llvm_lines(output):
    """`cargo llvm-lines` output as its `(TOTAL)` pair and its function rows.

    Separate from the call that produces it so the two patterns above are
    reachable from a test: this is the step that turns a tool's text into every
    number the codegen half reports, and a silent regression in it yields a
    plausible wrong attribution rather than a failure. `total` is `None` when
    the output held no `(TOTAL)` row, which the caller reports as a
    measurement that could not be made.
    """
    total, functions = None, {}
    for line in output.splitlines():
        row = LLVM_LINES_ROW.match(line)
        if row is None:
            continue
        lines, copies = int(row[1]), int(row[2])
        name = DISAMBIGUATOR.sub("", row[3])
        if name == "(TOTAL)":
            total = (lines, copies)
        else:
            functions[name] = (lines, copies)
    return total, functions


def measure_codegen(flags, env):
    """The fixture's `(TOTAL)` IR lines and copies, and its per-function rows."""
    output = capture(
        [
            "cargo", "llvm-lines", "--package", "kynos",
            "--example", "cost_fixture", "--color", "never", "--sort", "lines",
            *flags,
        ],
        env,
    )
    total, functions = parse_llvm_lines(output)
    if total is None:
        return fail("cargo llvm-lines printed no `(TOTAL)` row")
    return total, functions


def sweep_binary(env, host):
    """`.text` at every point, and each point's delta against the baseline."""
    size = llvm_size(host)
    text = {}
    for label, flags in points():
        print(f"cost: binary {label}", file=sys.stderr, flush=True)
        text[label] = measure_binary(size, flags, env)
    base = text[BASELINE]
    return {
        label: {"text": value, "delta": value - base}
        for label, value in text.items()
    }


def sweep_codegen(env):
    """IR lines and copies at every point, plus per-point function tables."""
    totals, functions = {}, {}
    for label, flags in points():
        print(f"cost: codegen {label}", file=sys.stderr, flush=True)
        totals[label], functions[label] = measure_codegen(flags, env)
    base_lines, base_copies = totals[BASELINE]
    rows = {
        label: {
            "lines": lines,
            "copies": copies,
            "delta_lines": lines - base_lines,
            "delta_copies": copies - base_copies,
        }
        for label, (lines, copies) in totals.items()
    }
    return rows, functions


def read_recorded(path):
    """A committed baseline keyed by feature, or `None` if none is recorded."""
    if not path.is_file():
        return None
    rows = [
        line.split("\t")
        for line in path.read_text().splitlines()
        if line and not line.startswith("#")
    ]
    if not rows:
        return None
    names, *body = rows
    return {
        cells[0]: dict(zip(names[1:], (int(cell) for cell in cells[1:])))
        for cells in body
    }


def write_tsv(path, header, names, rows):
    """A baseline file: `#` prose, one header line, one line per point."""
    lines = ["\t".join(["feature", *names])]
    lines += [
        "\t".join([label, *(str(values[name]) for name in names)])
        for label, values in rows.items()
    ]
    path.write_text(header + "\n".join(lines) + "\n")


def table(rows, recorded, value, delta, unit):
    """The per-kind report table, and the two buckets it ranks points by.

    Two buckets rather than one, because a point the recorded baseline has no
    row for has no drift: there is nothing for it to have drifted from, and
    ranking it beside the points that did drift would put two different
    quantities in one list. It is collected as `fresh` and ranked by what it
    costs instead. Collecting it nowhere is what made the run that first
    measures a new feature the one run that ranks and attributes nothing --
    the row was in the table, and the separately headed sections below it were
    empty.
    """
    lines = [
        f"| feature | {unit} | delta | recorded | drift |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    drifts, fresh = {}, {}
    for label, measured in rows.items():
        was = None if recorded is None else recorded.get(label, {}).get(delta)
        if was is None:
            shown, moved = ("—", "—") if recorded is None else ("—", "new")
            # Not on a first run, where `movers` already ranks every point by
            # cost, and not for the baseline, which is the point the deltas are
            # taken against rather than a point with a cost of its own.
            if recorded is not None and label != BASELINE:
                fresh[label] = measured[delta]
        else:
            moved_by = measured[delta] - was
            # `openapi31` is the point every delta is taken against, so its own
            # drift is zero by construction. Ranking it would spend one of five
            # slots saying the baseline is the baseline.
            if label != BASELINE:
                drifts[label] = moved_by
            shown, moved = f"{was:+}", f"{moved_by:+}"
        lines.append(
            f"| `{label}` | {measured[value]} | {measured[delta]:+} "
            f"| {shown} | {moved} |"
        )
    return "\n".join(lines), drifts, fresh


def movers(rows, drifts, recorded, delta):
    """The points to list, ranked, and what the ranking means.

    By drift once there is a recorded baseline to have drifted from, and by raw
    cost before that -- a first run has no recorded deltas to have moved away
    from, and ranking it by nothing would print an empty list.
    """
    if recorded is None:
        ranked = [
            (label, measured[delta])
            for label, measured in rows.items()
            if label != BASELINE
        ]
        heading = "Largest cost, against the `openapi31` baseline"
    else:
        ranked = list(drifts.items())
        heading = "Largest drift, against the recorded baseline"
    # Only what actually moved. Padding the list to five with rows that did not
    # move makes the steady state -- nothing drifting at all -- into five lines
    # of noise that happen to be the alphabetically first features, and a
    # reader who learns to skip that section skips the run where something did
    # move. It is also what makes `- none` reachable and true.
    ranked = [row for row in ranked if row[1] != 0]
    ranked.sort(key=lambda item: (-abs(item[1]), item[0]))
    return ranked[:TOP], heading


def attribute(label, functions):
    """One point's composition: how its monomorphizations differ from baseline.

    This is the feature's standing cost against `openapi31` in *this* run. It
    is deliberately not the drift the list above may have ranked by, and the
    two must not be confused: drift is measured against the recorded baseline,
    and no per-function drift can be computed because per-function counts are
    not recorded -- `codegen.tsv` says why, and it is a decision rather than an
    omission. The heading printed above these blocks says which quantity this
    is, for that reason.

    Report-only, and not recorded: these names churn with every generic
    signature, so a committed file of them would be a diff generator rather
    than a baseline.
    """
    base, here = functions[BASELINE], functions[label]
    # The union of the two name sets, not just this point's. A feature that
    # *removes* a monomorphization moves the total exactly as much as one that
    # adds one, and walking only this point's functions would report a total
    # that changed with nothing under it to explain the change.
    moved = []
    for name in set(base) | set(here):
        lines, copies = here.get(name, (0, 0))
        was = base.get(name, (0, 0))[0]
        if lines != was:
            moved.append((name, lines - was, copies))
    if not moved:
        return [f"##### `{label}`", "", "- no monomorphization moved", ""]
    moved.sort(key=lambda row: (-abs(row[1]), row[0]))
    listed = []
    for name, lines, copies in moved[:TOP]:
        if copies == 0:
            tally = "not instantiated here"
        else:
            tally = f"{copies} " + ("copy" if copies == 1 else "copies")
        listed.append(f"- `{name}` {lines:+} lines, {tally}")
    return [f"##### `{label}`", "", *listed, ""]


def newcomers(fresh):
    """The points the recorded baseline has no row for, ranked by cost.

    Zero is kept here, unlike in `movers`. There the filter drops rows that did
    not move, which are noise; here a zero is the finding -- a feature added to
    `Cargo.toml` that costs a program not using it nothing is exactly what a
    reader of this section wants told, and dropping it would leave the feature
    unmentioned in the one run that first measured it.
    """
    return sorted(fresh.items(), key=lambda item: (-abs(item[1]), item[0]))[:TOP]


def section(title, note, rows, recorded, value, delta, unit, functions=None):
    """One kind's table, its ranked points, and optionally its attribution."""
    body, drifts, fresh = table(rows, recorded, value, delta, unit)
    ranked, heading = movers(rows, drifts, recorded, delta)
    listed = [f"- `{label}` {moved:+}" for label, moved in ranked] or ["- none"]
    out = [f"### {title}", "", note, "", body, "", f"#### {heading}", "", *listed, ""]
    new = newcomers(fresh)
    if new:
        out += [
            "#### Not in the recorded baseline, ranked by cost against "
            "`openapi31`",
            "",
            "These points have no drift to rank: the recorded baseline has no "
            "row for them, so there is nothing for them to have drifted from. "
            "What is ranked is what each costs in this run. Recording the "
            "baseline is what gives them a drift.",
            "",
            *[f"- `{label}` {cost:+}" for label, cost in new],
            "",
        ]
    attributed = [label for label, _ in ranked] + [label for label, _ in new]
    if functions is not None and attributed:
        out += [
            "#### What those features instantiate, against `openapi31`",
            "",
            "Each block is the listed feature's own composition in this run: "
            "the monomorphizations that differ between it and the `openapi31` "
            "baseline. Where the ranking above is by drift, this is *not* an "
            "explanation of that drift — per-function counts are not recorded "
            "in the baseline, so no per-function drift exists to show.",
            "",
        ]
        for label in attributed:
            out += attribute(label, functions)
    return out


def report(binary, codegen, functions, recorded, versions):
    """The trend report, as Markdown.

    A trend and nothing more: it states what moved and by how much, and passes
    no verdict on whether a number is too large. There is no ceiling to compare
    against, and `nfr.md#thresholds` holds that guessing one is worse than
    having none.
    """
    out = [
        "## Per-feature cost",
        "",
        f"Toolchain `{versions[0]}` on `{versions[1]}`, over "
        "`crates/kynos/cost/fixture.rs` at each feature.",
        "",
        "No ceiling is applied. This reports a trend; a threshold is set from a "
        "recorded measurement as a change to `docs/nfr.md`.",
        "",
    ]
    if binary is not None:
        out += section(
            "Binary delta",
            "`.text` of the linked fixture, built at `--release`.",
            binary,
            recorded[BINARY_TSV],
            "text",
            "delta",
            "`.text` bytes",
        )
    if codegen is not None:
        out += section(
            "Codegen delta",
            "Monomorphized LLVM IR for the fixture, counted at the dev profile "
            "because a fat-LTO build deletes what this counts. The attributed "
            "functions below need not sum to the total: `cargo llvm-lines` "
            "counts more into `(TOTAL)` than it lists as rows.",
            codegen,
            recorded[CODEGEN_TSV],
            "lines",
            "delta_lines",
            "IR lines",
            functions,
        )
    return "\n".join(out) + "\n"


def main():
    parsed = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parsed.add_argument(
        "--kind",
        choices=("both", "binary", "codegen"),
        default="both",
        help="which half of the sweep to run (default: both)",
    )
    kind = parsed.parse_args().kind

    env = sweep_env()
    versions = toolchain()
    recorded = {
        name: read_recorded(COST / name) for name in (BINARY_TSV, CODEGEN_TSV)
    }

    binary = sweep_binary(env, versions[1]) if kind != "codegen" else None
    codegen, functions = (
        sweep_codegen(env) if kind != "binary" else (None, None)
    )

    written = []
    if binary is not None:
        written.append(
            (
                BINARY_TSV,
                "cost-binary.tsv",
                BINARY_HEADER.format(
                    toolchain=versions[0],
                    host=versions[1],
                    baseline=binary[BASELINE]["text"],
                ),
                ["text", "delta"],
                binary,
            )
        )
    if codegen is not None:
        written.append(
            (
                CODEGEN_TSV,
                "cost-codegen.tsv",
                CODEGEN_HEADER.format(
                    toolchain=versions[0],
                    host=versions[1],
                    baseline=codegen[BASELINE]["lines"],
                ),
                ["lines", "copies", "delta_lines", "delta_copies"],
                codegen,
            )
        )

    text = report(binary, codegen, functions, recorded, versions)
    (ROOT / "cost-report.md").write_text(text)
    print(text)

    overwrite = os.environ.get("KYNOS_COST") == "overwrite"
    for name, generated, header, names, rows in written:
        write_tsv(ROOT / generated, header, names, rows)
        if overwrite:
            COST.mkdir(parents=True, exist_ok=True)
            write_tsv(COST / name, header, names, rows)
            print(f"cost: recorded {COST / name}", file=sys.stderr)

    missing = [name for name, _, _, _, _ in written if recorded[name] is None]
    if missing and not overwrite:
        print(
            f"cost: no baseline recorded for {', '.join(missing)}; "
            "run `mise run cost:record`",
            file=sys.stderr,
        )


# Guarded rather than called outright, so that `scripts/cost_features_test.py`
# can import the pure functions above -- the two parsers, the round-trip and
# the report's ranking -- without running a fifty-build sweep to reach them.
if __name__ == "__main__":
    main()
