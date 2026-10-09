"""Runs the request-path profile and reports what it counted, exactly.

Runs `crates/kynos-profile/benches/requests.rs` under Callgrind and DHAT, the
instruction-count kind in [`performance.md`](../docs/performance.md), and turns
gungraun's output into numbers a comparison can hold exact.

**Callgrind counts are split by object.** glibc's `malloc` and `memcpy` vary
with the heap's history, so only the benchmark binary's count (Kynos and every
Rust dependency, statically linked) is compared; shared objects are reported
beside it. DHAT counts the allocator's work exactly.

**Exact per host, not across hosts.** Dependencies pick routines from the CPU
they find, so the baseline records its host and a report taken elsewhere says
so.

**Calibration.** DHAT's block count for each `SHAPES` and `STACKS` row of
`crates/kynos/tests/alloc.rs`, read off disk, must *equal* the row's count:
the failure guarded against is DHAT counting too few. So a change removing one
of those allocations must lower the ceiling in the same change, or the profile
on `master` reports `DISAGREE`.

`KYNOS_PROFILE=overwrite` records this run as
`crates/kynos-profile/requests.tsv`; otherwise the run is compared with it.
Either way it is written to `profile-requests.tsv`, which CI uploads so the host
of record's run can be copied over the baseline.

Exit zero whenever a measurement was made (no threshold, per
[`nfr.md`](../docs/nfr.md#thresholds)); non-zero when nothing could be measured
or read (`NOTHING`), or when the two instruments disagree (`DISAGREE`).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from collections import namedtuple
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "crates/kynos-profile/requests.tsv"
ALLOC = ROOT / "crates/kynos/tests/alloc.rs"
REPORT = ROOT / "profile-report.md"
RUN = ROOT / "profile-requests.tsv"

# Where `profile:valgrind` installs, unless `KYNOS_VALGRIND` says otherwise.
VALGRIND = Path(
    os.environ.get("KYNOS_VALGRIND", Path.home() / ".local/share/kynos-valgrind")
)

MEASURED, NOTHING, DISAGREE = 0, 1, 2

# Each calibration benchmark and its `alloc.rs` row (`SHAPES` by request
# target, `STACKS` by depth), keyed so reordering cannot mispair them.
CALIBRATION = {
    "alloc_counter_agreement.static_match": ("shape", "/ping"),
    "alloc_counter_agreement.capture": ("shape", "/users/7"),
    "alloc_counter_agreement.miss": ("shape", "/nope"),
    "alloc_counter_agreement.stacked_4": ("stack", "4"),
    "alloc_counter_agreement.stacked_8": ("stack", "8"),
}

# An ambient flag builds something else; `-C target-cpu=native` with AVX-512
# builds a binary Valgrind cannot decode.
RUSTFLAGS = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")

Row = namedtuple("Row", "benchmark program other blocks bytes")

COLUMNS = ("benchmark", "program_instructions", "heap_blocks", "heap_bytes")


class Unreadable(Exception):
    """Output this cannot read, which is a measurement not made."""


def split_by_object(text, program):
    """Instructions per object in a Callgrind output file, as (program, other).

    `program` is the benchmark executable's path; everything else is summed
    into `other`. Only self cost is counted: the line after a `calls=` line is
    the inclusive cost of that call, already counted where the callee's own
    lines are, so reading it would count the callee twice.

    Callgrind compresses a repeated name to `(id)` after its first spelling as
    `(id) name`, and `cob=` shares that table with `ob=`, so both are read into
    one map even though only `ob=` decides where a cost line belongs.

    Only `positions: line` is read. A position column per `--dump-instr` or
    `--dump-line` setting moves the first cost to a later field, and a file
    read at the wrong offset still yields plausible integers, so any other
    header is refused rather than guessed at.
    """
    names = {}
    current = None
    events = None
    positions = None
    counts = {}
    after_call = False

    for line in text.splitlines():
        if line.startswith("events:"):
            events = line.split()[1:]
            continue
        if line.startswith("positions:"):
            positions = line.split()[1:]
            continue
        match = re.match(r"^(c?ob)=(?:\((\d+)\))?\s*(.*)$", line)
        if match:
            kind, index, name = match.groups()
            if index is not None and name:
                names[index] = name
            resolved = names.get(index, name) if index is not None else name
            if kind == "ob":
                current = resolved
            continue
        if line.startswith("calls="):
            after_call = True
            continue
        if line[:1].isdigit() or line[:1] in "+-*":
            if after_call:
                after_call = False
                continue
            fields = line.split()
            if len(fields) >= 2:
                counts[current] = counts.get(current, 0) + int(fields[1])

    if events is None or events[:1] != ["Ir"]:
        raise Unreadable("a Callgrind output with no `events: Ir ...` header")
    if positions not in (None, ["line"]):
        raise Unreadable(
            f"a Callgrind output with `positions: {' '.join(positions)}`; "
            "only `line` is read"
        )

    program_count = sum(count for obj, count in counts.items() if obj == program)
    other = sum(count for obj, count in counts.items() if obj != program)
    return program_count, other


def dhat(summary):
    """(total blocks, total bytes) from a gungraun `summary.json`."""
    for profile in summary["profiles"]:
        if profile["tool"] == "DHAT":
            metrics = profile["data"]["total"]["metrics"]
            return (
                metrics["TotalBlocks"]["values"]["new"],
                metrics["TotalBytes"]["values"]["new"],
            )
    raise Unreadable(
        f"{summary['module_path']} {summary['id']} carries no DHAT profile"
    )


def measure(directory):
    """Every benchmark under `directory`, as rows sorted by benchmark name.

    Paths in a summary are relative to the `project_root` it names, which is
    where gungraun ran rather than where this script lives.
    """
    rows = []
    for path in sorted(directory.rglob("summary.json")):
        summary = json.loads(path.read_text())
        root = Path(summary["project_root"])
        output = root / summary["output_dir"]
        callgrind = output / f"callgrind.{summary['function_name']}.{summary['id']}.out"
        program = str(root / summary["benchmark_exe"])
        instructions, other = split_by_object(callgrind.read_text(), program)
        if instructions == 0:
            raise Unreadable(
                f"{callgrind} attributes nothing to {program}; the executable's "
                "path no longer matches the `ob=` Callgrind wrote, and every "
                "instruction would read as the allocator's"
            )
        blocks, octets = dhat(summary)
        rows.append(
            Row(
                f"{summary['group']}.{summary['id']}",
                instructions,
                other,
                blocks,
                octets,
            )
        )
    return rows


def recorded_counts(text):
    """What `alloc.rs` records, as {("shape", target) or ("stack", depth): count}.

    The depth-0 `STACKS` row names `STACKED_ALONE` rather than a number -- it is
    the static match again -- so the pattern reads only rows that write one.
    """
    shape_table = re.search(
        r"const SHAPES: \[\(&str, StatusCode, usize\); \d+\] = \[(.*?)\n\];", text, re.S
    )
    stack_table = re.search(r"const STACKS: \[Stack; \d+\] = \[(.*?)\n\];", text, re.S)
    if shape_table is None or stack_table is None:
        raise Unreadable(
            "tests/alloc.rs no longer declares `SHAPES` and `STACKS` "
            "in the shape this reads"
        )
    counts = {
        ("shape", target): int(count)
        for target, count in re.findall(
            r'\("([^"]+)",\s*StatusCode::\w+,\s*(\d+)\)', shape_table.group(1)
        )
    }
    counts.update(
        {
            ("stack", depth): int(count)
            for depth, count in re.findall(
                r"\((\d+),\s*\w+,\s*(\d+)\)", stack_table.group(1)
            )
        }
    )
    return counts


def disagreements(rows, recorded):
    """Where DHAT's block count differs from what `alloc_counter` recorded."""
    by_name = {row.benchmark: row for row in rows}
    problems = []
    for name, key in CALIBRATION.items():
        row = by_name.get(name)
        count = recorded.get(key)
        if count is None:
            problems.append(f"{name}: tests/alloc.rs records no {key[0]} {key[1]!r}")
        elif row is None:
            problems.append(f"{name} was not measured")
        elif row.blocks != count:
            problems.append(
                f"{name}: DHAT counted {row.blocks} blocks, "
                f"alloc_counter records {count}. If a change removed an "
                "allocation, lower that ceiling in tests/alloc.rs; "
                "if not, DHAT lost frames or the region moved"
            )
    return problems


