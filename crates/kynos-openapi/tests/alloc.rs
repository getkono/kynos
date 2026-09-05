//! What producing a description costs, counted at 10, 100 and 1000 operations.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), for the
//! document model rather than the routing path. It is a target of its own
//! rather than a sibling `tests.rs` because a `#[global_allocator]` is
//! process-wide: installed in the library's unit-test binary the counter would
//! reach, and slow, every other unit test in it. That is the same reason
//! [`crates/kynos/tests/alloc.rs`](../../kynos/tests/alloc.rs) is a target of
//! its own, and it is why there are now two of them rather than one shared one
//! — an integration binary cannot be depended on.
//!
//! **The counter is per-thread.** `alloc_counter` counts into thread locals
//! rather than into globals, so a region reports what the measuring thread
//! allocated and nothing else. `work_on_another_thread_is_not_counted` in
//! `kynos`'s target holds the dependency to that, once, for the version this
//! workspace pins at its root manifest; this file does not restate it.
//!
//! # What is measured
//!
//! Two stages, because one of them cannot fail on the cost this file exists to
//! watch:
//!
//! - [`Document::to_json`] is the only entry point producing output *bytes*, so
//!   the output-size half of the requirement can be read from nowhere else. It
//!   walks `paths` exactly once: `Paths::serialize` iterates its map and every
//!   `Serialize` below it is a derive.
//! - [`Document::emit`] is where a nested walk over `paths` would live.
//!   `emit::downgrade::three_two_only_constructs` iterates `document.paths.items`
//!   and descends per entry, building a JSON pointer per node — and `to_json`
//!   never calls it. Without this stage the target would count a walk that is
//!   linear by construction and report the other one as unmeasured.
//!
//! **Reaching a cost is not the same as detecting it, and the two halves of the
//! assertion do not divide the way the shape suggests.** Stage two puts the
//! downgrade walk inside a counted region; what fails when that walk turns
//! quadratic is the recorded per-size **ceiling**, not the growth relation. The
//! relation is algebraically blind to an added exactly-quadratic term — see
//! "What this cannot see" below, which works the cancellation out. So the
//! ceilings are load-bearing for precisely the defect this file was written to
//! catch, and dropping them in favour of the relation alone would leave that
//! defect detected by nothing.
//!
//! # What the fixture is, and is not
//!
//! One operation per path, one `GET` per Path Item, so the operation count is
//! the dimension that scales. `components` stays empty deliberately: the
//! requirement scales in *operation count*, and every additional dimension that
//! grew alongside it would make a growth factor unattributable to either.
//!
//! The fixture is built by the caller, outside every counted region.
//! Construction allocates orders of magnitude more than emission does — a
//! thousand Path Items, each with a boxed Operation, a parameter and a response
//! — and construction is linear, so a fixture built inside a region would keep
//! the relation below green while measuring something the requirement does not
//! name.
//!
//! # Features
//!
//! The recorded ceilings are the `--all-features` reading, which is what CI
//! measures. `emit/downgrade.rs` gates its whole walk behind `openapi32`, so a
//! baseline build returns an empty `Vec` and does the clone alone: stage two
//! was measured at 125, 1205 and 12 005 there against the 374, 3524 and 35 024
//! recorded below. The same ceilings hold at baseline by being loose rather
//! than by a second table — one `#[cfg]`-free table is worth more than a
//! second set of numbers nothing else reads. Stage one and the byte counts are
//! identical under both, since neither feature changes what 3.1 serializes.
//! Nothing here is `#[cfg]`-gated, so `mise run test:baseline` runs the file as
//! written.
//!
//! # What this cannot see
//!
//! - **A quadratic that allocates nothing.** `Vec::contains` in a hot loop is
//!   O(n²) comparisons and zero allocations. `alloc_counter` counts calls, so
//!   this target is blind to it; that is the timed twin's job, and the twin
//!   lives in `kynos-bench`.
//! - **Allocation *size*.** One `Vec::with_capacity(n * n)` is one allocation.
//!   The output-bytes series covers the sub-case where a blowup reaches the
//!   wire — and it is the recorded byte ceiling that covers it, not the
//!   relation over bytes, which cancels an exactly-quadratic term exactly as
//!   the relation over allocations does. A quadratic scratch buffer that never
//!   reaches the wire is invisible to both.
//! - **Exponents strictly between 1 and 2.** `n^1.9` satisfies the relation.
//! - **An added exactly-quadratic term, at any coefficient — including the
//!   nested walk over `paths` that stage two exists to reach.** This is the
//!   sharp one, and it is algebra rather than a matter of degree. Write the
//!   cost as `a(x) = L(x) + c·x²`. The relation asserts `a(n)·m² < a(m)·n²`,
//!   which expands to `L(n)·m² + c·n²m² < L(m)·n² + c·m²n²` — and the two `c`
//!   terms are *identical*, so they cancel and leave `L(n)·m² < L(m)·n²`. The
//!   relation therefore tests the non-quadratic remainder and nothing else. It
//!   fires on `n^2.0001` and on `n³`; it does not fire at exactly `n²` over a
//!   linear cost, at any coefficient, over any span of sizes. Concretely: a
//!   descent in `three_two_only_constructs` allocating one pointer per
//!   (path, path) pair reads 474, 13 524 and 1 035 024 — 97% of the cost at a
//!   thousand operations — and *both* decades satisfy the relation.
//!
//!   **The per-size ceilings are what catch that case** (474 exceeds the 374
//!   recorded at ten operations), and they are what catch the sub-quadratic
//!   exponents above. That is the division of labour, and it is the reverse of
//!   what a reader expects: the relation is the half that carries no recorded
//!   number and cannot go stale, and the ceilings are the half that actually
//!   detects a nested walk. Neither substitutes for the other, and
//!   [`performance.md`](../../../docs/performance.md) observing that relations
//!   outlive absolutes is not licence to drop these absolutes.
//! - **A one-time cost paid at the smallest size,** which would depress the
//!   first decade's growth factor. Both decades are asserted independently, and
//!   `a_repeated_emission_costs_what_the_first_one_did` is what says there is no
//!   such cost inside a region to begin with.
//! - **Anything above 1000 operations,** and anything outside this crate: a
//!   rescan during route *registration* is `kynos`'s router, not the document
//!   model.

