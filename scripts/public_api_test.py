"""Tests for what `public_api.py` reads its allowance, its graph and its
listings with, and for the rules it states over them.

A parser that drops a bullet, a crate or a member line leaves the gate green
while a foreign type sits in a signature, so each case below is a mutation the
real tree does not contain. The documents and listings are written here rather
than read off disk, except in `RealDocuments`, which holds the parsers to this
repository's own `architecture.md` so a reworded sentence fails here first.

Run it as `mise run api:test`, or directly. It runs no `cargo`.
"""

import contextlib
import io
import pathlib
import tempfile
import unittest

import public_api as gate

ARCHITECTURE = """\
- No async machinery. The rule is scoped to the checked surface: so
  [`UncheckedService`](x.rs) names a boxed future, and so does
  [`UncheckedInner`](x.rs). That is the shape, and those two are the only ones.

**Public dependencies.** The allowance is exactly these. A crate outside it
is still public:

- `http`, `bytes`, `serde` — the re-export allowance above.
- `tokio` — the `Server::serve_io`
  bound.

Two crates are public API today without being in the allowance, and each has
an issue: `uuid` under `schema/impls/`; and `tracing` as the
`tracing::Level` that `Trace::level` takes.

### The graph

| Layer | Crate | Named in | Fix reaches | Status |
| --- | --- | --- | --- | --- |
| Runtime | `tokio` | `server/` | public API | built |
| Body | `http-body`, `http-body-util` | a | public API (`http-body`); core (`http-body-util`) | built |
| Routing | `matchit` | `router/` | core | built |
| Later | — | — | — | deferred |
"""


def failures_of(function, *args):
    failures = []
    result = function(*args, failures)
    return result, failures


class Allowance(unittest.TestCase):
    def test_every_bullet_is_read_with_its_continuation(self):
        allowed, failures = failures_of(gate.allowance, ARCHITECTURE)
        self.assertEqual(failures, [])
        self.assertEqual(list(allowed), ["http", "bytes", "serde", "tokio"])
        # The continuation line still belongs to the bullet it wraps.
        self.assertEqual(allowed["tokio"], ["Server::serve_io"])

    def test_a_bullet_without_its_dash_is_reported(self):
        text = ARCHITECTURE.replace("`tokio` — the", "`tokio` the")
        _, failures = failures_of(gate.allowance, text)
        self.assertEqual(len(failures), 1)
        self.assertIn("not `crates — reason`", failures[0])

    def test_a_missing_list_is_reported_not_raised(self):
        text = ARCHITECTURE.replace(gate.ALLOWANCE_MARKER, "**Gone.**")
        allowed, failures = failures_of(gate.allowance, text)
        self.assertEqual(allowed, {})
        self.assertIn("no **Public dependencies.** list", failures[0])


class Pending(unittest.TestCase):
    def test_the_crates_are_read_and_paths_are_not(self):
        crates, failures = failures_of(gate.pending, ARCHITECTURE)
        self.assertEqual(failures, [])
        # `schema/impls/` and `tracing::Level` are backticked and no crate.
        self.assertEqual(crates, ["uuid", "tracing"])

    def test_a_count_that_disagrees_is_reported(self):
        text = ARCHITECTURE.replace("Two crates", "Three crates")
        _, failures = failures_of(gate.pending, text)
        self.assertIn("names 2", failures[0])

    def test_one_left_is_read_in_the_singular(self):
        text = ARCHITECTURE.replace(
            "Two crates are public API today without being in the allowance, and each has\n"
            "an issue: `uuid` under `schema/impls/`; and `tracing` as the",
            "One crate is public API today without being in the allowance, and it has\n"
            "an issue: `tracing` as the",
        )
        crates, failures = failures_of(gate.pending, text)
        self.assertEqual((crates, failures), (["tracing"], []))

    def test_none_left_is_a_count_of_zero(self):
        text = ARCHITECTURE.replace(
            "Two crates are public API today without being in the allowance, and each has\n"
            "an issue: `uuid` under `schema/impls/`; and `tracing` as the\n"
            "`tracing::Level` that `Trace::level` takes.",
            "No crates are public API today without being in the allowance.",
        )
        crates, failures = failures_of(gate.pending, text)
        self.assertEqual((crates, failures), ([], []))


class Tiers(unittest.TestCase):
    def test_single_and_paired_cells_are_read_and_dash_rows_skipped(self):
        tiered, failures = failures_of(gate.tiers, ARCHITECTURE)
        self.assertEqual(failures, [])
        self.assertEqual(
            tiered,
            {
                "tokio": "public API",
                "http-body": "public API",
                "http-body-util": "core",
                "matchit": "core",
            },
        )

    def test_a_row_without_a_tier_is_reported(self):
        text = ARCHITECTURE.replace("| `router/` | core |", "| `router/` | maybe |")
        _, failures = failures_of(gate.tiers, text)
        self.assertIn("names no tier", failures[0])
        self.assertIn("matchit", failures[0])


