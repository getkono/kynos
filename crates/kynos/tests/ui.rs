//! The `trybuild` UI suite: what a rejected program's diagnostic actually says.
//!
//! Every compile-fail case has a sibling in `ui/pass/` differing in exactly the
//! property under test, per the pass-control rule in `docs/testing.md`. A
//! negative on its own cannot tell "the rule holds" from "the surface is
//! unusable", because `compile_fail` passes for any reason at all.
//!
//! Snapshots are recorded under the toolchain `mise.toml` pins, and under
//! `--all-features`. Both pins matter: rustc rewords diagnostics between
//! releases, and its "the following other types implement" list enumerates
//! implementations that feature flags add and remove. Re-record with
//! `TRYBUILD=overwrite cargo nextest run -p kynos --test ui --all-features`.
//!
//! Cases that could only fail with a feature *off* are therefore not here.
//! [`ui/PENDING.md`](ui/PENDING.md) names them.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn ui() {
    let cases = trybuild::TestCases::new();

    cases.compile_fail("tests/ui/macros/*.rs");
    cases.compile_fail("tests/ui/schema/*.rs");
    cases.compile_fail("tests/ui/antipattern/*.rs");
    cases.pass("tests/ui/pass/*.rs");
}

/// One `ui/schema/` case per row of the rejection table in `kynos::schema`.
///
/// A count, not a mapping: it catches a row added without a case, which is the
/// drift that actually happens, and does not catch a case renamed to cover a
/// different row. Several rows also list more than one type — the row for
/// `serde_json::Value` names `Map` and `RawValue` too — so the case count is a
/// floor on coverage rather than a measure of it. The pass-control rule is
/// enforced by review, not here.
///
/// The table is the specification of which types are deliberately undescribable,
/// and a row nobody wrote a case for is a rule nobody checks. Counting here
/// turns adding a row without adding a case into a test failure.
#[test]
fn every_rejected_schema_type_has_a_case() {
    const SOURCE: &str = include_str!("../src/schema/mod.rs");

    let rows = rejection_table_rows(SOURCE);
    assert!(
        rows > 0,
        "the rejection table in `schema/mod.rs` was not found; this test is looking for the \
         heading `# Types deliberately left without an implementation`"
    );

    let cases = compile_fail_cases(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui/schema"));
    assert_eq!(
        cases, rows,
        "`tests/ui/schema/` holds {cases} case(s) for {rows} row(s) of the rejection table in \
         `schema/mod.rs`; a row added without a case leaves a documented refusal that nothing \
         checks"
    );
}

/// The body rows of the Markdown table under the rejection heading.
fn rejection_table_rows(source: &str) -> usize {
    source
        .lines()
        .map(str::trim_start)
        .skip_while(|line| !line.ends_with("# Types deliberately left without an implementation"))
        .map(|line| line.trim_start_matches("//!").trim())
        // The table ends at the first line that is not part of it.
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        // The header row and the `| --- |` separator describe the table rather
        // than populating it. Dropped by position rather than by content, so a
        // row that happens to contain a dash still counts.
        .skip(2)
        .count()
}

/// The `.rs` files directly inside `directory`.
fn compile_fail_cases(directory: PathBuf) -> usize {
    fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|it| it == "rs"))
        .count()
}
