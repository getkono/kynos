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
replaced: the ambient `RUSTFLAGS` refusal, the stale-output removal, and the
host key a report compares across hosts.

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

    def run_main(self, blocks, mode, baseline_text=None, program=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.tree(root, blocks, program)
            alloc = root / "alloc.rs"
            alloc.write_text(Calibration.ALLOC)
            baseline = root / "requests.tsv"
            if baseline_text is not None:
                baseline.write_text(baseline_text)
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(
                io.StringIO()
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
                )
                self.run_written = (
                    (root / "run.tsv").read_text()
                    if (root / "run.tsv").is_file()
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


class RunBenchmarks(unittest.TestCase):
    """`run_benchmarks` with `cargo bench` replaced, so no Valgrind runs."""

    def setUp(self):
        self.environ = mock.patch.dict(report.os.environ, clear=False)
        self.environ.start()
        self.addCleanup(self.environ.stop)
        for name in report.RUSTFLAGS:
            report.os.environ.pop(name, None)
        self.cargo = mock.patch.object(report.subprocess, "run")
        self.run_cargo = self.cargo.start()
        self.addCleanup(self.cargo.stop)

    def test_an_ambient_rustflags_variable_is_refused_before_anything_runs(self):
        for name in report.RUSTFLAGS:
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


if __name__ == "__main__":
    unittest.main()
