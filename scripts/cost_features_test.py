"""Tests for the pure half of `cost_features.py`: its parsers and its report.

Everything under test here reads text and returns a number or a string. The
half that shells out to `cargo` is deliberately absent: a sweep is fifty builds
and testing it would test cargo, while a parser regression is silent -- it
yields a plausible wrong attribution and exits zero -- and `DISAMBIGUATOR` has
already regressed once, in `1cf9bd6`, where a pattern meant for a crate id also
ate a float slice and collapsed two functions onto one key.

`LLVM_LINES_SAMPLE` is captured output rather than invented: `cargo llvm-lines
--package kynos --example cost_fixture --color never --sort lines
--no-default-features --features openapi31` at 0.4.48, whose `(TOTAL)` is the
`openapi31` row of the committed `crates/kynos/cost/codegen.tsv`. Its column
widths, its two parenthesised shares per column and its bare integer total are
the tool's, not a guess at them. The float-slice row is the one line here that
is constructed, and it is marked as such: no target in this workspace
instantiates a float-slice generic today, which is exactly why that defect was
a wrong number waiting rather than a wrong number.

Run it as `mise run cost:test`, or directly. There is no Python test runner in
this repository and `unittest` needs none.
"""

import sys
import tempfile
import unittest
from pathlib import Path

# Before the import below, and before anything else can trigger one: a `.pyc`
# written beside the scripts would be an untracked directory in every working
# tree that ran these tests, and `.gitignore` has no entry for one. The task
# passes `-B` for the same reason; this covers a direct `python3` run.
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))

import cost_features as cost  # noqa: E402  (the path insert must come first)

# Captured from the run described in the module docstring: four of its 1590
# rows, chosen for what each one exercises, none of them altered.
LLVM_LINES_SAMPLE = """\
  Lines                Copies              Function name
  -----                ------              -------------
  50217                1590                (TOTAL)
   1608 (3.2%,  3.2%)     1 (0.1%,  0.1%)  <matchit[97f224fc0a92797f]::tree::Node<usize>>::insert
    918 (1.8%,  5.0%)     1 (0.1%,  0.1%)  <kynos[a28856ce07048254]::router::Router<(), kynos[a28856ce07048254]::middleware::catch_panic::Propagate, (), kynos[a28856ce07048254]::middleware::stack::Cons<kynos[a28856ce07048254]::middleware::limits::BodySize, ()>>>::describe
    407 (0.8%, 11.6%)     1 (0.1%,  0.5%)  core[37f591cfbe66b0b1]::str::pattern::simd_contains
      1 (0.0%,100.0%)     1 (0.1%, 99.9%)  hashbrown[30cec78cb8082ff]::util::cold_path
"""

# The same `describe`, as a second point of the sweep prints it: same program,
# different enabled features, therefore a different `StableCrateId`.
DESCRIBE_AT_ANOTHER_POINT = """\
  Lines                Copies              Function name
  -----                ------              -------------
  50318                1590                (TOTAL)
    918 (1.8%,  5.0%)     1 (0.1%,  0.1%)  <kynos[7c2f01aa9b3d5e60]::router::Router<(), kynos[7c2f01aa9b3d5e60]::middleware::catch_panic::Propagate, (), kynos[7c2f01aa9b3d5e60]::middleware::stack::Cons<kynos[7c2f01aa9b3d5e60]::middleware::limits::BodySize, ()>>>::describe
"""

# Constructed, not captured -- see the module docstring.
FLOAT_SLICES = """\
  Lines                Copies              Function name
  -----                ------              -------------
    120                   2                (TOTAL)
     70 (58.3%, 58.3%)     1 (50.0%, 50.0%)  <[f64] as core[37f591cfbe66b0b1]::fmt::Debug>::fmt
     50 (41.7%,100.0%)     1 (50.0%,100.0%)  <[f32] as core[37f591cfbe66b0b1]::fmt::Debug>::fmt
"""

DESCRIBE = (
    "<kynos::router::Router<(), kynos::middleware::catch_panic::Propagate, (),"
    " kynos::middleware::stack::Cons<kynos::middleware::limits::BodySize,"
    " ()>>>::describe"
)

RECORDED_TOOLCHAIN = "1.97.1 (8bab26f4f 2026-07-14)"
RECORDED_HOST = "x86_64-unknown-linux-gnu"
LIVE = (RECORDED_TOOLCHAIN, RECORDED_HOST)
OTHER_TOOLCHAIN = ("1.98.0 (0000000000 2026-09-01)", RECORDED_HOST)


def binary_header(baseline):
    """The real `binary.tsv` prose, so a round-trip crosses the real format."""
    return cost.BINARY_HEADER.format(
        toolchain=RECORDED_TOOLCHAIN, host=RECORDED_HOST, baseline=baseline
    )