use alloc_counter::{AllocCounterSystem, count_alloc};
use kynos_openapi::{
    Document, Info, Method, Operation, Parameter, PathItem, PathTemplate, Response, Responses,
    Schema, SpecVersion, model::schema::types::SchemaType,
};

/// Declared here rather than reached for: `alloc_counter` installs nothing on
/// its own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

/// One measured point: an operation count, and what emitting at it costs today.
///
/// All three ceilings were transcribed from the first recorded run, verbatim
/// and with no margin added, as
/// [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first
/// measurement. A margin would be a guess wearing a measurement's clothes, and
/// raising one of these is a deliberate edit rather than a rounding error
/// absorbing a regression.
///
/// **These are not the softer half of the file — do not delete them in favour
/// of the growth relation.** The relation cancels an added exactly-quadratic
/// term algebraically, so a nested walk over `paths` satisfies it at every
/// coefficient and every span, in bytes exactly as in allocations; these
/// numbers are the only thing in the file that reads such a walk as a failure.
/// The module documentation works the cancellation out.
struct Size {
    /// How many operations the fixture at this point declares.
    operations: usize,
    /// What one [`Document::to_json`] allocates here.
    json_allocations: usize,
    /// What one [`Document::emit`] allocates here, at `--all-features`.
    emit_allocations: usize,
    /// How many bytes one [`Document::to_json`] writes here.
    output_bytes: usize,
}

