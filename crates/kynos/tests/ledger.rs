//! Counts held against `kynos-macros`, which only this repository can make.
//!
//! Every other target here asserts something about `kynos` alone. These three
//! read the macro crate's source for what it declares, and compare that against
//! a set the suites next door witness — so a macro added without a witness
//! fails the build rather than expanding to whatever it likes, and a refusal
//! the `Schema` derive documents without a snapshot fails it rather than going
//! unchecked.
//!
//! That is why they are not in the files whose witnesses they count.
//! `crates/kynos/Cargo.toml` keeps this target out of the published archive:
//! `cargo package` carries a package directory and nothing beside it, so
//! `../../kynos-macros/` does not exist in a tarball and an assertion written
//! against it could only fail there. The claim is a property of the workspace,
//! and the workspace is where it is checked.

use std::{fs, path::Path};

/// The macro crate's entry points, as text.
fn macro_crate() -> String {
    fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kynos-macros/src/lib.rs"
    ))
    .expect("the macro crate's entry points are readable from the workspace")
}

/// The derives, counted against the entry points that declare them.
///
/// `every_derive_implements_its_trait` in [`derives.rs`](derives.rs) witnesses
/// a set someone chose, and nothing tied that set to the macros the crate
/// actually exports. A derive added without a witness is one that could expand
/// to anything — and eight witnesses against ten entry points is the state this
/// test was written to end.
///
/// A count rather than a mapping: it catches a derive added without a witness,
/// and not a witness renamed to cover a different one.
#[test]
fn every_derive_has_a_witness() {
    /// Every derive witnessed in `derives.rs`. `Provider` is exercised by
    /// `the_provider_derive_supplies_every_field_it_was_not_told_to_skip`,
    /// `ApiError` and `Reply` through `implements_responses`,
    /// `MultipartForm` through `implements_multipart`, and the rest by
    /// `every_derive_implements_its_trait`.
    const WITNESSED: &[&str] = &[
        "ApiError",
        "CookieParams",
        "HeaderParams",
        "MultipartForm",
        "PathParams",
        "Provider",
        "QueryParams",
        "Reply",
        "Schema",
        "SecurityScheme",
        "Tag",
    ];

    let declared = macro_crate().matches("#[proc_macro_derive(").count();
    assert_eq!(
        declared,
        WITNESSED.len(),
        "`kynos-macros` declares {declared} derive(s) and {} are witnessed; a derive added \
         without one is a derive nothing asks to implement its trait",
        WITNESSED.len()
    );
}

/// The attributes the `Schema` derive's rustdoc lists as refused, each named
/// against the snapshot that holds its refusal.
///
/// `every_rejected_schema_type_has_a_case` in [`ui.rs`](ui.rs) holds the type
/// table to its cases, and this list had no counterpart: three of its five
/// entries once had nothing behind them. A mapping rather than a count, per
/// "Name the set where the set has names" in `docs/testing.md` — counting
/// `ui/macros/schema_*` would also count grammar rules no entry lists.
#[test]
fn every_rejected_schema_attribute_has_a_case() {
    /// A fragment only its own entry carries, and the snapshot of its refusal
    /// under `tests/ui/`.
    const RECORDED: &[(&str, &str)] = &[
        (
            "`#[serde(with = ...)]`",
            "macros/schema_serialize_with.stderr",
        ),
        ("`#[serde(untagged)]`", "macros/schema_untagged_enum.stderr"),
        ("`#[serde(flatten)]`", "macros/schema_flatten_map.stderr"),
        (
            "`#[serde(other)]`",
            "macros/schema_catch_all_variant.stderr",
        ),
        (
            "`skip_serializing_if` on a non-`Option` field",
            "macros/schema_skip_serializing_if_without_default.stderr",
        ),
    ];
    const HEADING: &str = "# Rejected, because serde and the schema would disagree";

    let entries = listed_entries(&macro_crate(), HEADING);
    assert!(
        !entries.is_empty(),
        "no list was found under `{HEADING}` in `kynos-macros/src/lib.rs`; this test reads the \
         entries between that heading and `#[proc_macro_derive(Schema`"
    );
    assert_eq!(
        entries.len(),
        RECORDED.len(),
        "`Schema` lists {} refused attribute(s) and {} are recorded; an entry added without a \
         snapshot is a refusal nothing checks: {entries:#?}",
        entries.len(),
        RECORDED.len()
    );

    // Every entry names exactly one recorded refusal. Counting entries alone
    // lets an unbacked entry stand in for a deleted one whose fragment another
    // entry now mentions in passing.
    for entry in &entries {
        let recorded = RECORDED
            .iter()
            .filter(|(fragment, _)| entry.contains(fragment))
            .count();
        assert_eq!(
            recorded, 1,
            "an entry under `{HEADING}` names {recorded} recorded refusal(s), and each names \
             exactly one: {entry:?}"
        );
    }

    for (fragment, snapshot) in RECORDED {
        let naming = entries
            .iter()
            .filter(|entry| entry.contains(fragment))
            .count();
        assert_eq!(
            naming, 1,
            "{naming} entries under `{HEADING}` name {fragment}, and a recorded fragment names \
             exactly one: {entries:#?}"
        );

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui")
            .join(snapshot);
        let recorded = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !recorded.trim().is_empty(),
            "{fragment} is listed as refused, and `{}` holds no snapshot of the refusal",
            path.display()
        );
    }
}

/// The entries of the rustdoc list under `heading` in the `Schema` derive's
/// documentation, each joined across its continuation lines.
fn listed_entries(source: &str, heading: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let list = source
        .lines()
        .map(str::trim_start)
        .skip_while(|line| !line.ends_with(heading))
        .skip(1)
        .take_while(|line| !line.starts_with("#[proc_macro_derive(Schema"));

    for line in list {
        if let Some(entry) = line.strip_prefix("/// - ") {
            entries.push(entry.to_owned());
        } else if let (Some(continued), Some(entry)) =
            (line.strip_prefix("///   "), entries.last_mut())
        {
            entry.push(' ');
            entry.push_str(continued.trim());
        }
    }
    entries
}

/// The route attributes, counted against the entry points that declare them.
///
/// The cases are in [`pipeline.rs`](pipeline.rs). Under `openapi32`, because
/// `query` is gated there and the full set only exists in that build — which is
/// the one `mise run test` uses.
#[cfg(feature = "openapi32")]
#[test]
fn every_route_attribute_has_a_case() {
    /// The eight ungated attributes, `query`, and `operation`.
    const WITNESSED: usize = 10;

    let declared = macro_crate().matches("#[proc_macro_attribute]").count();
    assert_eq!(
        declared, WITNESSED,
        "`kynos-macros` declares {declared} attribute(s) and {WITNESSED} are witnessed; an \
         attribute added without a case is one whose method nothing reads"
    );
}
