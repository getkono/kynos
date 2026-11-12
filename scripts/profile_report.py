"""Runs the request-path profile and reports what it counted, exactly.

Runs `crates/kynos-profile/benches/requests.rs` under Callgrind and DHAT, the
instruction-count kind in [`performance.md`](../docs/performance.md), and turns
gungraun's output into numbers a comparison can hold exact.

**Callgrind counts are split by object.** glibc's `malloc` and `memcpy` vary
with the heap's history, so only the benchmark binary's count (Kynos and every
Rust dependency, statically linked) is compared; shared objects are reported
beside it. DHAT counts the allocator's work exactly.

**So is every routine the CPU picks.** A dependency that chooses its code by
a runtime CPU check is linked statically into that binary, so its choice would
be compared with the program's own. Each one `DISPATCHED` lists is split off
by the source file or symbol a cost line belongs to and reported beside the
program's count, never compared, as libc's is: aws-lc's C and assembly, zstd's
C, and the bodies `httparse`, `memchr`, `crc32fast` and `simd-adler32` select
with `is_x86_feature_detected!`. The dispatcher and its call sites stay in the
program's count, so a change in how often Kynos reaches them is still held.
Any dependency on the request path that picks code by CPU is carved out here,
never tolerated.

**Exact across hosts exposing the same features.** The program's count is the
same on every CPU whose features Valgrind exposes alike: a dispatcher's own
code is in it, and it can branch by CPU inside the program (memchr's AVX2 setup
inlined into `memchr::memmem::find`, httparse's swar fallback inlined into
`simd::runtime` where AVX2 is absent). The dispatched share and libc's are each
host's own, so the baseline records its host and a report taken elsewhere says
so.

**Calibration.** DHAT's block count for each `SHAPES` and `STACKS` row of
`crates/kynos/tests/alloc.rs`, read off disk, must *equal* the row's count:
the failure guarded against is DHAT counting too few. So a change removing one
of those allocations must lower the ceiling in the same change, or the profile
on `master` reports `DISAGREE`.

**Marginal cost.** A scenario naming a `base` in `crates/kynos-profile` differs
from it in one variable, so the report gives each such row's difference from
its base: what that one variable costs. The catalog is read from the
`catalog` binary rather than restated here.

**Exactness is asserted, not assumed.** `--repeat N` runs the benchmark N
times and fails (`INEXACT`) when any program or DHAT count differs between
runs. A group that cannot be exact, because what it counts reads the wall
clock or a key the process draws at random, is named on the baseline's
`# trend:` line: it is reported, never compared, against the baseline or
across runs.

**Attribution.** Each run also writes `target/gungraun/profile-attribution.md`:
per benchmark, the top inclusive frames `callgrind_annotate --inclusive=yes`
ranks, and DHAT's allocation sites, by the innermost frame in a workspace
crate. A run that cannot be attributed says so and writes no file; it is
still the measurement it was, and exits as one.

**The harness builds what applications run.** `--run` refuses (`NOTHING`) a
harness whose `serde_json` carries a `FOREIGN_FEATURES` entry, which a
test-only feature of `kynos` would otherwise bring into every JSON row.

`KYNOS_PROFILE=overwrite` records this run as
`crates/kynos-profile/requests.tsv`; otherwise the run is compared with it.
Either way it is written to `profile-requests.tsv`, which CI uploads so the host
of record's run can be copied over the baseline.

**The baseline is a ceiling.** Each row outside a `trend` group is held to
what `requests.tsv` records for it, the first recorded measurement
[`nfr.md`](../docs/nfr.md#thresholds) sets a threshold from: its heap blocks,
heap bytes and program instructions, each exactly. A compared run that exceeds one
fails (`EXCEEDED`); a lower count passes and is reported, since lowering the
ceiling is re-recording. A row the baseline does not hold yet has no ceiling.
A recording is never held to the baseline it replaces.

Exit zero when a measurement was made and no row exceeded its ceiling; non-zero
when nothing could be measured or read (`NOTHING`), when the two instruments
disagree (`DISAGREE`), when two runs of an exact group disagree (`INEXACT`), or
when a row exceeded its ceiling (`EXCEEDED`).
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

# Before the import below, which is a sibling script's: a run writes no `.pyc`
# into the tree it reads.
sys.dont_write_bytecode = True

from cost_features import AMBIENT_FLAGS  # noqa: E402

ROOT =Path(__file__).resolve().parent.parent
BASELINE = ROOT / "crates/kynos-profile/requests.tsv"
ALLOC = ROOT / "crates/kynos/tests/alloc.rs"
REPORT = ROOT / "profile-report.md"
RUN = ROOT / "profile-requests.tsv"
# Under the target directory, beside the output it is read from.
ATTRIBUTION = Path("gungraun/profile-attribution.md")

# Where `profile:valgrind` installs, which mise.toml's `[env]` decides; the
# script runs under `mise run`.
VALGRIND = Path(os.environ["KYNOS_VALGRIND"])

MEASURED, NOTHING, DISAGREE, INEXACT, EXCEEDED = 0, 1, 2, 3, 4

# `serde_json` features no application's request path is assumed to carry: a
# harness build with one measures a parser the applications it describes do not
# run. `kynos/test-util` is what would bring `float_roundtrip`.
FOREIGN_FEATURES = ("float_roundtrip",)

# How many inclusive frames attribution lists per benchmark.
FRAMES = 15

# A frame whose function belongs to a workspace crate, after DHAT's address.
WORKSPACE_FRAME = re.compile(r"^<?(kynos|kynos_openapi|kynos_macros|kynos_profile)::")

# Each calibration benchmark and its `alloc.rs` row (`SHAPES` by request
# target, `STACKS` by depth), keyed so reordering cannot mispair them.
CALIBRATION = {
    "alloc_counter_agreement.static_match": ("shape", "/ping"),
    "alloc_counter_agreement.capture": ("shape", "/users/7"),
    "alloc_counter_agreement.miss": ("shape", "/nope"),
    "alloc_counter_agreement.stacked_4": ("stack", "4"),
    "alloc_counter_agreement.stacked_8": ("stack", "8"),
}

#: `scenario` is the catalog name the benchmark was set up with, read off its
#: summary; `None` where the summary names none.
#: `dispatched` is the `DISPATCHED` share of the program object, which
#: `program` excludes.
Row = namedtuple(
    "Row", "benchmark program other blocks bytes scenario dispatched", defaults=(None, 0)
)

COLUMNS = ("benchmark", "program_instructions", "heap_blocks", "heap_bytes")


class Unreadable(Exception):
    """Output this cannot read, which is a measurement not made."""


#: One dependency whose instructions inside the program object its CPU check
#: chooses: a cost line is its when the line's source file matches `file` or
#: its function matches `function`. Either may be `None`, matching nothing.
Dispatched = namedtuple("Dispatched", "name file function")

#: Every dependency on the request path that picks code by CPU, checked against
#: the sources of the versions `Cargo.lock` locks: `aws-lc-sys` 0.46.0,
#: `zstd-sys` 2.1.2+zstd.1.5.7, `httparse` 1.10.1, `memchr` 2.8.3,
#: `crc32fast` 1.5.2, `simd-adler32` 0.3.10, and the toolchain's `std_detect`.
#: A lockfile bump that moves one of them, or adds a dependency that dispatches,
#: is checked against this table.
#:
#: - aws-lc's C and assembly picks AES-NI, AVX and SHA routines from
#:   `OPENSSL_ia32cap`. `aws-lc-sys` maps its tree to `/aws-lc`, and an unmapped
#:   build keeps it under the crate's `aws-lc/`; `aws-lc-rs-<version>/` is the
#:   Rust binding's, which rustls calls, and stays the program's. Every exported
#:   symbol carries its version prefix, which names a line with no source file.
#: - zstd's C picks a BMI2 or a generic body with `ZSTD_cpuid` (`DYNAMIC_BMI2`
#:   on x86-64). Its sources are under `zstd-sys`'s `zstd/lib/`; a line it
#:   inlines from a system header (`emmintrin.h`, `string_fortified.h`) is named
#:   by its function, every one of which zstd prefixes with its module.
#: - `httparse` parses with an AVX2, an SSE4.2 or a SWAR body; `memchr` searches
#:   with an AVX2 or an SSE2 one, through `memmem`'s searcher and prefilter
#:   kinds and each `memchr_raw`-style ifunc's `find_avx2`/`find_sse2`;
#:   `crc32fast` hashes with PCLMULQDQ or its table-driven baseline, which also
#:   finishes the PCLMULQDQ body's tail; `simd-adler32` with one `imp` per
#:   instruction set. A `#[target_feature]` body cannot be inlined into a caller
#:   without the feature, so each keeps its own symbol and is named by it; the
#:   crate's dispatcher does not match, save `simd-adler32`'s `imp::get_imp`,
#:   whose chain of checks stops at the first set the CPU has.
#: - `std_detect`'s first-use detection runs `cpuid` and `xgetbv` and branches
#:   on what they return, should a measured region ever be the first to ask.
DISPATCHED = (
    Dispatched(
        "aws-lc",
        re.compile(r"(?:^|/)aws-lc/"),
        re.compile(r"^aws_lc_\d+_\d+_\d+_"),
    ),
    Dispatched(
        "zstd",
        re.compile(r"/zstd-sys-[^/]+/zstd/lib/"),
        re.compile(r"^(?:ZSTD|ZSTDMT|ZDICT|HUF|FSE|HIST|POOL)_"),
    ),
    Dispatched("httparse", None, re.compile(r"^<?httparse::simd::(?:avx2|sse42)::")),
    Dispatched(
        "memchr",
        None,
        re.compile(
            r"^<?memchr::(?:arch::x86_64::(?:avx2|sse2)::"
            r"|arch::x86_64::memchr::\w+::find_(?:avx2|sse2)$"
            r"|memmem::searcher::(?:searcher|prefilter)_kind_(?:avx2|sse2)$)"
        ),
    ),
    Dispatched("crc32fast", None, re.compile(r"^<?crc32fast::(?:specialized|baseline)::")),
    Dispatched("simd-adler32", None, re.compile(r"^<?simd_adler32::imp::")),
    Dispatched("std_detect", None, re.compile(r"^<?std_detect::")),
)

#: What one Callgrind output counted: the program's own instructions, the
#: `DISPATCHED` share of the program object, and every other object's.
Split = namedtuple("Split", "program dispatched other")

# Each name kind Callgrind compresses, by the specification that names it;
# the call-side spellings share their table with the cost side's.
NAME_KINDS = {
    "ob": "object",
    "cob": "object",
    "fl": "file",
    "fi": "file",
    "fe": "file",
    "cfi": "file",
    "cfl": "file",
    "fn": "function",
    "cfn": "function",
}


def split_by_object(text, program):
    """Instructions per object in a Callgrind output file, as a `Split`.

    `program` is the benchmark executable's path; every other object is summed
    into `other`, and the program's own lines that belong to a `DISPATCHED`
    entry, by their source file or their function, into `dispatched` rather
    than `program`. Only self cost is counted: the line after a `calls=` line is
    the inclusive cost of that call, already counted where the callee's own
    lines are, so reading it would count the callee twice.

    Callgrind compresses a repeated name to `(id)` after its first spelling as
    `(id) name`, one table per kind: `cob=` shares the objects' with `ob=`,
    `cfn=` the functions' with `fn=`, and `fi=`, `fe=`, `cfi=` and `cfl=` the
    files' with `fl=`. Only `ob=` decides where a cost line's object is, `fl=`,
    `fi=` and `fe=` its file, and `fn=` its function, which also returns the
    file to the `fl=` one.

    Only `positions: line` is read. A position column per `--dump-instr` or
    `--dump-line` setting moves the first cost to a later field, and a file
    read at the wrong offset still yields plausible integers, so any other
    header is refused rather than guessed at.
    """
    names = {}
    current = {"object": None, "file": None, "function": None}
    source = None
    events = None
    positions = None
    counts = {}
    dispatched = 0
    after_call = False

    for line in text.splitlines():
        if line.startswith("events:"):
            events = line.split()[1:]
            continue
        if line.startswith("positions:"):
            positions = line.split()[1:]
            continue
        match = re.match(r"^(c?ob|fl|fi|fe|cfi|cfl|c?fn)=(?:\((\d+)\))?\s*(.*)$", line)
        if match:
            spelling, index, name = match.groups()
            kind = NAME_KINDS[spelling]
            if index is not None and name:
                names[kind, index] = name
            resolved = names.get((kind, index), name) if index is not None else name
            if spelling in ("ob", "fl", "fn"):
                current[kind] = resolved
            if spelling in ("fl", "fn"):
                source = current["file"]
            elif spelling in ("fi", "fe"):
                source = resolved
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
                cost = int(fields[1])
                obj = current["object"]
                counts[obj] = counts.get(obj, 0) + cost
                if obj == program and dispatcher(source, current["function"]):
                    dispatched += cost

    if events is None or events[:1] != ["Ir"]:
        raise Unreadable("a Callgrind output with no `events: Ir ...` header")
    if positions not in (None, ["line"]):
        raise Unreadable(
            f"a Callgrind output with `positions: {' '.join(positions)}`; "
            "only `line` is read"
        )

    program_count = sum(count for obj, count in counts.items() if obj == program)
    other = sum(count for obj, count in counts.items() if obj != program)
    return Split(program_count - dispatched, dispatched, other)


def dispatcher(source, function):
    """The name of the `DISPATCHED` entry a cost line in `source`, inside
    `function`, belongs to, or `None` where it is the program's own.

    Either names it: a function is its dependency's wherever a line of it was
    inlined from, a system header's `memcpy` included, and a line of a C
    dependency's static function, which carries no prefix, is by the file it
    was compiled from.
    """
    for entry in DISPATCHED:
        if function is not None and entry.function and entry.function.match(function):
            return entry.name
        if source is not None and entry.file and entry.file.search(source):
            return entry.name
    return None


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


def summaries(directory):
    """Every gungraun `summary.json` under `directory`, in path order.

    Each as `(summary, root, output)`: the parsed summary, the `project_root`
    it names and its output directory under that root. Paths in a summary are
    relative to that root, which is where gungraun ran rather than where this
    script lives.
    """
    for path in sorted(directory.rglob("summary.json")):
        summary = json.loads(path.read_text())
        root = Path(summary["project_root"])
        yield summary, root, root / summary["output_dir"]


def measure(directory):
    """Every benchmark under `directory`, as rows sorted by benchmark name."""
    rows = []
    for summary, root, output in summaries(directory):
        callgrind = output / f"callgrind.{summary['function_name']}.{summary['id']}.out"
        program = str(root / summary["benchmark_exe"])
        instructions, dispatched, other = split_by_object(callgrind.read_text(), program)
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
                scenario_of(summary),
                dispatched,
            )
        )
    return rows


def scenario_of(summary):
    """The scenario a benchmark was set up with: the first string literal of
    the `setup(...)` call gungraun records as its `description`."""
    found = re.search(r'"([^"]+)"', summary.get("description") or "")
    return found.group(1) if found else None


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


def read_trend(text):
    """The groups a baseline's `# trend:` line names, which are never compared."""
    for line in text.splitlines():
        if line.startswith("# trend:"):
            return frozenset(line.removeprefix("# trend:").split())
    return frozenset()


