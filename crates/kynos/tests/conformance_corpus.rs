//! The corpus a downstream generator is tested against.
//!
//! Kynos emits the contract; a client generator consumes it. Neither can check
//! the other from its own repository, so the checkable thing between them is a
//! set of committed documents: Kynos asserts it still emits them, and a
//! generator asserts it can read them.
//!
//! # What this is not
//!
//! [`conformance.rs`](conformance.rs) is inward-facing — it checks a *running
//! service* against its own description and exports nothing. This is the
//! outward-facing half, and the two answer different questions: that a service
//! keeps its promises, and that the promises are the ones a generator was built
//! against.
//!
//! # Regenerating
//!
//! ```text
//! mise run fixtures:generate
//! ```
//!
//! The same idiom as the `trybuild` snapshots: the test writes the corpus when
//! it is told to and compares it otherwise, so a deliberate change to what
//! Kynos emits is reviewed as a diff of the emitted documents rather than
//! landing invisibly.
//!
//! # Why this is sound
//!
//! Only because emission is byte-stable. `tests/determinism.rs` is what makes
//! "the committed file equals a freshly generated one" a statement about the
//! *description* rather than about the order two `HashMap`s happened to
//! iterate in.

#![cfg(all(feature = "macros", feature = "json", feature = "openapi32"))]

use std::{path::PathBuf, pin::Pin};

use kynos::{
    Router,
    extract::body::json_lines::JsonLines,
    openapi::{Document, SpecVersion},
    prelude::*,
    response::stream::sse::Sse,
};
use serde::{Deserialize, Serialize};

/// Set to `overwrite` to write the corpus instead of comparing it.
const OVERWRITE: &str = "KYNOS_FIXTURES";

// --- The fixture API ------------------------------------------------------
//
// Chosen for what a 3.2 client generator has to understand and cannot get from
// a 3.1 document: `itemSchema` for a sequential body, and `contentMediaType`
// with `contentSchema` for the JSON an SSE `data` field carries.

/// One reading, which is what every stream here carries.
#[derive(Schema, Serialize, Deserialize)]
struct Reading {
    /// When it was taken, in milliseconds since the epoch.
    at_millis: i64,
    /// What was read.
    value: f64,
}

/// How a device reported in.
///
/// A tagged union, which is the shape a generator can decode by reading one
/// property. `#[serde(untagged)]` is refused, so no fixture can carry one.
#[derive(Schema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Report {
    /// A measurement.
    Sample { reading: Reading },
    /// A device saying nothing is wrong.
    Heartbeat,
}

/// A finite stream of readings.
struct Readings(u32);

impl futures_core::Stream for Readings {
    type Item = Reading;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.0 == 0 {
            return std::task::Poll::Ready(None);
        }
        self.0 -= 1;
        std::task::Poll::Ready(Some(Reading {
            at_millis: i64::from(self.0),
            value: f64::from(self.0),
        }))
    }
}

/// The same, as events.
struct Events(u32);

impl futures_core::Stream for Events {
    type Item = Result<kynos::response::stream::sse::Event<Report>, std::convert::Infallible>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.0 == 0 {
            return std::task::Poll::Ready(None);
        }
        self.0 -= 1;
        std::task::Poll::Ready(Some(Ok(kynos::response::stream::sse::Event::new(
            Report::Heartbeat,
        ))))
    }
}

/// Streams readings as JSON Lines, described item by item.
///
/// The `itemSchema` case: 3.1 can only call this an opaque string, which is the
/// whole reason a generator needs 3.2.
#[kynos::get("/readings/export")]
async fn export() -> JsonLines<Readings> {
    JsonLines { items: Readings(3) }
}

/// Streams reports as Server-Sent Events.
///
/// The envelope case: the response is `text/event-stream` with an `itemSchema`
/// describing the *event*, and the JSON in its `data` field reached through
/// `contentMediaType` and `contentSchema`.
#[kynos::get("/reports/live")]
async fn live() -> Sse<Events> {
    Sse::new(Events(3))
}

/// An ordinary JSON body, so the corpus is not only streams.
#[kynos::post("/readings")]
async fn record(Json(reading): Json<Reading>) -> Created<Json<Reading>> {
    Created::at("/readings/1", Json(reading))
}

/// The description the corpus holds.
fn document(version: SpecVersion) -> Document {
    Router::<()>::new()
        .mount(kynos::routes![export, live, record])
        .openapi_as(version)
        .expect("the fixture describes itself")
}

/// Where the corpus lives.
fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conformance")
}

/// Every document in the corpus, by file name.
fn documents() -> Vec<(&'static str, Document)> {
    vec![("sequential-3.2.json", document(SpecVersion::V3_2))]
}

/// The corpus on disk is the corpus this build emits.
///
/// Fails on any difference, and writes instead when `KYNOS_FIXTURES=overwrite`
/// — so a change to what Kynos emits reaches review as a diff of the documents
/// a generator is built against, rather than as a silent widening.
#[test]
fn the_committed_corpus_is_what_this_build_emits() {
    let overwrite = std::env::var(OVERWRITE).is_ok_and(|value| value == "overwrite");
    let directory = corpus();

    for (name, document) in documents() {
        let emitted = document.to_json().expect("a description serializes");
        let path = directory.join(name);

        if overwrite {
            std::fs::create_dir_all(&directory).expect("the corpus directory");
            std::fs::write(&path, format!("{emitted}\n")).expect("the corpus is writable");
            continue;
        }

        let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!("`{name}` is not in the corpus; run `mise run fixtures:generate` to add it")
        });

        assert!(
            committed.trim_end() == emitted.trim_end(),
            "`{name}` differs from what this build emits. If the change is intended, run \
             `mise run fixtures:generate` and review the diff; a generator is built against \
             these bytes."
        );
    }
}

/// The corpus carries the 3.2 constructs a generator is forked to understand.
///
/// Asserted against the *committed* text rather than a freshly built document,
/// because what a downstream repository reads is the file. A corpus that lost
/// `itemSchema` while still round-tripping would pass the test above and be
/// useless to the consumer it exists for.
#[test]
fn the_corpus_carries_what_a_3_2_generator_needs() {
    let committed = std::fs::read_to_string(corpus().join("sequential-3.2.json"))
        .expect("the corpus is committed");

    for construct in ["itemSchema", "contentMediaType", "contentSchema"] {
        assert!(
            committed.contains(construct),
            "the corpus carries no `{construct}`, which is one of the constructs it exists to \
             pin"
        );
    }

    assert!(
        committed.contains("text/event-stream"),
        "the corpus carries no SSE envelope"
    );
    // A tagged union decodes by reading one property; the refusal of untagged
    // ones is what keeps every union in the corpus decodable.
    assert!(
        committed.contains("\"discriminator\""),
        "the corpus carries no discriminated union"
    );
}
