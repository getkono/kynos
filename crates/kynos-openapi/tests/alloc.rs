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
//! - [`Document::to_json`] is the only JSON entry point producing output
//!   *bytes*, so the output-size half of the requirement can be read from
//!   nowhere else in that serialization. It walks `paths` exactly once:
//!   `Paths::serialize` iterates its map and every `Serialize` below it is a
//!   derive.
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
//! **Stage two is recorded twice, because it costs two different amounts.**
//! `emit/downgrade.rs` gates its whole walk behind `openapi32`, so a baseline
//! build returns an empty `Vec` and does the clone alone: stage two reads 125,
//! 1205 and 12 005 there against the 374, 3524 and 35 024 at
//! `--all-features`. Enforcing the larger number at baseline would run
//! `mise run test:baseline` — a gate of its own — at a ratchet roughly three
//! times looser than its own measurement, which is the guessed ceiling
//! [`nfr.md`](../../../docs/nfr.md#thresholds) refuses; a per-operation
//! allocation added to `three_two_only_constructs`'s baseline arm, which
//! `--all-features` never compiles, would fit inside the slack. So
//! `EMIT_CEILINGS` is the one `#[cfg]`-gated item in the file and every other
//! number is shared: stage one and the byte counts are identical under both,
//! since neither feature changes what 3.1 serializes.
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
//! - **`Document::to_yaml`'s output size.** It is the other entry point
//!   producing bytes, behind the `yaml` feature, and no series here reads it.
//!   It drives the same `Serialize` impls over the same single walk, so a
//!   nested walk added below `paths` is read as a failure by the JSON series
//!   above; what nothing records is what the YAML formatter itself writes.
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
//!   (path, path) pair reads 474, 13 524 and 1 035 024 at `--all-features` —
//!   each recorded ceiling plus n², which is how `NESTED_WALK` below derives
//!   them from whichever ceilings the build enforces, and over 95% of the cost
//!   at a thousand operations — and *both* decades satisfy the relation.
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
    /// What one [`Document::emit`] allocates here, in this build's features.
    emit_allocations: usize,
    /// How many bytes one [`Document::to_json`] writes here.
    output_bytes: usize,
}

/// What one [`Document::emit`] allocates at each of [`SIZES`]'s points, at
/// `--all-features`.
///
/// The only `#[cfg]`-gated item here, and it is gated because the cost it
/// records is: `emit::downgrade::three_two_only_constructs` walks `paths` per
/// entry under `openapi32` and returns an empty `Vec` without it, so one table
/// enforced under both would be roughly three times looser than its own
/// measurement in the build that reads it lower.
#[cfg(feature = "openapi32")]
const EMIT_CEILINGS: [usize; 3] = [374, 3524, 35_024];

/// ...and at baseline, where the downgrade walk is compiled out and
/// [`Document::emit`]'s `self.clone()` is the whole of stage two.
///
/// Transcribed from the baseline run exactly as the `--all-features` numbers
/// are from theirs, so `mise run test:baseline` ratchets on what it measures
/// rather than on what another feature set does.
#[cfg(not(feature = "openapi32"))]
const EMIT_CEILINGS: [usize; 3] = [125, 1205, 12_005];