def write_baseline(rows, toolchain, host, trend=frozenset()):
    """`rows` in the baseline's format, carrying the `trend` groups over so a
    recording keeps the declaration it was compared under."""
    lines = [
        "# Written by `mise run profile:record`; read by `mise run profile:requests`.",
        f"# toolchain: {toolchain}",
        f"# host: {host}",
    ]
    if trend:
        lines.append(f"# trend: {' '.join(sorted(trend))}")
    lines.append("\t".join(COLUMNS))
    lines += [
        f"{row.benchmark}\t{row.program}\t{row.blocks}\t{row.bytes}" for row in rows
    ]
    return "\n".join(lines) + "\n"


def in_trend(benchmark, trend):
    return benchmark.split(".", 1)[0] in trend


def counts(row):
    """What is compared of a row: everything but the uncompared libc and
    dispatched counts."""
    return (row.program, row.blocks, row.bytes)


def inexact(runs, trend=frozenset()):
    """Each benchmark outside a `trend` group whose counts differ between runs.

    `runs` is one row list per run. A benchmark missing from a run is a
    difference too: a run that measured less is not the same measurement.
    """
    seen = {}
    for run in runs:
        for row in run:
            seen.setdefault(row.benchmark, []).append(counts(row))
    problems = []
    for benchmark, measured in sorted(seen.items()):
        if in_trend(benchmark, trend):
            continue
        if len(measured) != len(runs) or len(set(measured)) > 1:
            problems.append(
                f"{benchmark}: (instructions, blocks, bytes) across "
                f"{len(runs)} runs were {measured}"
            )
    return problems