class Erased(unittest.TestCase):
    def test_the_linked_names_are_read(self):
        names, failures = failures_of(gate.erased, ARCHITECTURE)
        self.assertEqual((names, failures), ({"UncheckedService", "UncheckedInner"}, []))

    def test_a_count_that_disagrees_is_reported(self):
        text = ARCHITECTURE.replace("those two are", "those three are")
        _, failures = failures_of(gate.erased, text)
        self.assertIn("links 2", failures[0])


class Listing(unittest.TestCase):
    def test_roots_skip_the_standard_library_primitives_and_inner_segments(self):
        line = (
            "pub fn kynos::a::B::c(u8, core::option::Option<serde_core::de::X>) "
            "-> http::response::Response<bytes::bytes::Bytes>"
        )
        self.assertEqual(gate.roots(line), {"serde_core", "http", "bytes"})

    def test_an_item_path_drops_generics_and_its_signature(self):
        self.assertEqual(
            gate.item_path("pub fn kynos::server::Server<C>::serve_io<I>(&self, I) -> X"),
            "kynos::server::Server::serve_io",
        )
        self.assertEqual(
            gate.item_path("pub type kynos::u::Inner<C>::Future = core::pin::Pin<X>"),
            "kynos::u::Inner::Future",
        )
        self.assertIsNone(gate.item_path("impl kynos::a::B"))

    def test_an_impl_header_splits_trait_from_type(self):
        self.assertEqual(
            gate.impl_header("impl<F: Fn() -> u8> futures_core::stream::Stream for kynos::r::Records<F> where F: Send"),
            ("futures_core::stream::Stream", "kynos::r::Records"),
        )
        self.assertEqual(gate.impl_header("impl !core::marker::Freeze for kynos::B")[0], "core::marker::Freeze")
        self.assertEqual(gate.impl_header("impl<T> kynos::r::Records<T>"), (None, "kynos::r::Records"))


POLL = "pub fn kynos::r::Records<T>::poll_next(core::pin::Pin<&mut Self>, &mut X) -> Y"
FUTURE = "pub type kynos::u::UncheckedInner<C>::Future = core::pin::Pin<alloc::boxed::Box<dyn F>>"
SERVE_IO = "pub fn kynos::server::Server<C>::serve_io<I>(&self, I) -> X where I: tokio::io::AsyncRead"


class Surface(unittest.TestCase):
    def check(self, listing):
        return gate.surface_failures(
            "kynos", listing, ["Server::serve_io"], {"UncheckedService", "UncheckedInner"}
        )

    def test_a_pin_a_foreign_trait_fixes_is_allowed(self):
        listing = "\n".join(
            [
                "impl<T> futures_core::stream::Stream for kynos::r::Records<T>",
                POLL,
                "impl<C> tower_service::Service<X> for kynos::u::UncheckedInner<C>",
                FUTURE,
                SERVE_IO,
            ]
        )
        self.assertEqual(self.check(listing), [])

    def test_a_pin_in_an_inherent_method_is_reported(self):
        listing = "impl<T> kynos::r::Records<T>\n" + POLL
        self.assertEqual(len(self.check(listing)), 1)

    def test_a_pin_in_a_kynos_trait_impl_is_reported(self):
        listing = "impl<T> kynos::r::Poll for kynos::r::Records<T>\n" + POLL
        self.assertEqual(len(self.check(listing)), 1)

    def test_a_boxed_future_on_an_unnamed_type_is_reported(self):
        listing = (
            "impl core::future::IntoFuture for kynos::a::Other\n"
            "pub type kynos::a::Other::IntoFuture = core::pin::Pin<alloc::boxed::Box<dyn F>>"
        )
        self.assertEqual(len(self.check(listing)), 1)

    def test_a_free_function_after_a_trait_impl_is_not_its_member(self):
        listing = (
            "impl<T> futures_core::stream::Stream for kynos::r::Records<T>\n"
            "pub fn kynos::r::poll(core::pin::Pin<&mut u8>)"
        )
        self.assertEqual(len(self.check(listing)), 1)

    def test_a_tokio_path_elsewhere_is_reported(self):
        listing = "pub fn kynos::server::Server<C>::serve<I>(I) where I: tokio::io::AsyncRead"
        failures = self.check(listing)
        self.assertEqual(len(failures), 1)
        self.assertIn("tokio", failures[0])