def read_baseline(text):
    """A recorded baseline, as ({benchmark: (instructions, blocks, bytes)}, host)."""
    recorded = {}
    host = None
    for line in text.splitlines():
        if line.startswith("# host: "):
            host = line.removeprefix("# host: ")
            continue
        if not line or line.startswith("#") or line.startswith(COLUMNS[0]):
            continue
        name, instructions, blocks, octets = line.split("\t")
        recorded[name] = (int(instructions), int(blocks), int(octets))
    return recorded, host


def write_baseline(rows, toolchain, host):
    lines = [
        "# Written by `mise run profile:record`; read by `mise run profile:requests`.",
        f"# toolchain: {toolchain}",
        f"# host: {host}",
        "\t".join(COLUMNS),
    ]
    lines += [
        f"{row.benchmark}\t{row.program}\t{row.blocks}\t{row.bytes}" for row in rows
    ]
    return "\n".join(lines) + "\n"


def delta(now, then):
    if then is None:
        return "new"
    if now == then:
        return "="
    return f"{now - then:+d}"


def report(rows, recorded, host=None, recorded_host=None):
    """The Markdown a person reads: one row per benchmark, and what moved."""
    lines = []
    if recorded_host is not None and host is not None and host != recorded_host:
        lines += [
            f"Measured on `{host}`; the baseline was recorded on `{recorded_host}`. "
            "Instruction deltas include whatever the two CPUs make "
            "dependencies do differently.",
            "",
        ]
    header = (
        "Benchmark",
        "Instructions (program)",
        "Δ",
        "Instructions (libc, not compared)",
        "Heap blocks",
        "Δ",
        "Heap bytes",
        "Δ",
    )
    lines += ["| " + " | ".join(header) + " |", "| --- |" + " ---: |" * 7]
    for row in rows:
        then = recorded.get(row.benchmark, (None, None, None))
        cells = (
            f"`{row.benchmark}`",
            row.program,
            delta(row.program, then[0]),
            row.other,
            row.blocks,
            delta(row.blocks, then[1]),
            row.bytes,
            delta(row.bytes, then[2]),
        )
        lines.append("| " + " | ".join(str(cell) for cell in cells) + " |")
    return "\n".join(lines) + "\n"


