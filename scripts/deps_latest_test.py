"""Tests for the pure half of `deps_latest.py`: the graph walk, the parser and
the report.

The half that shells out to `cargo` is left out, since it needs the registry.
A parser regression here is silent: it empties the report and exits zero. So
every line shape `cargo update --dry-run --verbose` printed over this workspace
at Cargo 1.97.1 is asserted, and so is the refusal of a shape it does not know.
`DRY_RUN` is captured output, one line per shape, with the names kept as they
were.

Run as `mise run deps:test`, or directly.
"""

import subprocess
import sys
import unittest
from pathlib import Path

# Before the import below: a run writes no `.pyc` into the tree it reads.
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))

from deps_latest import (  # noqa: E402
    Behind,
    Unparsed,
    annotations,
    parse_update,
    published_graph,
    report,
    run,
)

DRY_RUN = """\
    Updating crates.io index
     Locking 59 packages to latest Rust 1.85 compatible versions
    Updating utf8_iter v1.0.4 -> v1.1.0
    Updating jiff v0.2.39 -> v0.2.46 (available: v1.0.1)
   Unchanged tracing-opentelemetry v0.33.0 (available: v0.34.0)
   Unchanged uuid v1.26.1 (available: v1.29.3, requires Rust 1.89.0)
   Unchanged rcgen v0.14.10 (requires Rust 1.88)
      Adding utf_types v1.0.0
    Removing windows-sys v0.52.0
note: to see how you depend on a package, run `cargo tree --invert <dep>@<ver>`
warning: not updating lockfile due to dry run
"""

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index#"


def package(name, version="1.0.0", member=False):
    source = None if member else "registry+https://github.com/rust-lang/crates.io-index"
    id = f"path+file:///w/{name}#{version}" if member else f"{REGISTRY}{name}@{version}"
    return {"id": id, "name": name, "version": version, "source": source}


def edge(target, *kinds):
    return {"pkg": target["id"], "dep_kinds": [{"kind": k, "target": None} for k in kinds]}


class PublishedGraph(unittest.TestCase):
    def setUp(self):
        self.kynos = package("kynos", member=True)
        self.profile = package("kynos-profile", member=True)
        self.tokio = package("tokio")
        self.mio = package("mio")
        self.cc = package("cc")
        self.trybuild = package("trybuild")
        self.both = package("serde")
        self.gungraun = package("gungraun")
        self.metadata = {
            "packages": [
                self.kynos, self.profile, self.tokio, self.mio, self.cc,
                self.trybuild, self.both, self.gungraun,
            ],
            "resolve": {"nodes": [
                {"id": self.kynos["id"], "deps": [
                    edge(self.tokio, None),
                    edge(self.trybuild, "dev"),
                    edge(self.both, None, "dev"),
                ]},
                {"id": self.profile["id"], "deps": [
                    edge(self.kynos, None), edge(self.gungraun, None),
                ]},
                {"id": self.tokio["id"], "deps": [edge(self.mio, None)]},
                {"id": self.mio["id"], "deps": [edge(self.cc, "build")]},
                {"id": self.cc["id"], "deps": []},
                {"id": self.trybuild["id"], "deps": []},
                {"id": self.both["id"], "deps": []},
                {"id": self.gungraun["id"], "deps": []},
            ]},
        }

    def graph(self):
        return {name for name, _ in published_graph(self.metadata, {"kynos"})}

    def test_transitive_normal_and_build_edges_are_followed(self):
        self.assertTrue({"tokio", "mio", "cc"} <= self.graph())

    def test_a_dev_only_edge_is_not_followed(self):
        self.assertNotIn("trybuild", self.graph())

    def test_an_edge_that_is_dev_and_normal_is_followed(self):
        self.assertIn("serde", self.graph())

    def test_what_only_an_unpublished_member_pulls_in_is_left_out(self):
        self.assertNotIn("gungraun", self.graph())

    def test_members_themselves_are_not_registry_packages(self):
        self.assertNotIn("kynos", self.graph())

    def test_versions_are_kept_so_two_majors_stay_apart(self):
        self.assertIn(("tokio", "1.0.0"), published_graph(self.metadata, {"kynos"}))


class ParseUpdate(unittest.TestCase):
    def test_every_captured_shape(self):
        self.assertEqual(parse_update(DRY_RUN), [
            Behind("compatible", "utf8_iter", "1.0.4", "1.1.0", None),
            Behind("compatible", "jiff", "0.2.39", "0.2.46", None),
            Behind("incompatible", "jiff", "0.2.39", "1.0.1", None),
            Behind("incompatible", "tracing-opentelemetry", "0.33.0", "0.34.0", None),
            Behind("msrv", "uuid", "1.26.1", "1.29.3", "1.89.0"),
        ])

    def test_a_build_metadata_version_is_one_token(self):
        line = "Unchanged wasip2 v1.0.1+wasi-0.2.4 (available: v1.0.4+wasi-0.2.12, requires Rust 1.87.0)"
        self.assertEqual(parse_update(line), [
            Behind("msrv", "wasip2", "1.0.1+wasi-0.2.4", "1.0.4+wasi-0.2.12", "1.87.0"),
        ])

    def test_a_downgrade_is_reported(self):
        self.assertEqual(parse_update("Downgrading foo v1.2.0 -> v1.1.0"), [
            Behind("downgrade", "foo", "1.2.0", "1.1.0", None),
        ])

    def test_a_git_source_being_fetched_is_not_a_package(self):
        self.assertEqual(parse_update("Updating git repository `https://x/y`"), [])

    def test_a_known_verb_in_an_unknown_shape_fails_closed(self):
        for line in (
            "Updating jiff v0.2.39 -> v0.2.46 (latest: v1.0.1)",
            "Unchanged uuid v1.26.1",
            "Downgrading foo 1.2.0 -> 1.1.0",
        ):
            with self.subTest(line=line), self.assertRaises(Unparsed):
                parse_update(line)


class Run(unittest.TestCase):
    # `cargo metadata` prints its index fetch and downloads to stderr on a cold
    # registry, as on CI; only its stdout is the JSON.
    NOISY = [
        sys.executable, "-c",
        "import sys; sys.stderr.write('Downloaded x v1\\n'); print('{}')",
    ]

    def test_stderr_is_kept_out_of_the_output_by_default(self):
        self.assertEqual(run(self.NOISY), "{}\n")

    def test_stderr_is_merged_when_it_carries_the_result(self):
        self.assertEqual(
            run(self.NOISY, stderr=subprocess.STDOUT), "Downloaded x v1\n{}\n"
        )


class Report(unittest.TestCase):
    def test_nothing_behind_says_none(self):
        self.assertTrue(report([]).rstrip().endswith("None."))

    def test_each_kind_gets_its_own_table_and_rows(self):
        text = report(parse_update(DRY_RUN))
        self.assertIn("| `utf8_iter` | 1.0.4 | 1.1.0 |", text)
        self.assertIn("| `uuid` | 1.26.1 | 1.29.3 (Rust 1.89.0) |", text)
        self.assertEqual(text.count("| Crate | Locked | Latest |"), 3)

    def test_one_annotation_per_kind_names_every_crate(self):
        lines = annotations(parse_update(DRY_RUN))
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(line.startswith("::warning title=") for line in lines))
        self.assertIn("(2)::jiff 0.2.39 -> 1.0.1, tracing-opentelemetry", lines[2])


if __name__ == "__main__":
    unittest.main()