/// The three points, a decade apart, as the requirement names them.
const SIZES: [Size; 3] = [
    Size {
        operations: 10,
        json_allocations: 27,
        emit_allocations: 374,
        output_bytes: 4675,
    },
    Size {
        operations: 100,
        json_allocations: 210,
        emit_allocations: 3524,
        output_bytes: 45985,
    },
    Size {
        operations: 1000,
        json_allocations: 2013,
        emit_allocations: 35024,
        output_bytes: 460_885,
    },
];

/// How many times each reading is repeated by
/// `a_repeated_emission_costs_what_the_first_one_did`.
///
/// Eight, not the ten thousand `kynos`'s target replays: a thousand-operation
/// emission in a debug build under `-C instrument-coverage` is milliseconds
/// rather than microseconds, and eight readings across three sizes and two
/// stages stays well inside `.config/nextest.toml`'s bound. Eight agreeing
/// readings is what the check needs; more of them buys nothing it does not
/// already have.
const REPEATS: usize = 8;

/// The fixture at `operations` operations, built here rather than in a region.
///
/// **It asserts its own size, because nothing else in the file can.** Every
/// live assertion here is satisfied by a fixture that collapsed: the ceilings
/// are `<=`, so a constant series passes all three; the relation holds over a
/// constant series at every span; and a repeated emission of the same collapsed
/// document costs what the first one did. A template that normalized to one
/// string, or a `Paths::insert` that replaced rather than added, would turn this
/// whole target green while measuring ten documents of one operation. The count
/// is taken over the operations rather than over `paths.items`, because the
/// operation count is the dimension the requirement scales in and a Path Item
/// can hold up to nine of them.
fn document(operations: usize) -> Document {
    let mut document = Document::new(SpecVersion::V3_1, Info::new("Fixture", "1.0.0"));

    for index in 0..operations {
        let template = PathTemplate::parse(format!("/resources/{index}/items/{{id}}"))
            .expect("a fixture template parses");
        let operation = Operation::new(format!("getItem{index}"))
            .with_tag("items")
            .with_parameter(
                Parameter::path("id", Schema::of_type(SchemaType::String)).required(true),
            )
            .with_responses(Responses::new().with(200, Response::new("The item")));

        document.paths.insert(
            &template,
            PathItem::new().with_operation(Method::Get, operation),
        );
    }

    let declared: usize = document
        .paths
        .items
        .values()
        .map(|item| item.operations().count())
        .sum();
    assert_eq!(
        declared, operations,
        "the fixture built for {operations} operations declares {declared} of them; a fixture \
         that collapses satisfies every ceiling, the growth relation and repeat-invariance alike, \
         so a size read here is the only thing saying what was measured"
    );

    document
}

/// Widens a count for the cross-multiplication below.
///
/// `usize` has no `From` conversion to `u128` — it could in principle be wider
/// — and `as` is what `clippy::cast_lossless` is there to refuse. At the counts
/// recorded above, times a six-digit square, 128 bits overflows nothing.
fn wide(value: usize) -> u128 {
    u128::try_from(value).expect("a count fits in 128 bits")
}

/// Serializes once and reports both what it allocated and how large it was.
///
/// Fresh allocations and reallocations summed, so that growing a buffer cannot
/// pass as free. The `expect`, the `len` and the `drop` are all outside the
/// region: what is counted is the serialization and nothing around it.
fn counted_json(document: &Document) -> (usize, usize) {
    let ((allocations, reallocations, _), emitted) = count_alloc(|| document.to_json());

    let emitted = emitted.expect("a fixture is representable in JSON");
    let bytes = emitted.len();
    drop(emitted);

    (allocations + reallocations, bytes)
}