def binary_rows(**deltas):
    """`{feature: {text, delta}}`, with the baseline point always present."""
    base = 865004
    rows = {cost.BASELINE: {"text": base, "delta": 0}}
    for label, delta in deltas.items():
        rows[label.replace("_", "-")] = {"text": base + delta, "delta": delta}
    return rows


def codegen_rows(**deltas):
    """`{feature: {lines, copies, delta_lines, delta_copies}}`, baseline first."""
    base = 50217
    rows = {
        cost.BASELINE: {
            "lines": base,
            "copies": 1590,
            "delta_lines": 0,
            "delta_copies": 0,
        }
    }
    for label, delta in deltas.items():
        rows[label.replace("_", "-")] = {
            "lines": base + delta,
            "copies": 1590,
            "delta_lines": delta,
            "delta_copies": 0,
        }
    return rows


def codegen_functions(rows):
    """One monomorphization per point, sized so its delta explains the row."""
    return {
        label: {DESCRIBE: (918 + values["delta_lines"], 1)}
        for label, values in rows.items()
    }


def written(directory, name, rows, names, header):
    """A baseline written by `write_tsv` and read back by `read_recorded`.

    Through the file rather than around it, so the round-trip crosses the same
    prose header, the same tab separator and the same integer parse the
    committed baselines do.
    """
    path = Path(directory) / name
    cost.write_tsv(path, header, names, rows)
    return cost.read_recorded(path)


def recorded_binary(directory, rows):
    """`binary.tsv` as this run's baseline, recorded by `RECORDED_TOOLCHAIN`."""
    return written(
        directory, cost.BINARY_TSV, rows, ["text", "delta"], binary_header(865004)
    )


def recorded_codegen(directory, rows):
    """`codegen.tsv` as this run's baseline, recorded by the same toolchain."""
    return written(
        directory,
        cost.CODEGEN_TSV,
        rows,
        ["lines", "copies", "delta_lines", "delta_copies"],
        cost.CODEGEN_HEADER.format(
            toolchain=RECORDED_TOOLCHAIN, host=RECORDED_HOST, baseline=50217
        ),
    )