class Dependencies(unittest.TestCase):
    ALLOWED = {"serde": [], "futures-core": [], "tokio": ["Server::serve_io"]}

    def check(self, named, awaiting=(), tiered=None):
        return gate.dependency_failures(named, self.ALLOWED, list(awaiting), tiered or {})

    NAMED = {"serde_core": "a", "futures_core": "b", "tokio": "c"}

    def test_a_facade_and_a_hyphenated_crate_are_matched(self):
        self.assertEqual(self.check(self.NAMED), [])

    def test_a_crate_outside_both_lists_is_reported(self):
        failures = self.check({**self.NAMED, "prost": "impl<T: prost::Message>"})
        self.assertEqual(len(failures), 1)
        self.assertIn("`prost`", failures[0])

    def test_a_pending_crate_is_admitted_until_it_is_gone(self):
        self.assertEqual(self.check({**self.NAMED, "tracing_core": "x"}, ["tracing"]), [])
        failures = self.check(self.NAMED, ["tracing"])
        self.assertIn("no public item names it any more", failures[0])

    def test_an_allowed_crate_nothing_names_is_reported(self):
        failures = self.check({"serde_core": "a", "tokio": "c"})
        self.assertEqual(len(failures), 1)
        self.assertIn("`futures-core`", failures[0])

    def test_a_tier_the_listing_contradicts_is_reported(self):
        failures = self.check(self.NAMED, tiered={"futures-core": "core", "hyper": "public API"})
        self.assertEqual(len(failures), 2)


class Snapshot(unittest.TestCase):
    def test_equal_missing_and_different(self):
        self.assertEqual(gate.snapshot_failures("kynos", "a\n", "a\n"), [])
        self.assertIn("is not there", gate.snapshot_failures("kynos", None, "a\n")[0])
        failure = gate.snapshot_failures("kynos", "a\nb\n", "a\nc\n")[0]
        self.assertIn("-b", failure)
        self.assertIn("+c", failure)


class Main(unittest.TestCase):
    # The fixture's graph tiers `http-body` public API, so the allowance has to
    # carry it for the documents to agree with each other.
    ARCHITECTURE = ARCHITECTURE.replace("- `tokio` —", "- `http-body` — the impl.\n- `tokio` —")

    LISTING = "\n".join(
        [
            "pub fn kynos::a(http::Request, bytes::Bytes, serde_core::X)",
            SERVE_IO,
            "pub fn kynos::b(uuid::Uuid, tracing_core::Level, http_body::Frame)",
        ]
    )

    def run_main(self, recorded, listing=LISTING, overwrite=False, root=None):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
            status = gate.main(
                architecture=self.ARCHITECTURE,
                listings={"kynos": listing},
                recorded={"kynos": recorded},
                overwrite=overwrite,
                root=root,
            )
        return status, stderr.getvalue()

    def test_overwrite_records_the_listing_and_still_runs_every_other_rule(self):
        listing = self.LISTING + "\npub fn kynos::c(mio::Token)"
        with tempfile.TemporaryDirectory() as root:
            snapshot = pathlib.Path(root, "crates", "kynos", gate.SNAPSHOT)
            snapshot.parent.mkdir(parents=True)
            snapshot.write_text("stale\n")
            status, stderr = self.run_main(
                "stale\n", listing, overwrite=True, root=pathlib.Path(root)
            )
            self.assertEqual(snapshot.read_text(), listing)
        self.assertEqual(status, 1)
        self.assertEqual(stderr.count("public-api: "), 1)
        self.assertIn("mio", stderr)

    def test_overwrite_records_every_crate_after_the_first(self):
        listings = {
            "kynos-openapi": "pub fn kynos_openapi::a(serde_core::X)",
            "kynos": self.LISTING,
        }
        with tempfile.TemporaryDirectory() as root:
            root = pathlib.Path(root)
            for crate in listings:
                (root / "crates" / crate).mkdir(parents=True)
            with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(
                io.StringIO()
            ):
                status = gate.main(
                    architecture=self.ARCHITECTURE,
                    listings=listings,
                    recorded={},
                    overwrite=True,
                    root=root,
                )
            for crate, listing in listings.items():
                self.assertEqual((root / "crates" / crate / gate.SNAPSHOT).read_text(), listing)
        self.assertEqual(status, 0)

    def test_a_listing_matching_its_snapshot_and_the_documents_passes(self):
        self.assertEqual(self.run_main(self.LISTING), (0, ""))

    def test_a_drifted_snapshot_is_the_only_failure(self):
        status, stderr = self.run_main(self.LISTING.replace("uuid::Uuid, ", ""))
        self.assertEqual(status, 1)
        self.assertEqual(stderr.count("public-api: "), 1)
        self.assertIn("+pub fn kynos::b(uuid::Uuid", stderr)


class RealDocuments(unittest.TestCase):
    def test_architecture_md_reads_without_a_failure(self):
        text = gate.ARCHITECTURE.read_text()
        failures = []
        self.assertIn("tokio", gate.allowance(text, failures))
        self.assertTrue(gate.tiers(text, failures))
        self.assertTrue(gate.erased(text, failures))
        gate.pending(text, failures)
        self.assertEqual(failures, [])


if __name__ == "__main__":
    unittest.main()