/// Emits once at 3.1 and reports what it allocated.
///
/// This is the stage that walks `paths` per entry when `openapi32` is on. The
/// fixture is built at 3.1 and carries no 3.2-only construct, so the walk
/// finds no blocker and the emission succeeds — which is what keeps the walk
/// and the copy that follows it inside one reading.
///
/// **A blocker would not shorten the walk.**
/// `emit::downgrade::three_two_only_constructs` pushes each one it finds and
/// carries on to the end, so a fixture carrying one is walked exactly as far.
/// What the blocker skips is [`Document::emit`]'s `self.clone()`, which the
/// early `Err` return never reaches — the cost the Features note above reads
/// on its own at baseline, where the walk returns an empty `Vec` and the clone
/// is all that is left. A blocker-free fixture is chosen for the larger of two
/// readings, then, rather than for the longer of two walks.
fn counted_emit(document: &Document) -> usize {
    let ((allocations, reallocations, _), emitted) =
        count_alloc(|| document.emit(SpecVersion::V3_1));

    let emitted = emitted.expect("a fixture built at 3.1 downgrades to 3.1");
    drop(emitted);

    allocations + reallocations
}

/// Asserts that `readings` grows by strictly less than the square of the size
/// ratio, for each consecutive pair.
///
/// The comparison is cross-multiplied into integers — `a(n)·m² < a(m)·n²`,
/// which is exactly `a(n)/a(m) < (n/m)²` — so there is no `f64`, no `log10` and
/// no platform on which the verdict differs.
///
/// **A fitted log-log slope with a ceiling was rejected, and the reason is that
/// the ceiling would be a guess.** A pure quadratic over a decade fits at slope
/// exactly 2.0, so a gate at 2.0 turns on the last bit of a logarithm, and any
/// number below it is one nobody measured — which
/// [`nfr.md`](../../../docs/nfr.md#thresholds) refuses. The `n²` here is not a
/// threshold: it is the definition of sub-quadratic, written down, and it
/// cannot go stale on a toolchain bump. Each decade is asserted on its own
/// rather than fitted across all three points, because a single fit *averages*
/// the two decades and a quadratic term still small at 10 operations can hide
/// inside that average.
///
/// **What this cannot see, stated where it is asserted:** an added
/// exactly-quadratic term cancels on both sides of the cross-multiplication, so
/// this function tests the non-quadratic remainder alone. `SIZES`'s recorded
/// ceilings are what fail on a nested walk over `paths`, and
/// `the_relation_passes_a_nested_walk` below holds both halves of that
/// statement against one series rather than asserting it in prose. A
/// per-operation form
/// — `a(n)/n` non-increasing — would see that term, and is rejected because it
/// asserts *linearity*: `nfr.md` asks for sub-quadratic, and an emitter that
/// legitimately reached `n·log n` would fail it. Over-asserting a requirement
/// is its own defect.
fn stays_sub_quadratic(measure: &str, readings: &[(usize, usize)]) {
    for pair in readings.windows(2) {
        let (smaller, at_smaller) = pair[0];
        let (larger, at_larger) = pair[1];

        let bound = wide(at_smaller) * wide(larger).pow(2) / wide(smaller).pow(2);
        assert!(
            wide(at_larger) * wide(smaller).pow(2) < wide(at_smaller) * wide(larger).pow(2),
            "at {smaller} operations {measure} was {at_smaller} and at {larger} it was \
             {at_larger}; a quadratic cost would reach {bound} and this reading is not below it, \
             so producing a description has stopped scaling sub-quadratically in operation count"
        );
    }
}

/// The policy clause a recorded ceiling carries when it has nothing more
/// specific to say. Two of the three do; `emit`'s carries a caveat of its own.
const A_CEILING_MOVES_BY_DECISION: &str =
    "raising a ceiling is a deliberate edit and lowering one is what an improvement looks like";

