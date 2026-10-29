"""Tests for `profile_report.py`: its Callgrind reader, its calibration and
its baseline.

The half that runs Valgrind is absent, for `cost_features_test.py`'s reason:
testing it would test gungraun. What is tested is where a wrong number would
look right. A cost line read after `calls=` is the callee's inclusive cost, and
counting it double-counts every call; an object mis-resolved through Callgrind's
`(id)` compression moves the program's instructions into the allocator's
column; and a calibration that stopped comparing would pass forever.

`CALLGRIND_SAMPLE` is shaped after a real `callgrind.*.out` from
`mise run profile:requests` -- its header, its compressed names, a `cob=`/`cfn=`
call into libc -- cut down to what each assertion reads. The two lines a
different Callgrind configuration would add, a `positions:` column and a
`jump=` line, are spliced in by the tests that need them.

What runs around Valgrind without needing it is tested with `cargo bench`
replaced: the ambient-flag refusal, the stale-output removal, the
host key a report compares across hosts, and `cli`'s `--repeat`, which runs
the benchmark N times and hands `main` every run but the last.

What a run is read *for* is tested over fixture outputs shaped after real ones:
each row's difference from its base, a count that moves between `--repeat`
runs, the `# trend:` declaration that exempts a group from both comparisons,
and the two attribution parsers -- `callgrind_annotate`'s ranking and DHAT's
allocation sites, where the region is found the way gungraun finds it -- and
`main`'s attribution, which reads each benchmark's own files and whose failure
never replaces the measurement's outcome.

Run it as `mise run profile:test`, or directly.
"""

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import profile_report as report  # noqa: E402

PROGRAM = "/work/target/release/deps/requests-0123"
LIBC = "/usr/lib64/libc.so.6"

CALLGRIND_SAMPLE = f"""# callgrind format
version: 1
creator: callgrind-3.27.1
positions: line
events: Ir Dr Dw
summary: 180 0 0

ob=(1) {PROGRAM}
fl=(1) /work/crates/kynos/src/router/dispatch.rs
fn=(1) <kynos::router::dispatch::Dispatch<()>>::serve::{{closure#0}}
221 25 4 9
+2 5 1 1
cob=(2) {LIBC}
cfi=(2) ???
cfn=(2) malloc
calls=1 0
221 120 10 10
* 10 0 0

ob=(2)
fl=(2)
fn=(2)
0 100 5 5
0 20 1 1

ob=(1)
fl=(1)
fn=(1)
-1 5 0 0
"""


def summary(
    group, bench, blocks, octets, function="request", output_dir=None, root="/work"
):
    return {
        "project_root": str(root),
        "group": group,
        "id": bench,
        "function_name": function,
        "module_path": f"requests::{group}::{function}",
        "benchmark_exe": "target/release/deps/requests-0123",
        "output_dir": output_dir or f"target/gungraun/{group}/{function}.{bench}",
        "profiles": [
            {
                "tool": "Callgrind",
                "data": {"total": {"metrics": {"Ir": {"values": {"new": 1}}}}},
            },
            {
                "tool": "DHAT",
                "data": {
                    "total": {
                        "metrics": {
                            "TotalBlocks": {"values": {"new": blocks}},
                            "TotalBytes": {"values": {"new": octets}},
                        }
                    }
                },
            },
        ],
    }


class SplitByObject(unittest.TestCase):
    def test_a_call_line_is_not_counted_where_it_is_made(self):
        program, other = report.split_by_object(CALLGRIND_SAMPLE, PROGRAM)
        # 25 + 5 + 10 + 5 in the program, the 120 after `calls=` skipped, and
        # libc's own 100 + 20 counted where libc's lines are.
        self.assertEqual((program, other), (45, 120))

    def test_a_compressed_object_name_resolves_to_its_first_spelling(self):
        renamed = CALLGRIND_SAMPLE.replace(
            f"ob=(1) {PROGRAM}", "ob=(1) /elsewhere/requests"
        )
        program, other = report.split_by_object(renamed, PROGRAM)
        self.assertEqual((program, other), (0, 165))

    def test_a_file_that_counts_something_other_than_instructions_is_refused(self):
        with self.assertRaises(report.Unreadable):
            report.split_by_object(
                CALLGRIND_SAMPLE.replace("events: Ir Dr Dw", "events: Dr Dw"), PROGRAM
            )

    def test_a_file_with_an_instruction_address_column_is_refused(self):
        # `--dump-instr=yes` puts an address before the line, so the first
        # cost moves one field right; read at the old offset, it is a line
        # number that looks like a count.
        with self.assertRaises(report.Unreadable):
            report.split_by_object(
                CALLGRIND_SAMPLE.replace("positions: line", "positions: instr line"),
                PROGRAM,
            )

    def test_a_jump_line_is_not_read_as_a_cost(self):
        with_jumps = CALLGRIND_SAMPLE.replace("+2 5 1 1", "jump=3 +4\n+2 5 1 1")
        self.assertEqual(report.split_by_object(with_jumps, PROGRAM), (45, 120))