class ParseLlvmLines(unittest.TestCase):
    """`parse_llvm_lines` over what the tool actually prints."""

    def test_the_total_is_the_bare_integer_pair(self):
        total, _ = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertEqual(total, (50217, 1590))

    def test_the_total_is_not_also_a_function(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertNotIn("(TOTAL)", functions)

    def test_the_column_headers_are_not_rows(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertEqual(len(functions), 4)

    def test_a_share_is_not_taken_for_a_count(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertEqual(functions["<matchit::tree::Node<usize>>::insert"], (1608, 1))

    def test_a_name_keeps_its_spaces_commas_and_angle_brackets(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertEqual(functions[DESCRIBE], (918, 1))

    def test_output_with_no_total_is_reported_rather_than_guessed(self):
        total, functions = cost.parse_llvm_lines(
            "  Lines                Copies              Function name\n"
            "   407 (0.8%, 11.6%)     1 (0.1%,  0.5%)  core::str::pattern::x\n"
        )
        self.assertIsNone(total)
        self.assertEqual(len(functions), 1)


class Disambiguator(unittest.TestCase):
    """The crate-id stripping, and the two bounds that keep it off a slice."""

    def test_a_sixteen_digit_crate_id_goes(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertIn("core::str::pattern::simd_contains", functions)

    def test_a_fifteen_digit_crate_id_goes_too(self):
        _, functions = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        self.assertIn("hashbrown::util::cold_path", functions)

    def test_one_function_keys_the_same_at_two_points_of_the_sweep(self):
        _, here = cost.parse_llvm_lines(LLVM_LINES_SAMPLE)
        _, there = cost.parse_llvm_lines(DESCRIBE_AT_ANOTHER_POINT)
        self.assertEqual(set(there), {DESCRIBE})
        self.assertEqual(here[DESCRIBE], there[DESCRIBE])

    def test_a_float_slice_is_not_a_crate_id(self):
        _, functions = cost.parse_llvm_lines(FLOAT_SLICES)
        self.assertEqual(
            sorted(functions),
            [
                "<[f32] as core::fmt::Debug>::fmt",
                "<[f64] as core::fmt::Debug>::fmt",
            ],
        )


class ReadRecorded(unittest.TestCase):
    """`write_tsv` out, `read_recorded` back, and the header in between."""

    def test_every_number_survives_the_round_trip(self):
        rows = binary_rows(openapi32=75920, uuid=-32, cookie=0)
        with tempfile.TemporaryDirectory() as directory:
            back = recorded_binary(directory, rows)
        self.assertEqual(back.rows, rows)

    def test_the_four_codegen_columns_survive_it_too(self):
        rows = codegen_rows(openapi32=101, test_util=-3982)
        with tempfile.TemporaryDirectory() as directory:
            back = recorded_codegen(directory, rows)
        self.assertEqual(back.rows, rows)

    def test_the_recording_toolchain_survives_the_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            back = recorded_binary(directory, binary_rows(openapi32=75920))
        self.assertEqual(back.toolchain, RECORDED_TOOLCHAIN)
        self.assertEqual(back.host, RECORDED_HOST)

    def test_a_missing_file_is_no_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(cost.read_recorded(Path(directory) / "absent.tsv"))

    def test_prose_alone_is_no_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baseline.tsv"
            path.write_text(binary_header(865004))
            self.assertIsNone(cost.read_recorded(path))


class Ranking(unittest.TestCase):
    """What the report ranks, and against what."""

    def binary(self, measured, recorded):
        """The report for a binary-only run, against `recorded` or nothing."""
        with tempfile.TemporaryDirectory() as directory:
            was = None if recorded is None else recorded_binary(directory, recorded)
        return cost.report(
            measured, None, None, {cost.BINARY_TSV: was, cost.CODEGEN_TSV: None}, LIVE
        )

    def codegen(self, measured, recorded):
        """The report for a codegen-only run, which is the attributed half."""
        with tempfile.TemporaryDirectory() as directory:
            was = None if recorded is None else recorded_codegen(directory, recorded)
        return cost.report(
            None,
            measured,
            codegen_functions(measured),
            {cost.BINARY_TSV: None, cost.CODEGEN_TSV: was},
            LIVE,
        )

    def test_a_point_that_did_not_move_is_not_ranked(self):
        rows = binary_rows(openapi32=75920, uuid=-32)
        text = self.binary(rows, rows)
        self.assertIn("- none", text)
        self.assertNotIn("- `openapi32`", text)

    def test_a_point_that_moved_is_ranked_by_how_far(self):
        text = self.binary(
            binary_rows(openapi32=76000, uuid=-40),
            binary_rows(openapi32=75920, uuid=-32),
        )
        self.assertIn("- `openapi32` +80", text)
        self.assertIn("- `uuid` -8", text)
        self.assertLess(text.index("- `openapi32`"), text.index("- `uuid`"))

    def test_the_baseline_point_is_never_ranked(self):
        text = self.binary(binary_rows(openapi32=76000), binary_rows(openapi32=75920))
        self.assertNotIn(f"- `{cost.BASELINE}`", text)

    def test_a_first_run_ranks_by_cost_instead(self):
        text = self.binary(binary_rows(openapi32=75920, uuid=-32), None)
        self.assertIn("Largest cost", text)
        self.assertIn("- `openapi32` +75920", text)

    def test_a_feature_the_baseline_never_recorded_is_ranked(self):
        text = self.binary(
            binary_rows(openapi32=75920, brand_new=4096),
            binary_rows(openapi32=75920),
        )
        self.assertIn("- `brand-new` +4096", text)

    def test_a_feature_the_baseline_never_recorded_is_not_ranked_as_drift(self):
        text = self.binary(
            binary_rows(openapi32=75920, brand_new=4096),
            binary_rows(openapi32=75920),
        )
        listed = text.index("- `brand-new` +4096")
        self.assertLess(text.index("Largest drift"), listed)
        self.assertLess(
            text.index("no drift to rank", 0, listed), listed
        )

    def test_a_feature_the_baseline_never_recorded_is_attributed(self):
        measured = codegen_rows(openapi32=101, brand_new=640)
        text = self.codegen(measured, codegen_rows(openapi32=101))
        self.assertIn("##### `brand-new`", text)

    def test_a_new_feature_that_costs_nothing_is_still_listed(self):
        text = self.binary(
            binary_rows(openapi32=75920, brand_new=0), binary_rows(openapi32=75920)
        )
        self.assertIn("- `brand-new` +0", text)


class Provenance(unittest.TestCase):
    """Which compiler the drift column is a difference across."""

    def report(self, versions):
        """One drifting run, against a baseline recorded by `RECORDED_TOOLCHAIN`."""
        with tempfile.TemporaryDirectory() as directory:
            was = recorded_binary(directory, binary_rows(openapi32=75920))
        return cost.report(
            binary_rows(openapi32=76000),
            None,
            None,
            {cost.BINARY_TSV: was, cost.CODEGEN_TSV: None},
            versions,
        )

    def test_the_recording_toolchain_is_stated_beside_the_live_one(self):
        text = self.report(OTHER_TOOLCHAIN)
        self.assertIn(RECORDED_TOOLCHAIN, text)
        self.assertIn(OTHER_TOOLCHAIN[0], text)

    def test_a_drift_across_toolchains_is_flagged(self):
        self.assertIn("mixes toolchains", self.report(OTHER_TOOLCHAIN))

    def test_a_drift_within_one_toolchain_is_not_flagged(self):
        self.assertNotIn("mixes toolchains", self.report(LIVE))


if __name__ == "__main__":
    unittest.main()