/// Asserts one reading is at or below the number recorded beside it.
///
/// A function rather than three inline `assert!`s, so that the comparison has
/// a caller which is not a passing live reading:
/// `a_recorded_ceiling_rejects_a_nested_walk` drives it with the
/// counterexample the module documentation works out, and that is what holds
/// the `<=` the right way round. `note` is what differs between the three
/// recorded numbers; everything else about the message is shared.
fn within_recorded(measure: &str, operations: usize, reading: usize, recorded: usize, note: &str) {
    assert!(
        reading <= recorded,
        "{measure} at {operations} operations read {reading} against a recorded {recorded}; {note}"
    );
}

/// A nested descent over `paths`, allocating one pointer per (path, path)
/// pair, as the module documentation reads it off the fixture.
///
/// Synthetic on purpose: no emitter in this crate produces it, and writing one
/// that did would be a change to the code under measurement rather than a
/// check on the measurement.
const NESTED_WALK: [(usize, usize); 3] = [(10, 474), (100, 13_524), (1000, 1_035_024)];

/// The claim this file is built around, held as a test rather than as prose:
/// the walk stage two exists to reach satisfies the relation at every point.
///
/// Every other assertion here runs only on live readings that pass, so
/// `stays_sub_quadratic` collapsing to `true` turns nothing red. This is the
/// control. Should it ever fail, the relation has become able to see an
/// exactly-quadratic term — a change to the argument the file makes, not a
/// regression to paper over.
#[test]
fn the_relation_passes_a_nested_walk() {
    stays_sub_quadratic("a nested walk's allocation count", &NESTED_WALK);
}

/// ...and the relation can still fail, on a decade that grows by a full cube.
///
/// The expected fragment names the interpolated readings rather than the
/// closing clause, so collapsing the message's three arms into one fixed
/// string fails here.
#[test]
#[should_panic(expected = "at 10 operations a cubic cost was 10 and at 100 it was 10000")]
fn the_relation_fails_on_a_cubic_series() {
    stays_sub_quadratic("a cubic cost", &[(10, 10), (100, 10_000)]);
}

/// The other half of the division of labour, at the point where it is
/// tightest: 474 against the 374 recorded at ten operations.
///
/// With the two above, this is the whole of what
/// [`nfr.md`](../../../docs/nfr.md#document-model)'s row asserts — that the
/// recorded ceilings and not the relation are what read a nested walk as a
/// failure — held where a reader can run it.
#[test]
#[should_panic(expected = "read 474 against a recorded 374")]
fn a_recorded_ceiling_rejects_a_nested_walk() {
    let (operations, reading) = NESTED_WALK[0];

    within_recorded(
        "a nested walk's emit allocation count",
        operations,
        reading,
        SIZES[0].emit_allocations,
        A_CEILING_MOVES_BY_DECISION,
    );
}

/// The record: what one emission costs today, at each size and in both stages.
///
/// Recorded rather than merely observed, because the relation below records
/// nothing. An emitter that doubled its per-operation allocation count would
/// stay perfectly linear and turn none of the growth assertions red.
#[test]
fn one_emission_allocates_what_was_recorded() {
    for size in &SIZES {
        let document = document(size.operations);

        let (json, _) = counted_json(&document);
        within_recorded(
            "one to_json's allocation count",
            size.operations,
            json,
            size.json_allocations,
            A_CEILING_MOVES_BY_DECISION,
        );

        let emitted = counted_emit(&document);
        within_recorded(
            "one emit's allocation count",
            size.operations,
            emitted,
            size.emit_allocations,
            "the ceiling is the `--all-features` reading, so a baseline run is expected to come \
             in under it rather than at it",
        );
    }
}

/// The requirement itself, over both stages: cost per operation does not grow
/// with the number of operations.
#[test]
fn emission_allocations_grow_sub_quadratically_in_operation_count() {
    let readings: Vec<(usize, usize, usize)> = SIZES
        .iter()
        .map(|size| {
            let document = document(size.operations);
            let (json, _) = counted_json(&document);
            (size.operations, json, counted_emit(&document))
        })
        .collect();

    let json: Vec<(usize, usize)> = readings
        .iter()
        .map(|&(operations, json, _)| (operations, json))
        .collect();
    stays_sub_quadratic("one to_json's allocation count", &json);

    let emitted: Vec<(usize, usize)> = readings
        .iter()
        .map(|&(operations, _, emitted)| (operations, emitted))
        .collect();
    stays_sub_quadratic("one emit's allocation count", &emitted);
}