/// The three points, a decade apart, as the requirement names them.
const SIZES: [Size; 3] = [
    Size {
        operations: 10,
        json_allocations: 27,
        emit_allocations: EMIT_CEILINGS[0],
        output_bytes: 4675,
    },
    Size {
        operations: 100,
        json_allocations: 210,
        emit_allocations: EMIT_CEILINGS[1],
        output_bytes: 45985,
    },
    Size {
        operations: 1000,
        json_allocations: 2013,
        emit_allocations: EMIT_CEILINGS[2],
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
/// whole target green while measuring ten documents of one operation.
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

    let declared = declared_operations(&document);
    assert_eq!(
        declared, operations,
        "the fixture built for {operations} operations declares {declared} of them; a fixture \
         that collapses satisfies every ceiling, the growth relation and repeat-invariance alike, \
         so a size read here is the only thing saying what was measured"
    );

    document
}

/// How many operations a document declares, summed over its Path Items.
///
/// A function rather than the loop [`document`] used to hold inline, for the
/// reason [`within_recorded`] is one: the comparison above it runs only on a
/// fixture that passes, so a count that had collapsed to the number of *entries*
/// — or to anything else agreeing with the loop bound — would turn nothing red.
/// `a_collapsed_document_declares_fewer_operations_than_were_inserted` drives
/// this with a document built to collapse, and that is what holds the size
/// guard the right way round.
///
/// The count is taken over the operations rather than over `paths.items`,
/// because the operation count is the dimension the requirement scales in and a
/// Path Item holds one per method — a count of entries would be the same number
/// in [`document`]'s fixture only for as long as it keeps one operation per
/// path.
fn declared_operations(document: &Document) -> usize {
    document
        .paths
        .items
        .values()
        .map(|item| item.operations().count())
        .sum()
}

/// Widens a count for the cross-multiplication below.
///
/// `usize` has no `From` conversion to `u128` — it could in principle be wider
/// — and `as` is what `clippy::cast_lossless` is there to refuse. At the counts
/// recorded above, times a six-digit square, 128 bits overflows nothing.
fn wide(value: usize) -> u128 {
    u128::try_from(value).expect("a count fits in 128 bits")
}

/// Runs one operation in a counted region and reports the heap operations it
/// made, alongside whatever it produced.
///
/// Fresh allocations and reallocations summed, so that growing a buffer cannot
/// pass as free. **This is the one place in the file that sums them**, and both
/// stages below read through it: two copies of the expression are two things to
/// hold and one of them can drift, so
/// [`the_counter_reports_every_heap_operation_in_the_region`] holds it once for
/// both. That is the arrangement `kynos`'s counting targets arrived at in #133
/// and #145, in this crate's smaller form.
///
/// The result is handed back rather than unwrapped here, because an `expect`, a
/// `len` and a `drop` are what a caller does with a reading and not what
/// producing it cost. Every one of them then sits outside the region by
/// construction rather than by care at two call sites.
fn counted<T>(operation: impl FnOnce() -> T) -> (usize, T) {
    let ((allocations, reallocations, _), produced) = count_alloc(operation);

    (allocations + reallocations, produced)
}

/// Serializes once and reports both what it allocated and how large it was.
///
/// The `expect`, the `len` and the `drop` are all outside the region: what is
/// counted is the serialization and nothing around it.
fn counted_json(document: &Document) -> (usize, usize) {
    let (allocations, emitted) = counted(|| document.to_json());

    let emitted = emitted.expect("a fixture is representable in JSON");
    let bytes = emitted.len();
    drop(emitted);

    (allocations, bytes)
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
    let (allocations, emitted) = counted(|| document.emit(SpecVersion::V3_1));

    let emitted = emitted.expect("a fixture built at 3.1 downgrades to 3.1");
    drop(emitted);

    allocations
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

/// The policy clause every recorded ceiling here carries.
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
///
/// **Derived from [`SIZES`] rather than transcribed beside it.** Each point is
/// that size's recorded `emit_allocations` plus one allocation per (path, path)
/// pair, which is what the module documentation adds up. Re-recording a
/// ceiling is the deliberate edit the record above asks for, and a transcribed
/// counterexample would survive one while describing a cost nothing records any
/// more — which turns the control below into a check on two stale numbers
/// agreeing with each other.
const NESTED_WALK: [(usize, usize); SIZES.len()] = nested_walk();

/// Builds [`NESTED_WALK`], as a `const fn` because a `const` initializer cannot
/// hold the loop that reads [`SIZES`].
const fn nested_walk() -> [(usize, usize); SIZES.len()] {
    let mut walk = [(0, 0); SIZES.len()];
    let mut index = 0;

    while index < SIZES.len() {
        let operations = SIZES[index].operations;
        walk[index] = (
            operations,
            SIZES[index].emit_allocations + operations * operations,
        );
        index += 1;
    }

    walk
}

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

/// The other half of the division of labour, at the smallest size — where the
/// added pair count is nearest the recorded ceiling and the rejection is
/// therefore tightest.
///
/// With the two above, this is the whole of what
/// [`nfr.md`](../../../docs/nfr.md#document-model)'s row asserts — that the
/// recorded ceilings and not the relation are what read a nested walk as a
/// failure — held where a reader can run it.
///
/// **`catch_unwind` rather than `#[should_panic]`, because the attribute's
/// `expected` is a string literal.** Writing the fragment there transcribes
/// both the walk's reading and the record it is refused against, so
/// re-recording a ceiling would fail this control while the file was correct,
/// and the failure would point at the control rather than at the edit. Caught
/// here instead, both halves of the fragment come from the same constants the
/// live tests read. The caught panic still reaches the default hook, so a
/// passing run prints one assertion message that nextest captures.
#[test]
fn a_recorded_ceiling_rejects_a_nested_walk() {
    let (operations, reading) = NESTED_WALK[0];
    let recorded = SIZES[0].emit_allocations;

    let panic = std::panic::catch_unwind(|| {
        within_recorded(
            "a nested walk's emit allocation count",
            operations,
            reading,
            recorded,
            A_CEILING_MOVES_BY_DECISION,
        );
    })
    .expect_err("a reading above its recorded ceiling is refused");

    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .expect("a formatted assertion panics with a String");
    let expected = format!("read {reading} against a recorded {recorded}");

    assert!(
        message.contains(&expected),
        "a nested walk at {operations} operations was refused with {message:?}, which does not \
         name the reading and the record it was refused against ({expected:?})"
    );
}

/// ...and a document that collapsed is read as smaller than it was built.
///
/// Two inserts of one template, each Path Item declaring a `GET` and a `POST`:
/// `Paths::insert` replaces the entry for a template rather than adding to it,
/// so four operations go in and one Path Item holding two of them survives.
/// That is a size no degenerate count reports — a count that repeated the
/// number its caller built would say four, and a count of `paths.items` would
/// say one — so both halves are asserted here rather than the equality alone,
/// which would say which number is right without saying what is wrong with the
/// other two. Built by hand rather than through [`document`], which asserts the
/// size it is about to be denied.
#[test]
fn a_collapsed_document_declares_fewer_operations_than_were_inserted() {
    let template = PathTemplate::parse("/resources/{id}").expect("a fixture template parses");
    let mut collapsed = Document::new(SpecVersion::V3_1, Info::new("Collapsed", "1.0.0"));
    let mut inserted = 0;

    for index in 0..2 {
        collapsed.paths.insert(
            &template,
            PathItem::new()
                .with_operation(Method::Get, Operation::new(format!("getItem{index}")))
                .with_operation(Method::Post, Operation::new(format!("addItem{index}"))),
        );
        inserted += 2;
    }

    let declared = declared_operations(&collapsed);
    let entries = collapsed.paths.items.len();

    assert!(
        declared < inserted,
        "{inserted} operations were inserted under one template and the document declares \
         {declared}; a count reporting the number it was asked to build sees no collapse at all, \
         which is the whole of what the size guard in `document` is for"
    );
    assert!(
        declared > entries,
        "the collapsed document holds {entries} Path Items declaring {declared} operations; a \
         count of entries reads a Path Item's second method as nothing at all, and would agree \
         with the guard only while the fixture kept one operation per path"
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
            A_CEILING_MOVES_BY_DECISION,
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

/// A fixture whose heap operations are fixed by construction rather than
/// measured: one fresh allocation and one reallocation.
///
/// Deliberately one of each kind. `Vec::with_capacity` is one fresh allocation;
/// extending past that capacity is one *reallocation*, because a `Vec` that
/// outgrows its buffer asks the allocator to resize it rather than to hand out
/// a second one. A driver that reported only the first would be counting half
/// of what [`counted`] says it counts.
///
/// [`black_box`](std::hint::black_box) is what keeps both from being optimized
/// away: nothing reads the buffer, and a dead `Vec` is exactly the shape a
/// compiler is free to delete. The buffer is returned rather than dropped here,
/// so that its teardown lands outside the region exactly as every other
/// caller's does.
fn calibrating() -> Vec<u8> {
    let mut buffer = Vec::<u8>::with_capacity(1);
    buffer.extend_from_slice(&[0, 0]);

    std::hint::black_box(buffer)
}

/// What [`calibrating`] costs, by construction: one fresh allocation and one
/// reallocation.
///
/// A constructed target rather than a recorded measurement: it is what
/// [`calibrating`]'s body does, not what a run reported. That is the ground for
/// holding it at an equality where every number in [`SIZES`] is a ceiling —
/// those record what an emitter costs today and are meant to be beaten, and
/// this records what the instrument must report, which no improvement to this
/// crate can lower.
///
/// Confirmed against the instrument all the same, the way all nine recorded
/// numbers were: set to zero, and the reading transcribed out of the failure.
/// Two at baseline (`cargo nextest run -p kynos-openapi --test alloc`) and two
/// with `--all-features`, the two configurations this target is built at — the
/// same two [`EMIT_CEILINGS`] is recorded at, and the only item in the file
/// that needed a `#[cfg]` to hold both.
const CALIBRATION: usize = 2;

/// The instrument's second invariant, and the one every number above rests on:
/// a count is *every* heap operation the region saw, fresh allocations and
/// reallocations alike.
///
/// **Nothing else here could see this, because every assertion above is
/// one-sided.** The nine recorded numbers are ceilings compared with `<=`, so a
/// count that *falls* passes; `stays_sub_quadratic` compares one reading with
/// another, so a uniform fall leaves every growth factor where it was; and
/// `a_repeated_emission_costs_what_the_first_one_did` asserts two readings
/// agree, which a consistently wrong driver satisfies perfectly. Measured,
/// before this assertion existed: dropping `reallocations` from both summing
/// sites left this target at 8 run, 8 passed — at baseline and at
/// `--all-features` alike.
///
/// **The absolute rather than a delta, which is not the form
/// [`kynos`'s counting target](../../kynos/tests/alloc.rs) uses.** There the
/// region is a whole request, so a reading is mostly the routing path's
/// irreducible cost, and pinning it would turn every genuine routing
/// improvement into a red *instrument* test; the calibration is read against a
/// control at the same depth to cancel that. Here the region is the closure
/// handed to [`counted`] and nothing else, so a fixture is the entire content
/// of its own reading and there is no baseline to cancel. A control would be a
/// second region asserted to cost zero, subtracted from this one — ceremony
/// around `2 - 0`.
///
/// **What the absolute buys, and what it still cannot see.** It buys the case a
/// delta gives up: a driver under-reporting *every* region by the same amount
/// is invisible to a difference and is red here. What stays invisible is a
/// driver correct on a two-operation region and wrong on a larger one — one
/// that dropped every operation past some count, say, or every allocation above
/// some size. No fixture of a fixed cost can reach that, and widening this one
/// would only move the boundary rather than remove it. The ceilings above bound
/// it from the other side, since a driver that under-reported the emitter would
/// have to under-report it consistently to keep
/// `a_repeated_emission_costs_what_the_first_one_did` green.
///
/// It is filed here rather than restated per stage because since the fold above
/// there is one summing site, which is what lets one assertion reach both — and
/// is why it has to exist, since a single edit to [`counted`] now moves every
/// recorded number in this file at once. It does not restate
/// `work_on_another_thread_is_not_counted`, for the reason the module
/// documentation gives above: that property is `alloc_counter`'s rather than
/// any driver's, and is held once for the workspace.
#[test]
fn the_counter_reports_every_heap_operation_in_the_region() {
    let (allocations, buffer) = counted(calibrating);
    drop(buffer);

    assert_eq!(
        allocations, CALIBRATION,
        "a region performing {CALIBRATION} heap operations by construction — \
         one fresh allocation and one reallocation — was counted at \
         {allocations}. A driver that stopped counting one of the two kinds \
         reports fewer here, and every ceiling recorded above would pass it, as \
         would both growth relations and the repeat-invariance check"
    );
}