def exceeded(rows, recorded, trend=frozenset()):
    """Each row outside a `trend` group whose counts pass the ceiling its
    `recorded` counts set, each exactly; a row with nothing recorded has none."""
    problems = []
    for row in rows:
        if in_trend(row.benchmark, trend) or row.benchmark not in recorded:
            continue
        instructions, blocks, octets = recorded[row.benchmark]
        over = []
        if row.program > instructions:
            over.append(
                f"{row.program} program instructions, recorded and ceiling "
                f"{instructions}"
            )
        if row.blocks > blocks:
            over.append(f"{row.blocks} heap blocks, recorded and ceiling {blocks}")
        if row.bytes > octets:
            over.append(f"{row.bytes} heap bytes, recorded and ceiling {octets}")
        if over:
            problems.append(f"{row.benchmark}: " + "; ".join(over))
    return problems


def read_catalog(text):
    """`catalog`'s output, as {scenario: base}, with `None` for no base."""
    bases = {}
    for line in text.splitlines():
        if not line:
            continue
        name, base, _driver = line.split("\t")
        bases[name] = base or None
    return bases


def marginal(rows, bases):
    """(row, base row) for each row whose scenario names a base that was
    measured, in `rows`' order."""
    by_scenario = {row.scenario: row for row in rows if row.scenario}
    pairs = []
    for row in rows:
        base = by_scenario.get(bases.get(row.scenario))
        if base is not None:
            pairs.append((row, base))
    return pairs