def output(command):
    return subprocess.run(
        command, capture_output=True, text=True, check=True
    ).stdout.strip()


def search_path():
    """`PATH` with the Valgrind `profile:valgrind` builds ahead of any other."""
    return f"{VALGRIND / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}"


def describe_host(cpuinfo=Path("/proc/cpuinfo")):
    """The CPU and the Valgrind a count was taken under."""
    model = "unknown CPU"
    if cpuinfo.is_file():
        found = re.search(r"^model name\s*:\s*(.+)$", cpuinfo.read_text(), re.M)
        if found:
            model = found.group(1).strip()
    valgrind = shutil.which("valgrind", path=search_path())
    version = output([valgrind, "--version"]) if valgrind else "no valgrind"
    return f"{model}, {version}"


def target_directory():
    """Cargo's target directory, wherever `CARGO_TARGET_DIR` or config put it."""
    metadata = json.loads(
        output(["cargo", "metadata", "--no-deps", "--format-version", "1"])
    )
    return Path(metadata["target_directory"])


def run_benchmarks(target):
    """Runs the benchmark from a clean output directory, under the pinned Valgrind.

    The previous run's output is removed first: a renamed or deleted benchmark
    leaves its summary behind, and `measure` would read it.
    """
    ambient = [name for name in RUSTFLAGS if os.environ.get(name)]
    if ambient:
        raise Unreadable(
            f"{', '.join(ambient)} is set; unset it -- the profile describes "
            "`[profile.bench]` as the workspace declares it, and a "
            "`target-cpu=native` build on an AVX-512 host is one Valgrind "
            "cannot run"
        )
    shutil.rmtree(target / "gungraun/kynos-profile", ignore_errors=True)
    subprocess.run(
        [
            "cargo",
            "bench",
            "-p",
            "kynos-profile",
            "--bench",
            "requests",
            "--",
            "--save-summary=json",
        ],
        cwd=ROOT,
        env=dict(os.environ, PATH=search_path()),
        check=True,
    )


def main(
    mode=None,
    directory=None,
    baseline=BASELINE,
    alloc=ALLOC,
    report_path=REPORT,
    toolchain=None,
    host=None,
    run_path=RUN,
):
    mode = os.environ.get("KYNOS_PROFILE", "") if mode is None else mode
    try:
        rows = measure(directory)
        recorded_alloc = recorded_counts(alloc.read_text())
    except (Unreadable, OSError, KeyError, ValueError) as error:
        print(f"profile: nothing could be read: {error}", file=sys.stderr)
        return NOTHING
    if not rows:
        print(
            f"profile: no gungraun summary under {directory}; run the benchmarks first",
            file=sys.stderr,
        )
        return NOTHING

    problems = disagreements(rows, recorded_alloc)
    recorded, recorded_host = (
        read_baseline(baseline.read_text()) if baseline.is_file() else ({}, None)
    )
    text = report(rows, recorded, host, recorded_host)
    report_path.write_text(text)
    print(text)

    if problems:
        print(
            "profile: DHAT and alloc_counter disagree about the same request:\n    "
            + "\n    ".join(problems),
            file=sys.stderr,
        )
        return DISAGREE
    try:
        toolchain = toolchain or output(["rustc", "-V"])
    except (subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the toolchain could not be named: {error}", file=sys.stderr)
        return NOTHING
    measured = write_baseline(rows, toolchain, host or "unrecorded host")
    run_path.write_text(measured)
    if mode == "overwrite":
        baseline.write_text(measured)
    return MEASURED


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--run", action="store_true", help="run the benchmark before reading its output"
    )
    arguments = parser.parse_args()
    try:
        target = target_directory()
        if arguments.run:
            run_benchmarks(target)
    except (Unreadable, subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the benchmark could not be run: {error}", file=sys.stderr)
        sys.exit(NOTHING)
    try:
        host = describe_host()
    except (subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the host could not be described: {error}", file=sys.stderr)
        sys.exit(NOTHING)
    sys.exit(main(directory=target / "gungraun/kynos-profile", host=host))
