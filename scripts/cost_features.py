"""Per-feature cost sweep: what each feature costs a linked artifact.

[`performance.md`](../docs/performance.md)'s taxonomy files two sweep kinds --
a binary delta over `.text` and a codegen delta over monomorphized IR -- and
says of both that they "build the same fixture at each feature and compare
artifacts". This is the driver for that, and `crates/kynos/cost/fixture.rs` is
the fixture. Because that fixture never *uses* an optional feature, each row
answers one question: what does merely enabling F cost a program that does not
use F? `lib.rs` claims enabling a feature is additive, and this is the first
thing in the repository that could falsify it.

`cargo hack` is deliberately not the driver, though the feature list copies
`features:targets`' shape exactly. cargo-hack has no hook between builds and
every build overwrites `target/release/examples/cost_fixture`, so a sweep it
drove could not attribute an artifact to the feature that produced it: it would
measure the last build once per point. Driving the loop here also means nothing
in this task rewrites a member manifest, which is why it may share a job with
neither `features:check` nor anything that follows it.

A script rather than a shell pipeline for `containment.py`'s reason: this
parses a tool's output, diffs a committed table and emits Markdown, and every
one of those is where a pipeline gets it quietly wrong.

Exit codes follow the `semver` CI job's rule. Zero whenever a measurement was
made, whatever it says -- no threshold is applied here and none is recorded
anywhere, per [`nfr.md`](../docs/nfr.md#thresholds), which sets a ceiling from a
first recorded measurement and never guesses one. Non-zero only when a
measurement could not be made at all: a build that did not compile, a missing
`llvm-size`, an ambient `RUSTFLAGS`.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

# From the script's own location rather than the working directory, so running
# it by hand from a crate directory sweeps the same tree mise does.
ROOT = Path(__file__).resolve().parent.parent
COST = ROOT / "crates/kynos/cost"
ARTIFACT = ROOT / "target/release/examples/cost_fixture"

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


def fail(message, code=1):
    """Report that a measurement could not be made, and stop."""
    print(f"cost: {message}", file=sys.stderr)
    sys.exit(code)


def capture(command, env=None):
    """Run `command`, returning its stdout; a non-zero exit is a failure."""
    result = subprocess.run(
        command, cwd=ROOT, env=env, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        fail(f"`{' '.join(str(part) for part in command)}` exited {result.returncode}")
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
    print and switching instruments partway through a trend is exactly the
    noise a trend must not have, so an absent tool is a failure that names
    what to install.
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
    metadata = json.loads(capture(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
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
    """
    capture(
        ["cargo", "build", "-p", "kynos", "--release", "--example", "cost_fixture", *flags],
        env,
    )
    for line in capture([str(size), "-A", str(ARTIFACT)]).splitlines():
        fields = line.split()
        if len(fields) >= 2 and fields[0] == ".text":
            return int(fields[1])
    return fail(f"llvm-size printed no `.text` section for {ARTIFACT}")


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
        "\t".join([feature, *(str(values[name]) for name in names)])
        for feature, values in rows.items()
    ]
    path.write_text(header + "\n".join(lines) + "\n")


def deltas(measured):
    """Each point's difference against the baseline point in the same run."""
    base = measured[BASELINE]
    return {feature: value - base for feature, value in measured.items()}


def table(rows, recorded, unit):
    """The per-kind report table, and the drift ranking it is sorted by."""
    lines = [
        f"| feature | {unit} | delta | recorded | drift |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    drifts = {}
    for feature, (value, delta) in rows.items():
        was = None if recorded is None else recorded.get(feature, {}).get("delta")
        if was is None:
            shown, drift = ("—", "—") if recorded is None else ("—", "new")
        else:
            drifts[feature] = delta - was
            shown, drift = f"{was:+}", f"{drifts[feature]:+}"
        lines.append(
            f"| `{feature}` | {value} | {delta:+} | {shown} | {drift} |"
        )
    return "\n".join(lines), drifts


def report(measured, recorded, versions):
    """The trend report, as Markdown.

    A trend and nothing more: it states what moved and by how much, and passes
    no verdict on whether a number is too large. There is no ceiling to compare
    against, and `nfr.md#thresholds` says guessing one is worse than having
    none.
    """
    delta = deltas(measured)
    rows = {feature: (measured[feature], delta[feature]) for feature in measured}
    body, drifts = table(rows, recorded, "`.text`")

    # Ranked by drift once there is something to drift from, and by raw cost
    # before that -- a first run has no recorded deltas to have moved away
    # from, and ranking it by nothing at all would print an empty list.
    if recorded is None:
        movers = [(f, d) for f, d in delta.items() if f != BASELINE]
        heading = "Largest cost, against the `openapi31` baseline"
    else:
        movers = list(drifts.items())
        heading = "Largest drift, against the recorded baseline"
    movers.sort(key=lambda item: -abs(item[1]))

    listed = "\n".join(f"- `{f}` {d:+} bytes" for f, d in movers[:5]) or "- none"
    return "\n".join(
        [
            "## Per-feature cost",
            "",
            f"Toolchain `{versions[0]}` on `{versions[1]}`. `.text` of "
            "`crates/kynos/cost/fixture.rs`, built at `--release`.",
            "",
            "No ceiling is applied. This reports a trend; a threshold is set "
            "from a recorded measurement as a change to `docs/nfr.md`.",
            "",
            "### Binary delta",
            "",
            body,
            "",
            f"#### {heading}",
            "",
            listed,
            "",
        ]
    )


def main():
    env = sweep_env()
    versions = toolchain()
    size = llvm_size(versions[1])

    measured = {}
    for label, flags in points():
        print(f"cost: building {label}", file=sys.stderr, flush=True)
        measured[label] = measure_binary(size, flags, env)

    recorded = read_recorded(COST / BINARY_TSV)
    delta = deltas(measured)
    rows = {
        feature: {"text": measured[feature], "delta": delta[feature]}
        for feature in measured
    }
    header = BINARY_HEADER.format(
        toolchain=versions[0], host=versions[1], baseline=measured[BASELINE]
    )

    (ROOT / "cost-report.md").write_text(report(measured, recorded, versions))
    write_tsv(ROOT / "cost-binary.tsv", header, ["text", "delta"], rows)
    if os.environ.get("KYNOS_COST") == "overwrite":
        COST.mkdir(parents=True, exist_ok=True)
        write_tsv(COST / BINARY_TSV, header, ["text", "delta"], rows)
        print(f"cost: recorded {COST / BINARY_TSV}", file=sys.stderr)

    print((ROOT / "cost-report.md").read_text())
    if recorded is None:
        print("cost: no baseline recorded; run `mise run cost:record`", file=sys.stderr)


main()