class Calibration(unittest.TestCase):
    ALLOC = """
const SHAPES: [(&str, StatusCode, usize); 3] = [
    // A static match.
    ("/ping", StatusCode::NO_CONTENT, 7),
    ("/users/7", StatusCode::NO_CONTENT, 11),
    ("/nope", StatusCode::NOT_FOUND, 6),
];

const STACKS: [Stack; 3] = [
    (0, service, STACKED_ALONE),
    (4, depth_4, 11),
    (8, depth_8, 15),
];
"""

    COUNTS = {
        "static_match": 7,
        "capture": 11,
        "miss": 6,
        "stacked_4": 11,
        "stacked_8": 15,
    }

    def rows(self, **overrides):
        counts = dict(self.COUNTS, **overrides)
        return [
            report.Row(f"alloc_counter_agreement.{name}", 1, 0, blocks, 0)
            for name, blocks in counts.items()
        ]

    def test_the_counts_are_read_by_target_and_depth(self):
        self.assertEqual(
            report.recorded_counts(self.ALLOC),
            {
                ("shape", "/ping"): 7,
                ("shape", "/users/7"): 11,
                ("shape", "/nope"): 6,
                ("stack", "4"): 11,
                ("stack", "8"): 15,
            },
        )

    def test_every_calibration_benchmark_names_a_row_the_real_file_records(self):
        recorded = report.recorded_counts(report.ALLOC.read_text())
        self.assertEqual(set(report.CALIBRATION.values()) - set(recorded), set())

    def test_a_table_this_cannot_find_is_a_failure_rather_than_no_counts(self):
        with self.assertRaises(report.Unreadable):
            report.recorded_counts(self.ALLOC.replace("const STACKS", "const DEPTHS"))

    def test_reordering_the_table_pairs_nothing_differently(self):
        ping = '    ("/ping", StatusCode::NO_CONTENT, 7),\n'
        users = '    ("/users/7", StatusCode::NO_CONTENT, 11),\n'
        shuffled = self.ALLOC.replace(ping + users, users + ping)
        self.assertNotEqual(shuffled, self.ALLOC)
        self.assertEqual(
            report.disagreements(self.rows(), report.recorded_counts(shuffled)), []
        )

    def test_agreement_is_silent_and_disagreement_names_the_benchmark(self):
        recorded = report.recorded_counts(self.ALLOC)
        self.assertEqual(report.disagreements(self.rows(), recorded), [])

        problems = report.disagreements(self.rows(stacked_8=8), recorded)
        self.assertEqual(len(problems), 1)
        self.assertIn("stacked_8", problems[0])

    def test_a_count_below_the_ceiling_is_a_disagreement_too(self):
        # The direction `alloc.rs`'s `<=` cannot see: an allocation removed
        # without its ceiling lowered.
        problems = report.disagreements(
            self.rows(static_match=6), report.recorded_counts(self.ALLOC)
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("lower that ceiling", problems[0])

    def test_a_calibration_benchmark_that_did_not_run_is_a_disagreement(self):
        rows = self.rows()[:1]
        self.assertEqual(
            len(report.disagreements(rows, report.recorded_counts(self.ALLOC))), 4
        )

    def test_a_row_the_file_no_longer_records_is_a_disagreement(self):
        recorded = report.recorded_counts(
            self.ALLOC.replace("    (8, depth_8, 15),\n", "")
        )
        problems = report.disagreements(self.rows(), recorded)
        self.assertEqual(len(problems), 1)
        self.assertIn("records no stack", problems[0])


class Baseline(unittest.TestCase):
    def test_a_written_baseline_reads_back_as_what_was_measured_and_where(self):
        rows = [report.Row("scenarios.plaintext", 2017, 3398, 11, 4057)]
        text = report.write_baseline(rows, "rustc 1.97.1", "a CPU, valgrind-3.27.1")
        self.assertEqual(
            report.read_baseline(text),
            ({"scenarios.plaintext": (2017, 11, 4057)}, "a CPU, valgrind-3.27.1"),
        )

    def test_the_uncompared_column_is_not_recorded(self):
        text = report.write_baseline(
            [report.Row("scenarios.plaintext", 2017, 3398, 11, 4057)], "rustc", "host"
        )
        self.assertNotIn("3398", text)

    def test_a_report_marks_what_moved_what_held_and_what_is_new(self):
        rows = [report.Row("a", 10, 0, 2, 3), report.Row("b", 5, 0, 1, 1)]
        text = report.report(rows, {"a": (12, 2, 3)})
        self.assertIn("| `a` | 10 | -2 |", text)
        self.assertIn("| 2 | = |", text)
        self.assertIn("| `b` | 5 | new |", text)

    def test_a_report_taken_on_another_host_says_so(self):
        rows = [report.Row("a", 10, 0, 2, 3)]
        self.assertIn("recorded on `there`", report.report(rows, {}, "here", "there"))
        self.assertNotIn("recorded on", report.report(rows, {}, "here", "here"))

    def test_a_summary_without_dhat_is_unreadable(self):
        broken = summary("scenarios", "plaintext", 1, 1)
        broken["profiles"] = broken["profiles"][:1]
        with self.assertRaises(report.Unreadable):
            report.dhat(broken)


class Main(unittest.TestCase):
    """`main` over a gungraun output tree written to a temporary directory."""

    def tree(self, root, blocks, program=None):
        for name, count in zip(report.CALIBRATION, blocks):
            group, bench = name.split(".")
            output = root / "target/gungraun" / group / f"calibration.{bench}"
            output.mkdir(parents=True)
            (output / "summary.json").write_text(
                json.dumps(
                    summary(
                        group,
                        bench,
                        count,
                        100,
                        "calibration",
                        str(output.relative_to(root)),
                        root,
                    )
                )
            )
            (output / f"callgrind.calibration.{bench}.out").write_text(
                CALLGRIND_SAMPLE.replace(
                    PROGRAM, program or str(root / "target/release/deps/requests-0123")
                )
            )

    def run_main(
        self, blocks, mode, baseline_text=None, program=None, earlier=(), annotate=None
    ):
        """`main` over the tree; with `annotate`, also its attribution, whose
        text (or `None` where none was written) is left in `self.attributed`."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.tree(root, blocks, program)
            attribution_path = None
            if annotate is not None:
                attribution_path = root / "target/gungraun/profile-attribution.md"
                attribution_path.write_text("a previous run's attribution\n")
                for output in (root / "target/gungraun").rglob("summary.json"):
                    bench = json.loads(output.read_text())["id"]
                    (output.parent / f"dhat.calibration.{bench}.out").write_text(
                        json.dumps(DHAT_SAMPLE).replace("request::__run_layers_8 ", f"calibration::__run_{bench} ")
                    )
            alloc = root / "alloc.rs"
            alloc.write_text(Calibration.ALLOC)
            baseline = root / "requests.tsv"
            if baseline_text is not None:
                baseline.write_text(baseline_text)
            self.stderr = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(
                self.stderr
            ):
                code = report.main(
                    mode=mode,
                    directory=root / "target/gungraun",
                    baseline=baseline,
                    alloc=alloc,
                    report_path=root / "report.md",
                    toolchain="rustc test",
                    host="test host",
                    run_path=root / "run.tsv",
                    earlier=earlier,
                    attribution_path=attribution_path,
                    annotate=annotate,
                )
                self.run_written = (
                    (root / "run.tsv").read_text()
                    if (root / "run.tsv").is_file()
                    else None
                )
                self.attributed = (
                    attribution_path.read_text()
                    if attribution_path is not None and attribution_path.is_file()
                    else None
                )
            written = baseline.read_text() if baseline.is_file() else None
            return code, written

    def test_measured_without_a_baseline_exits_zero(self):
        self.assertEqual(self.run_main([7, 11, 6, 11, 15], "")[0], report.MEASURED)

    def test_overwrite_records_what_was_measured_and_the_host(self):
        code, written = self.run_main([7, 11, 6, 11, 15], "overwrite")
        self.assertEqual(code, report.MEASURED)
        self.assertIn("alloc_counter_agreement.stacked_8\t45\t15\t100", written)
        self.assertIn("# host: test host", written)

    def test_a_compared_run_leaves_the_baseline_alone(self):
        _, recorded = self.run_main([7, 11, 6, 11, 15], "overwrite")
        self.assertEqual(
            self.run_main([7, 11, 6, 11, 15], "", recorded), (report.MEASURED, recorded)
        )

    def test_every_measured_run_is_written_in_the_baseline_format(self):
        # What CI uploads, so the baseline can be recorded from its host.
        _, recorded = self.run_main([7, 11, 6, 11, 15], "overwrite")
        self.run_main([7, 11, 6, 11, 15], "", recorded)
        self.assertEqual(self.run_written, recorded)

    def test_the_instruments_disagreeing_fails_in_every_mode(self):
        for mode in ("", "overwrite"):
            with self.subTest(mode=mode):
                code, written = self.run_main([7, 11, 6, 11, 14], mode)
                self.assertEqual(code, report.DISAGREE)
                self.assertIsNone(written)

    def test_output_attributing_nothing_to_the_program_is_a_measurement_not_made(self):
        code, _ = self.run_main([7, 11, 6, 11, 15], "", program="/somewhere/else")
        self.assertEqual(code, report.NOTHING)

    def test_nothing_measured_is_its_own_exit(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stderr(
            io.StringIO()
        ):
            code = report.main(
                mode="", directory=Path(directory), run_path=Path(directory) / "run.tsv"
            )
            self.assertEqual(code, report.NOTHING)

    def test_attribution_reads_each_benchmark_s_own_callgrind_and_dhat_files(self):
        annotated = []

        def annotate(path):
            annotated.append(path)
            return ANNOTATE_SAMPLE

        code, _ = self.run_main([7, 11, 6, 11, 15], "", annotate=annotate)
        self.assertEqual(code, report.MEASURED)
        self.assertEqual(
            sorted(path.name for path in annotated),
            sorted(f"callgrind.calibration.{name.split('.')[1]}.out" for name in report.CALIBRATION),
        )
        for name, blocks in zip(report.CALIBRATION, [7, 11, 6, 11, 15]):
            with self.subTest(benchmark=name):
                self.assertIn(f"## `{name}`", self.attributed)
                self.assertIn(
                    f"4 of {blocks} blocks and 144 of 100 bytes attributed.", self.attributed
                )
        self.assertIn("__memcpy_avx_unaligned_erms", self.attributed)

    def test_an_attribution_failure_keeps_the_measurement_s_own_outcome(self):
        def annotate(path):
            raise report.Unreadable(f"cannot annotate {path.name}")

        code, written = self.run_main([7, 11, 6, 11, 15], "overwrite", annotate=annotate)
        self.assertEqual(code, report.MEASURED)
        self.assertIn("alloc_counter_agreement.stacked_8\t45\t15\t100", written)
        self.assertEqual(self.run_written, written)
        self.assertIn("nothing could be attributed: cannot annotate", self.stderr.getvalue())
        # A previous run's file is never left to pass for this run's.
        self.assertIsNone(self.attributed)
        code, _ = self.run_main([7, 11, 6, 11, 14], "", annotate=annotate)
        self.assertEqual(code, report.DISAGREE)


#: Shaped after `cargo tree -p kynos-profile -e features -i serde_json` with
#: `kynos/test-util` on, cut down: a dependency's edge into a feature names the
#: feature on the line above it.
SERDE_JSON_TREE = """serde_json v1.0.151
├── serde_json feature "default"
│   ├── jsonschema v0.49.9
│   │   └── kynos v0.4.0 (/work/crates/kynos)
│   └── kynos-profile v0.0.0 (/work/crates/kynos-profile)
├── serde_json feature "float_roundtrip"
│   ├── jsonschema v0.49.9 (*)
│   └── jsonschema-value v0.49.9 (*)
└── serde_json feature "std"
    └── serde_json feature "default" (*)
"""


class ForeignFeatures(unittest.TestCase):
    def test_a_feature_test_util_brings_is_named(self):
        self.assertEqual(report.foreign_features(SERDE_JSON_TREE), ["float_roundtrip"])

    def test_a_tree_without_one_names_nothing(self):
        tree = SERDE_JSON_TREE.replace('feature "float_roundtrip"', 'feature "raw_value"')
        self.assertEqual(report.foreign_features(tree), [])

    def test_a_build_enabling_one_is_refused(self):
        with mock.patch.object(report, "serde_json_features", return_value=SERDE_JSON_TREE):
            with self.assertRaisesRegex(report.Unreadable, "float_roundtrip"):
                report.refuse_foreign_build()

    def test_a_build_enabling_none_is_let_through(self):
        tree = SERDE_JSON_TREE.replace('feature "float_roundtrip"', 'feature "raw_value"')
        with mock.patch.object(report, "serde_json_features", return_value=tree):
            self.assertIsNone(report.refuse_foreign_build())


class RunBenchmarks(unittest.TestCase):
    """`run_benchmarks` with `cargo bench` replaced, so no Valgrind runs."""

    def setUp(self):
        self.environ = mock.patch.dict(report.os.environ, clear=False)
        self.environ.start()
        self.addCleanup(self.environ.stop)
        for name in report.AMBIENT_FLAGS:
            report.os.environ.pop(name, None)
        self.cargo = mock.patch.object(report.subprocess, "run")
        self.run_cargo = self.cargo.start()
        self.addCleanup(self.cargo.stop)

    def test_an_ambient_flag_variable_is_refused_before_anything_runs(self):
        # Named rather than read off `AMBIENT_FLAGS`, so a name dropped from
        # that list fails here instead of leaving the loop shorter.
        for name in (
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_BUILD_TARGET",
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                stale = Path(directory) / "gungraun/kynos-profile/stale"
                stale.mkdir(parents=True)
                with mock.patch.dict(report.os.environ, {name: "-C target-cpu=native"}):
                    with self.assertRaisesRegex(report.Unreadable, name):
                        report.run_benchmarks(Path(directory))
                self.run_cargo.assert_not_called()
                self.assertTrue(stale.is_dir())

    def test_an_empty_rustflags_variable_is_not_a_flag(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(
            report.os.environ, {"RUSTFLAGS": ""}
        ):
            report.run_benchmarks(Path(directory))
        self.run_cargo.assert_called_once()

    def test_the_previous_output_is_gone_before_the_benchmark_runs(self):
        # A deleted benchmark's summary would otherwise be read as this run's.
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            output = target / "gungraun/kynos-profile"
            (output / "scenarios/gone.old").mkdir(parents=True)
            (output / "scenarios/gone.old/summary.json").write_text("{}")
            sibling = target / "gungraun/other"
            sibling.mkdir()
            self.run_cargo.side_effect = lambda *_, **__: self.assertFalse(
                output.exists()
            )
            report.run_benchmarks(target)
            self.run_cargo.assert_called_once()
            self.assertTrue(sibling.is_dir())

    def test_the_benchmark_runs_under_the_pinned_valgrind_first(self):
        with tempfile.TemporaryDirectory() as directory:
            report.run_benchmarks(Path(directory))
        environment = self.run_cargo.call_args.kwargs["env"]
        self.assertTrue(
            environment["PATH"].startswith(f"{report.VALGRIND / 'bin'}{report.os.pathsep}")
        )


class DescribeHost(unittest.TestCase):
    """The host key: the CPU model string and the Valgrind that will run."""

    def describe(self, cpuinfo_text, valgrind):
        with tempfile.TemporaryDirectory() as directory:
            cpuinfo = Path(directory) / "cpuinfo"
            if cpuinfo_text is not None:
                cpuinfo.write_text(cpuinfo_text)
            with mock.patch.object(
                report.shutil, "which", return_value=valgrind
            ) as which, mock.patch.object(
                report, "output", return_value="valgrind-3.27.1"
            ) as version:
                host = report.describe_host(cpuinfo)
        return host, which, version

    def test_the_key_is_the_model_name_and_the_valgrind_version(self):
        host, which, version = self.describe(
            "processor\t: 0\nvendor_id\t: AuthenticAMD\n"
            "model name\t: AMD Ryzen 7 7800X3D 8-Core Processor \nflags\t: sse2\n",
            "/opt/valgrind/bin/valgrind",
        )
        self.assertEqual(host, "AMD Ryzen 7 7800X3D 8-Core Processor, valgrind-3.27.1")
        version.assert_called_once_with(["/opt/valgrind/bin/valgrind", "--version"])
        # The Valgrind `profile:valgrind` builds is the one asked, not the distro's.
        self.assertTrue(
            which.call_args.kwargs["path"].startswith(str(report.VALGRIND / "bin"))
        )

    def test_a_host_without_cpuinfo_or_valgrind_is_named_as_such(self):
        host, _, version = self.describe(None, None)
        self.assertEqual(host, "unknown CPU, no valgrind")
        version.assert_not_called()

    def test_cpuinfo_without_a_model_name_is_an_unknown_cpu(self):
        host, _, _ = self.describe("processor\t: 0\n", "/usr/bin/valgrind")
        self.assertEqual(host, "unknown CPU, valgrind-3.27.1")


class Marginal(unittest.TestCase):
    """A row against its base: the one number an ablation is read by."""

    ROWS = [
        report.Row("scenarios.layers_0", 3564, 9, 12, 4607, "layers-0"),
        report.Row("scenarios.layers_4", 4824, 9, 16, 8831, "layers-4"),
        report.Row("runtime.json_small", 4111, 9, 12, 4607, "json-small-runtime"),
        report.Row("scenarios.json_small", 3564, 9, 12, 4607, "json-small"),
    ]
    BASES = {
        "layers-0": None,
        "layers-4": "layers-0",
        "json-small-runtime": "json-small",
        "json-small": None,
    }

    def test_the_catalog_reads_as_each_scenario_and_its_base(self):
        text = "layers-0\t\tPoll\nlayers-4\tlayers-0\tPoll\n"
        self.assertEqual(
            report.read_catalog(text), {"layers-0": None, "layers-4": "layers-0"}
        )

    def test_a_summary_names_the_scenario_its_setup_was_called_with(self):
        given = summary("scenarios", "layers_4", 1, 1)
        given["description"] = '(prepared("layers-4"))'
        self.assertEqual(report.scenario_of(given), "layers-4")
        self.assertIsNone(report.scenario_of(summary("scenarios", "x", 1, 1)))

    def test_each_row_is_paired_with_its_base_by_scenario_name(self):
        pairs = report.marginal(self.ROWS, self.BASES)
        self.assertEqual(
            [(row.benchmark, base.benchmark) for row, base in pairs],
            [
                ("scenarios.layers_4", "scenarios.layers_0"),
                ("runtime.json_small", "scenarios.json_small"),
            ],
        )

    def test_a_base_that_was_not_measured_pairs_nothing(self):
        self.assertEqual(report.marginal(self.ROWS[1:2], self.BASES), [])

    def test_the_report_gives_each_difference_and_marks_a_trend_pair(self):
        text = report.report(self.ROWS, {}, bases=self.BASES, trend={"runtime"})
        self.assertIn(
            "| `scenarios.layers_4` | `scenarios.layers_0` | +1260 | +4 | +4224 |", text
        )
        self.assertIn(
            "| `runtime.json_small` | `scenarios.json_small` | ~+547 | ~= | ~= |", text
        )

    def test_no_base_writes_no_marginal_table(self):
        self.assertNotIn("Marginal cost", report.report(self.ROWS, {}))


class Trend(unittest.TestCase):
    """A group the baseline declares inexact: reported, and compared with nothing."""

    def test_the_declaration_survives_a_recording(self):
        rows = [report.Row("runtime.json_small", 1, 0, 1, 1)]
        text = report.write_baseline(rows, "rustc", "host", frozenset({"runtime"}))
        self.assertIn("# trend: runtime\n", text)
        self.assertEqual(report.read_trend(text), {"runtime"})
        self.assertEqual(report.read_baseline(text)[0], {"runtime.json_small": (1, 1, 1)})

    def test_no_declaration_is_no_trend_and_no_line(self):
        text = report.write_baseline([], "rustc", "host")
        self.assertNotIn("# trend", text)
        self.assertEqual(report.read_trend(text), frozenset())

    def test_a_trend_row_is_never_compared_with_the_baseline(self):
        rows = [report.Row("runtime.json_small", 10, 0, 2, 3)]
        recorded = {"runtime.json_small": (12, 2, 3)}
        self.assertIn("| 10 | trend |", report.report(rows, recorded, trend={"runtime"}))
        self.assertIn("| 10 | -2 |", report.report(rows, recorded))


class Repeat(unittest.TestCase):
    """`--repeat`: a count that moves between runs of one build is not exact."""

    def run_of(self, program, blocks=2, octets=3, other=0):
        return [
            report.Row("scenarios.plaintext", program, other, blocks, octets),
            report.Row("runtime.json_small", program, other, blocks, octets),
        ]

    def test_runs_that_agree_are_exact_whatever_libc_did(self):
        self.assertEqual(report.inexact([self.run_of(5), self.run_of(5, other=9)]), [])

    def test_any_compared_count_that_moves_names_the_benchmark(self):
        for moved in (self.run_of(6), self.run_of(5, blocks=3), self.run_of(5, octets=4)):
            with self.subTest(moved=moved[0]):
                problems = report.inexact([self.run_of(5), moved], frozenset({"runtime"}))
                self.assertEqual(len(problems), 1)
                self.assertIn("scenarios.plaintext", problems[0])

    def test_a_trend_group_may_move(self):
        self.assertEqual(
            report.inexact([self.run_of(5), self.run_of(6)], {"runtime", "scenarios"}),
            [],
        )

    def test_a_benchmark_one_run_did_not_measure_is_a_difference(self):
        problems = report.inexact([self.run_of(5), self.run_of(5)[:1]])
        self.assertEqual(len(problems), 1)
        self.assertIn("runtime.json_small", problems[0])

    def test_main_refuses_to_record_an_inexact_run(self):
        main = Main()
        _, recorded = main.run_main([7, 11, 6, 11, 15], "overwrite")
        earlier = [
            [
                report.Row(name, instructions + 1, 0, blocks, octets)
                for name, (instructions, blocks, octets) in report.read_baseline(recorded)[0].items()
            ]
        ]
        code, written = main.run_main([7, 11, 6, 11, 15], "overwrite", earlier=earlier)
        self.assertEqual((code, written), (report.INEXACT, None))

    def test_main_records_a_run_its_repeats_agree_with(self):
        main = Main()
        _, recorded = main.run_main([7, 11, 6, 11, 15], "overwrite")
        earlier = [
            [
                report.Row(name, instructions, 0, blocks, octets)
                for name, (instructions, blocks, octets) in report.read_baseline(recorded)[0].items()
            ]
        ]
        code, written = main.run_main([7, 11, 6, 11, 15], "overwrite", earlier=earlier)
        self.assertEqual((code, written), (report.MEASURED, recorded))


class Cli(unittest.TestCase):
    """`cli`'s `--repeat` orchestration, with every runner replaced.

    `measure` returns the run's ordinal, so `earlier` shows which runs were
    read and in what order: every run but the last, which `main` reads itself.
    """

    def cli(self, argv, run=None, outcome=report.MEASURED):
        """`cli(argv)`'s exit code, the runners it called, and `main`'s arguments."""
        target = Path("/work/target")
        runs = []
        calls = mock.Mock()
        calls.target_directory.return_value = target
        calls.run_benchmarks.side_effect = run or (lambda _: runs.append(len(runs) + 1))
        calls.measure.side_effect = lambda _: runs[-1]
        calls.catalog.return_value = "layers-8\tlayers-0\trequest\nlayers-0\t\trequest\n"
        calls.describe_host.return_value = "test host"
        calls.main.return_value = outcome
        with contextlib.ExitStack() as stack:
            for name in (
                "target_directory",
                "refuse_foreign_build",
                "run_benchmarks",
                "measure",
                "catalog",
                "describe_host",
                "main",
            ):
                stack.enter_context(mock.patch.object(report, name, getattr(calls, name)))
            stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
            code = report.cli(argv)
        return code, calls

    def test_each_repeat_runs_the_benchmark_and_main_reads_the_last(self):
        code, calls = self.cli(["--run", "--repeat", "3"])
        self.assertEqual(code, report.MEASURED)
        self.assertEqual(calls.run_benchmarks.call_args_list, [mock.call(Path("/work/target"))] * 3)
        self.assertEqual(
            calls.measure.call_args_list,
            [mock.call(Path("/work/target/gungraun/kynos-profile"))] * 2,
        )
        self.assertEqual(calls.main.call_args.kwargs["earlier"], [1, 2])

    def test_one_run_leaves_main_nothing_earlier_to_compare(self):
        _, calls = self.cli(["--run"])
        self.assertEqual(calls.run_benchmarks.call_count, 1)
        self.assertEqual(calls.measure.call_count, 0)
        self.assertEqual(calls.main.call_args.kwargs["earlier"], [])

    def test_main_is_handed_the_run_s_paths_host_catalog_and_annotator(self):
        _, calls = self.cli(["--run"])
        self.assertEqual(
            calls.main.call_args,
            mock.call(
                directory=Path("/work/target/gungraun/kynos-profile"),
                host="test host",
                earlier=[],
                bases={"layers-8": "layers-0", "layers-0": None},
                attribution_path=Path("/work/target") / report.ATTRIBUTION,
                annotate=report.callgrind_annotate,
            ),
        )

    def test_without_run_nothing_is_built_and_the_last_output_is_read(self):
        code, calls = self.cli([])
        self.assertEqual(code, report.MEASURED)
        self.assertEqual(calls.refuse_foreign_build.call_count, 0)
        self.assertEqual(calls.run_benchmarks.call_count, 0)
        self.assertEqual(calls.main.call_args.kwargs["earlier"], [])

    def test_a_foreign_build_is_refused_before_anything_runs(self):
        calls = mock.Mock()
        with mock.patch.object(
            report, "refuse_foreign_build", side_effect=report.Unreadable("foreign")
        ), mock.patch.object(report, "run_benchmarks", calls), mock.patch.object(
            report, "target_directory", return_value=Path("/work/target")
        ), contextlib.redirect_stderr(io.StringIO()) as said:
            code = report.cli(["--run", "--repeat", "2"])
        self.assertEqual(code, report.NOTHING)
        self.assertEqual(calls.call_count, 0)
        self.assertIn("the benchmark could not be run: foreign", said.getvalue())

    def test_a_run_that_fails_stops_the_repeats_with_nothing_measured(self):
        def run(_):
            raise OSError("no valgrind")

        code, calls = self.cli(["--run", "--repeat", "3"], run=run)
        self.assertEqual(code, report.NOTHING)
        self.assertEqual(calls.run_benchmarks.call_count, 1)
        self.assertEqual(calls.main.call_count, 0)

    def test_main_s_outcome_is_the_exit_code(self):
        code, _ = self.cli(["--run", "--repeat", "2"], outcome=report.INEXACT)
        self.assertEqual(code, report.INEXACT)

    def test_a_repeat_without_run_or_below_one_is_a_usage_error(self):
        for argv in (["--repeat", "2"], ["--run", "--repeat", "0"]):
            with self.subTest(argv=argv):
                with contextlib.redirect_stderr(io.StringIO()) as said:
                    with self.assertRaises(SystemExit) as stopped:
                        report.cli(argv)
                self.assertEqual(stopped.exception.code, 2)
                self.assertIn("--repeat takes a count of at least 1", said.getvalue())


#: Shaped after `callgrind_annotate --inclusive=yes --show=Ir` over a real
#: `callgrind.*.out`: the header, the totals line, a caller at the region's
#: total, a recursive frame above it, a frame inlined from the toolchain's
#: sources, the same frame twice once its paths are cut, and libc.
ANNOTATE_SAMPLE = """--------------------------------------------------------------------------------
Profile data file 'callgrind.request.layers_8.out' (creator: callgrind-3.27.1)
--------------------------------------------------------------------------------
Events recorded:  Ir Dr Dw
Events shown:     Ir

--------------------------------------------------------------------------------
Ir
--------------------------------------------------------------------------------
16,964 (100.0%)  PROGRAM TOTALS

--------------------------------------------------------------------------------
Ir               file:function
--------------------------------------------------------------------------------
50,110 (295.4%)  /work/crates/kynos-profile/src/app.rs:<kynos_profile::app::Transparent as kynos::middleware::Interceptor<()>>::intercept::{closure#0} [/work/target/release/deps/requests-0123]
16,964 (100.0%)  ???:main [/work/target/release/deps/requests-0123]
14,791 (87.19%)  /work/crates/kynos/src/router/describe.rs:<kynos::router::Router<()>>::build::{closure#5}::{closure#0} [/work/target/release/deps/requests-0123]
14,790 (87.19%)  /work/crates/kynos/src/router/describe.rs:<kynos::router::Router<()>>::build::{closure#5}::{closure#0}
 6,315 (37.23%)  /home/ci/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/future/future.rs:<kynos::middleware::Next<()>>::run::{closure#0}
 5,889 (34.71%)  ???:__memcpy_avx_unaligned_erms [/usr/lib64/libc.so.6]
 1,378 ( 8.12%)  /home/ci/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde_json-1.0.151/src/ser.rs:serde_json::ser::format_escaped_str
"""


class InclusiveFrames(unittest.TestCase):
    def frames(self, **given):
        return report.inclusive_frames(
            ANNOTATE_SAMPLE, root="/work", program="requests-0123", **given
        )

    def test_the_region_s_callers_and_recursive_frames_are_omitted(self):
        names = [frame for _, frame in self.frames()]
        self.assertFalse(any("Transparent" in name or "main" in name for name in names))

    def test_each_frame_reads_as_the_workspace_names_it_and_is_listed_once(self):
        self.assertEqual(
            self.frames(),
            [
                (14791, "crates/kynos/src/router/describe.rs:<kynos::router::Router<()>>::build::{closure#5}::{closure#0}"),
                (6315, "library/core/src/future/future.rs:<kynos::middleware::Next<()>>::run::{closure#0}"),
                (5889, "???:__memcpy_avx_unaligned_erms [libc.so.6]"),
                (1378, "serde_json-1.0.151/src/ser.rs:serde_json::ser::format_escaped_str"),
            ],
        )

    def test_only_the_top_frames_are_kept(self):
        self.assertEqual([count for count, _ in self.frames(limit=2)], [14791, 6315])

    def test_output_with_no_totals_is_unreadable(self):
        with self.assertRaises(report.Unreadable):
            report.inclusive_frames(ANNOTATE_SAMPLE.replace("PROGRAM TOTALS", "TOTALS"))


#: Shaped after a real `dhat.*.out`, which covers the whole process: one
#: allocation in setup, three in the region -- two under one workspace frame,
#: reached through a `std` frame naming a workspace type, and one under none --
#: and one in a neighbouring benchmark whose id shares a prefix.
DHAT_SAMPLE = {
    "ftbl": [
        "[root]",
        "0x1: alloc (library/std/src/sys/alloc/unix.rs:14)",
        "0x2: new<kynos::router::dispatch::Routed> (boxed.rs:286)",
        "0x3: <kynos::router::dispatch::Dispatch<()>>::serve::{closure#0} (dispatch.rs:293)",
        "0x4: requests::request::__run_layers_8 (requests.rs:83)",
        "0x5: requests::prepared (requests.rs:64)",
        "0x6: requests::request::__run_layers_80 (requests.rs:83)",
        "0x7: <alloc::vec::Vec<u8>>::with_capacity (vec.rs:1)",
    ],
    "pps": [
        {"tb": 4096, "tbk": 1, "fs": [1, 3, 5]},
        {"tb": 104, "tbk": 1, "fs": [1, 2, 3, 4]},
        {"tb": 32, "tbk": 2, "fs": [1, 3, 4]},
        {"tb": 8, "tbk": 1, "fs": [1, 7, 4]},
        {"tb": 900, "tbk": 1, "fs": [1, 3, 6]},
    ],
}


class AllocationSites(unittest.TestCase):
    def test_only_the_region_s_allocations_are_attributed(self):
        _, attributed = report.allocation_sites(DHAT_SAMPLE, "request", "layers_8")
        self.assertEqual(attributed, (4, 144))

    def test_each_is_attributed_to_its_innermost_workspace_frame_largest_first(self):
        sites, _ = report.allocation_sites(DHAT_SAMPLE, "request", "layers_8")
        self.assertEqual(
            sites,
            [
                (3, 136, "<kynos::router::dispatch::Dispatch<()>>::serve::{closure#0} (dispatch.rs:293)"),
                (1, 8, None),
            ],
        )

    def test_another_function_s_benchmark_of_the_same_id_is_not_this_region(self):
        self.assertEqual(
            report.allocation_sites(DHAT_SAMPLE, "sweep", "layers_8"), ([], (0, 0))
        )

    def test_the_file_lists_frames_and_sites_and_says_how_much_was_attributed(self):
        sites = report.allocation_sites(DHAT_SAMPLE, "request", "layers_8")
        text = report.attribution(
            [("scenarios.layers_8", "layers-8", [(10, "a|b")], sites, (5, 150))]
        )
        self.assertIn("## `scenarios.layers_8` (`layers-8`)", text)
        self.assertIn("| 10 | `a\\|b` |", text)
        self.assertIn("4 of 5 blocks and 144 of 150 bytes attributed.", text)
        self.assertIn("| 1 | 8 | none |", text)


if __name__ == "__main__":
    unittest.main()
