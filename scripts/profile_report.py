"""Runs the request-path profile and reports what it counted, exactly.

The instruction-count kind in [`performance.md`](../docs/performance.md) runs
`crates/kynos-profile/benches/requests.rs` under Callgrind and DHAT. gungraun
prints what it measured; this turns that into numbers a comparison can hold
exact, and the reason it has to do more than read gungraun's own summary is
what the first measurement found.

**Callgrind's total is not repeatable, and the part of it that is not is
glibc's.** Across two runs of one binary over one request, every function in
the benchmark binary cost the same instructions, and two functions did not:
`_int_malloc` (426 against 468) and `__memcpy_avx_unaligned_erms` (1778 against
1780). What `malloc` does on a call depends on what the heap looks like, which
`Router::build` and every other allocation before the region decide, and a
`memcpy`'s head and tail depend on where the allocator put the buffers. So the
total moved by up to four percent between runs while no line of Kynos did.
Disabling address-space randomization does not change that; the heap's history
is the variable, not its base.

So every Callgrind count is split by the object the instructions ran in. The
benchmark binary carries Kynos and every Rust dependency, statically linked, and
its count is exact: the same integer on every run of the same build on the same
host, which is what `performance.md` asks of a counted kind. What ran in
`libc.so.6` and in any other shared object is reported beside it and never
compared. The allocator's work is not lost by that: DHAT counts every block and
every byte, and those depend on what was asked for rather than on how the
allocator found it.

**Exact per host, not across hosts.** Dependencies choose code paths from the
CPU they find: `memchr`, which `serde_json` scans strings with, picks AVX2 or
SSE2 routines at run time, and Valgrind reports whatever the host has. The
benchmark serves one request before the measured one, so the *detection* is
outside the region; the path it chose is not, and cannot be. The baseline
therefore records the host it was taken on, and a report taken elsewhere says
so rather than presenting a CPU's difference as a change.

**The calibration group holds this instrument to the other one.**
`crates/kynos/tests/alloc.rs` counts three request shapes and two interceptor
stacks with `alloc_counter`, and its `SHAPES` and `STACKS` tables are read off
disk here rather than copied, so DHAT's block count for each has to equal the
count those tables record. Equality rather than the `<=` that file asserts, and
that file is why: it records its counts "so that closing the gap turns
something red rather than nothing", and a ceiling alone stays green when an
allocation is removed. Two instruments that share no line of code agreeing on
one region is what says both measure the request and neither measures the
harness around it.

`KYNOS_PROFILE=overwrite` records this run as
`crates/kynos-profile/requests.tsv`; otherwise the run is compared with it and
reported. Exit codes follow `cost_features.py`'s rule: zero whenever a
measurement was made, whatever it says, because no threshold is set here and
[`nfr.md`](../docs/nfr.md#thresholds) sets none without a recorded
measurement. Non-zero when nothing could be measured or read (`NOTHING`), and
when the two instruments disagree (`DISAGREE`), which no mode tolerates.
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

# Where `profile:valgrind` installs, unless `KYNOS_VALGRIND` says otherwise.
VALGRIND = Path(
    os.environ.get("KYNOS_VALGRIND", Path.home() / ".local/share/kynos-valgrind")
)

MEASURED, NOTHING, DISAGREE = 0, 1, 2

# Each calibration benchmark and the `alloc.rs` row it is the same request as:
# a `SHAPES` row by its request target, a `STACKS` row by its depth. Keyed
# rather than paired by position, so that reordering either table cannot pair
# a benchmark with a neighbour whose count happens to match.
CALIBRATION = {
    "alloc_counter_agreement.static_match": ("shape", "/ping"),
    "alloc_counter_agreement.capture": ("shape", "/users/7"),
    "alloc_counter_agreement.miss": ("shape", "/nope"),
    "alloc_counter_agreement.stacked_4": ("stack", "4"),
    "alloc_counter_agreement.stacked_8": ("stack", "8"),
}

# An ambient flag builds something other than what the profile describes, and
# `-C target-cpu=native` on a host with AVX-512 builds a binary Valgrind cannot
# decode at all.
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


def describe_host():
    """The CPU and the Valgrind a count was taken under."""
    model = "unknown CPU"
    cpuinfo = Path("/proc/cpuinfo")
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
    if mode == "overwrite":
        baseline.write_text(
            write_baseline(
                rows, toolchain or output(["rustc", "-V"]), host or "unrecorded host"
            )
        )
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
        host = describe_host()
    except (Unreadable, subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the benchmark could not be run: {error}", file=sys.stderr)
        sys.exit(NOTHING)
    sys.exit(main(directory=target / "gungraun/kynos-profile", host=host))