def delta(now, then):
    if then is None:
        return "new"
    if now == then:
        return "="
    return f"{now - then:+d}"


def report(rows, recorded, host=None, recorded_host=None, trend=frozenset(), bases=None):
    """The Markdown a person reads: one row per benchmark, and what moved; then
    each row with a base, and what its one variable costs."""
    lines = []
    if recorded_host is not None and host is not None and host != recorded_host:
        lines += [
            f"Measured on `{host}`; the baseline was recorded on `{recorded_host}`. "
            "The libc and dispatched columns are each host's own; the "
            "compared counts are not.",
            "",
        ]
    header = (
        "Benchmark",
        "Instructions (program)",
        "Δ",
        "Instructions (libc, not compared)",
        "Instructions (dispatched, not compared)",
        "Heap blocks",
        "Δ",
        "Heap bytes",
        "Δ",
    )
    lines += ["| " + " | ".join(header) + " |", "| --- |" + " ---: |" * 8]
    for row in rows:
        if in_trend(row.benchmark, trend):
            moved = ("trend", "trend", "trend")
        else:
            then = recorded.get(row.benchmark, (None, None, None))
            moved = tuple(delta(now, was) for now, was in zip(counts(row), then))
        cells = (
            f"`{row.benchmark}`",
            row.program,
            moved[0],
            row.other,
            row.dispatched,
            row.blocks,
            moved[1],
            row.bytes,
            moved[2],
        )
        lines.append("| " + " | ".join(str(cell) for cell in cells) + " |")

    pairs = marginal(rows, bases or {})
    if pairs:
        lines += [
            "",
            "Marginal cost: each row against the base it differs from in one "
            "variable. `~` marks a pair with a side in a `trend` group.",
            "",
            "| Benchmark | Base | Δ instructions | Δ heap blocks | Δ heap bytes |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
        for row, base in pairs:
            trended = in_trend(row.benchmark, trend) or in_trend(base.benchmark, trend)
            approximate = "~" if trended else ""
            cells = [f"`{row.benchmark}`", f"`{base.benchmark}`"] + [
                approximate + delta(now, was) for now, was in zip(counts(row), counts(base))
            ]
            lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def inclusive_frames(text, root="", program="", limit=FRAMES):
    """The top `limit` frames of `callgrind_annotate --inclusive=yes --show=Ir`
    output, as [(instructions, frame)].

    A frame counted at or above the region's total is omitted: those are the
    benchmark's callers, which hold the whole region, and recursive frames,
    which Callgrind counts once per level. Paths are cut to what names them --
    `root`, the project root, and the toolchain's and registry's source
    directories -- and the `program` object's suffix is dropped, leaving a
    shared object's; a frame that then repeats one above it is listed once.
    """
    total = None
    frames = []
    seen = set()
    for line in text.splitlines():
        found = re.match(r"^\s*([\d,]+)\s+\(\s*[\d.]+%\)\s+(.+?)\s*$", line)
        if found is None:
            continue
        count, frame = int(found.group(1).replace(",", "")), found.group(2)
        if frame == "PROGRAM TOTALS":
            total = count
            continue
        if total is None or count >= total:
            continue
        if root:
            frame = frame.replace(root.rstrip("/") + "/", "")
        frame = re.sub(r"^\S*?/lib/rustlib/src/rust/", "", frame)
        frame = re.sub(r"^\S*?/registry/src/[^/]+/", "", frame)
        frame = re.sub(r" \[(?:[^\]]*/)?([^\]/]+)\]$", r" [\1]", frame)
        if program:
            frame = frame.removesuffix(f" [{program}]")
        if frame not in seen:
            seen.add(frame)
            frames.append((count, frame))
    if total is None:
        raise Unreadable("a `callgrind_annotate` output with no `PROGRAM TOTALS` line")
    return frames[:limit]


def allocation_sites(profile, function, bench):
    """DHAT's allocations inside one benchmark's region, as ([(blocks, bytes,
    frame)] by bytes, largest first, (blocks, bytes) attributed).

    `profile` is a parsed `dhat.*.out`, which covers the whole process. An
    allocation is in the region when its stack holds the benchmark's
    `<function>::__run_<bench>` frame, which is how gungraun finds it too. Each
    is attributed to its innermost frame in a workspace crate, or `None` where
    no frame is one.
    """
    region = re.compile(rf"\b{re.escape(function)}::__run_{re.escape(bench)} \(")
    table = profile["ftbl"]
    marks = {index for index, frame in enumerate(table) if region.search(frame)}
    sites = {}
    for point in profile["pps"]:
        if not marks.intersection(point["fs"]):
            continue
        site = None
        for index in point["fs"]:
            name = table[index].split(": ", 1)[-1]
            if WORKSPACE_FRAME.match(name):
                site = name
                break
        blocks, octets = sites.get(site, (0, 0))
        sites[site] = (blocks + point["tbk"], octets + point["tb"])
    ranked = sorted(
        ((blocks, octets, site) for site, (blocks, octets) in sites.items()),
        key=lambda entry: (-entry[1], -entry[0], entry[2] or ""),
    )
    attributed = (sum(entry[0] for entry in ranked), sum(entry[1] for entry in ranked))
    return ranked, attributed


def cell(text):
    """`text` as one Markdown table cell's code span."""
    return "`" + text.replace("|", "\\|").replace("`", "'") + "`"


def attribution(entries):
    """The attribution file: per benchmark, `(benchmark, scenario, frames,
    (sites, attributed), (blocks, bytes) DHAT counted)`."""
    lines = [
        "# Request-path attribution",
        "",
        f"Per benchmark: the top {FRAMES} frames by inclusive instructions "
        "(`callgrind_annotate --inclusive=yes`, program and libc, the region's "
        "callers and recursive frames omitted), and DHAT's allocation sites by "
        "the innermost frame in a workspace crate.",
    ]
    for benchmark, scenario, frames, (sites, attributed), counted in entries:
        lines += ["", f"## `{benchmark}`" + (f" (`{scenario}`)" if scenario else ""), ""]
        lines += ["| Instructions | Frame |", "| ---: | --- |"]
        lines += [f"| {count} | {cell(frame)} |" for count, frame in frames]
        lines += [
            "",
            f"{attributed[0]} of {counted[0]} blocks and {attributed[1]} of "
            f"{counted[1]} bytes attributed.",
            "",
            "| Blocks | Bytes | Innermost workspace frame |",
            "| ---: | ---: | --- |",
        ]
        lines += [
            f"| {blocks} | {octets} | {cell(site) if site else 'none'} |"
            for blocks, octets, site in sites
        ]
    return "\n".join(lines) + "\n"


def attribute(directory, annotate):
    """Every benchmark under `directory`, as `attribution`'s entries.

    `annotate` turns a Callgrind output path into `callgrind_annotate`'s text.
    """
    entries = []
    for summary, root, output_dir in summaries(directory):
        function, bench = summary["function_name"], summary["id"]
        frames = inclusive_frames(
            annotate(output_dir / f"callgrind.{function}.{bench}.out"),
            str(root),
            Path(summary["benchmark_exe"]).name,
        )
        profile = json.loads((output_dir / f"dhat.{function}.{bench}.out").read_text())
        entries.append(
            (
                f"{summary['group']}.{bench}",
                scenario_of(summary),
                frames,
                allocation_sites(profile, function, bench),
                dhat(summary),
            )
        )
    return entries


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
    leaves its summary behind, and `measure` would read it. An ambient flag
    builds something else -- `-C target-cpu=native` with AVX-512 builds a
    binary Valgrind cannot decode -- so each of `AMBIENT_FLAGS` is refused.
    """
    ambient = [name for name in AMBIENT_FLAGS if os.environ.get(name)]
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


def catalog():
    """The `catalog` binary's output: every scenario, its base and its driver.

    Built at the bench profile, so it reuses what the benchmark built.
    """
    return output(
        ["cargo", "run", "--quiet", "--profile", "bench", "-p", "kynos-profile", "--bin", "catalog"]
    )


def foreign_features(tree):
    """Which of `FOREIGN_FEATURES` `cargo tree -e features -i serde_json` shows
    the harness build enabling."""
    return [feature for feature in FOREIGN_FEATURES if f'serde_json feature "{feature}"' in tree]


def serde_json_features():
    """`serde_json`'s features in the harness build, as `cargo tree` draws them."""
    return output(
        ["cargo", "tree", "-p", "kynos-profile", "-e", "features", "-i", "serde_json"]
    )


def refuse_foreign_build():
    """Raises `Unreadable`, which `--run` exits `NOTHING` on, when the harness
    build enables a `FOREIGN_FEATURES` entry."""
    if foreign := foreign_features(serde_json_features()):
        raise Unreadable(
            f"the harness builds serde_json with {', '.join(foreign)}, which "
            "no application's request path is assumed to; see "
            "crates/kynos-profile/Cargo.toml"
        )


def callgrind_annotate(path):
    """`callgrind_annotate`'s inclusive ranking of `path`, under the pinned Valgrind."""
    annotate = shutil.which("callgrind_annotate", path=search_path())
    if annotate is None:
        raise Unreadable("no `callgrind_annotate` beside the Valgrind this runs under")
    return output(
        [annotate, "--inclusive=yes", "--show=Ir", "--threshold=100", "--auto=no", str(path)]
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
    earlier=(),
    bases=None,
    attribution_path=None,
    annotate=None,
):
    """Reads the run under `directory` and reports it.

    `earlier` holds the rows of the runs before it under `--repeat`, `bases`
    the catalog's `{scenario: base}`, and `annotate` what writes the
    attribution to `attribution_path`; each is skipped when absent.
    """
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
    baseline_text = baseline.read_text() if baseline.is_file() else ""
    recorded, recorded_host = read_baseline(baseline_text)
    trend = read_trend(baseline_text)
    text = report(rows, recorded, host, recorded_host, trend, bases)
    report_path.write_text(text)
    print(text)
    if attribution_path is not None and annotate is not None:
        try:
            attribution_path.write_text(attribution(attribute(directory, annotate)))
        except (Unreadable, OSError, KeyError, ValueError, subprocess.CalledProcessError) as error:
            # Attribution explains a measurement; it never decides its outcome.
            # A previous run's file is removed rather than left to pass for this one's.
            print(f"profile: nothing could be attributed: {error}", file=sys.stderr)
            attribution_path.unlink(missing_ok=True)

    if problems:
        print(
            "profile: DHAT and alloc_counter disagree about the same request:\n    "
            + "\n    ".join(problems),
            file=sys.stderr,
        )
        return DISAGREE
    if variance := inexact([*earlier, rows], trend) if earlier else []:
        print(
            "profile: a count differs between runs of a group not marked `trend`:\n    "
            + "\n    ".join(variance),
            file=sys.stderr,
        )
        return INEXACT
    try:
        toolchain = toolchain or output(["rustc", "-V"])
    except (subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the toolchain could not be named: {error}", file=sys.stderr)
        return NOTHING
    measured = write_baseline(rows, toolchain, host or "unrecorded host", trend)
    run_path.write_text(measured)
    if mode == "overwrite":
        baseline.write_text(measured)
        return MEASURED
    if over := exceeded(rows, recorded, trend):
        print(
            "profile: a count exceeds the ceiling requests.tsv records:\n    "
            + "\n    ".join(over)
            + "\nWhere the rise is intended, record CI's profile-requests.tsv "
            "over requests.tsv in a commit of its own.",
            file=sys.stderr,
        )
        return EXCEEDED
    return MEASURED


def cli(argv):
    """The command line over `argv`, returning the exit code `main` decides.

    Every runner is looked up on this module when called, so a test replaces
    `run_benchmarks`, `measure` and `main` to count runs and read `earlier`.
    """
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--run", action="store_true", help="run the benchmark before reading its output"
    )
    parser.add_argument(
        "--repeat",
        type=int,
        default=1,
        metavar="N",
        help="with --run, run it N times and fail when an exact group's counts differ",
    )
    arguments = parser.parse_args(argv)
    if arguments.repeat < 1 or (arguments.repeat > 1 and not arguments.run):
        parser.error("--repeat takes a count of at least 1, and only with --run")
    earlier = []
    try:
        target = target_directory()
        directory = target / "gungraun/kynos-profile"
        if arguments.run:
            refuse_foreign_build()
        for attempt in range(arguments.repeat if arguments.run else 0):
            run_benchmarks(target)
            if attempt + 1 < arguments.repeat:
                earlier.append(measure(directory))
        bases = read_catalog(catalog())
    except (Unreadable, subprocess.CalledProcessError, OSError, KeyError, ValueError) as error:
        print(f"profile: the benchmark could not be run: {error}", file=sys.stderr)
        return NOTHING
    try:
        host = describe_host()
    except (subprocess.CalledProcessError, OSError) as error:
        print(f"profile: the host could not be described: {error}", file=sys.stderr)
        return NOTHING
    return main(
        directory=directory,
        host=host,
        earlier=earlier,
        bases=bases,
        attribution_path=target / ATTRIBUTION,
        annotate=callgrind_annotate,
    )


if __name__ == "__main__":
    sys.exit(cli(sys.argv[1:]))