/// The other half of the same requirement, over the bytes that reach a reader.
///
/// **Both a ceiling and the relation, for the reason the module documentation
/// works out.** The relation cancels an added exactly-quadratic term, so a
/// `to_json` writing one byte per (path, path) pair satisfies it at every
/// coefficient and every span — and with the relation alone the output-size
/// half of the requirement would be caught here by nothing at all. The
/// recorded ceiling is what reads such a blowup as a failure, exactly as it is
/// on the allocation side.
///
/// It is not a second `wire.rs`, though a model change adding an
/// always-serialized field does move this number and is seen here. `wire.rs`
/// asks what each type writes at one fixed size, field name by field name, and
/// is where such a change is described; this asks how the total scales with
/// the operation count. Re-recording the number afterwards is the same
/// deliberate edit the allocation ceilings already ask for.
#[test]
fn output_size_grows_sub_quadratically_in_operation_count() {
    let bytes: Vec<(usize, usize)> = SIZES
        .iter()
        .map(|size| {
            let (_, bytes) = counted_json(&document(size.operations));
            within_recorded(
                "the emitted document's size in bytes",
                size.operations,
                bytes,
                size.output_bytes,
                A_CEILING_MOVES_BY_DECISION,
            );
            (size.operations, bytes)
        })
        .collect();

    stays_sub_quadratic("the emitted document's size in bytes", &bytes);
}

/// The instrument's own invariant, and the one every number above rests on: a
/// reading is a property of emission rather than of first-touch state in a
/// fixture that has been emitted before.
///
/// The fixture is built once and emitted repeatedly, so anything lazily paid on
/// first use — a buffer a `Document` grows and keeps, a static a serializer
/// initializes, the profiling runtime's own first-counter touch under
/// `coverage:ci` — would land in the first region and in no other. That
/// inflates the smallest size and depresses the first decade's growth factor,
/// which weakens the relation above in exactly the direction that hides a
/// defect. Readings that agree is what says there is no such cost to hide.
///
/// There is no warm-up, for the reason `kynos`'s target refuses one: a warm-up
/// is the single construct able to hide a one-time cost introduced later. This
/// test is the honest form of the same check.
///
/// It does not restate `work_on_another_thread_is_not_counted`
/// ([`crates/kynos/tests/alloc.rs`](../../kynos/tests/alloc.rs)). That holds a
/// property of `alloc_counter` itself, which is pinned once for the whole
/// workspace, and it is asserted in a target that every task building this one
/// also builds.
#[test]
fn a_repeated_emission_costs_what_the_first_one_did() {
    for size in &SIZES {
        let document = document(size.operations);

        let (first_json, first_bytes) = counted_json(&document);
        let first_emit = counted_emit(&document);

        for repeat in 1..REPEATS {
            let (json, bytes) = counted_json(&document);
            assert_eq!(
                (json, bytes),
                (first_json, first_bytes),
                "to_json at {} operations allocated {first_json} times for {first_bytes} bytes on \
                 the first call and {json} times for {bytes} bytes on call {repeat}; a reading \
                 that moves between identical calls is state the first emission paid for and the \
                 rest did not",
                size.operations
            );

            let emitted = counted_emit(&document);
            assert_eq!(
                emitted, first_emit,
                "emit at {} operations allocated {first_emit} times on the first call and \
                 {emitted} on call {repeat}; the growth factors above compare two such readings, \
                 so a one-time cost inside the region makes the smaller size look dearer than it \
                 is",
                size.operations
            );
        }
    }
}
