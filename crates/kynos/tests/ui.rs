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

use std::{fs, path::Path};

#[test]
fn ui() {
    let cases = trybuild::TestCases::new();

    cases.compile_fail("tests/ui/macros/*.rs");
    cases.compile_fail("tests/ui/schema/*.rs");
    cases.compile_fail("tests/ui/antipattern/*.rs");
    cases.compile_fail("tests/ui/traits/*.rs");
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

    let cases = compile_fail_cases(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui/schema"));
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
fn compile_fail_cases(directory: &Path) -> usize {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|it| it == "rs"))
        .count()
}

/// Every trait carrying `#[diagnostic::on_unimplemented]`, against the snapshot
/// that records what it says.
///
/// [`nfr.md`](https://github.com/getkono/kynos/blob/master/docs/nfr.md) marks "no diagnostic names an internal
/// type" enforced by this suite, and eight of the fourteen guided traits had no
/// snapshot at all: `Handler`, `Describe`, `RequestContent`, `Alternative`,
/// `MapKey`, `ShortCircuit`, `EndpointMeta` and `IntoEndpoints`. The attribute
/// is what replaces rustc's generic "the trait bound is not satisfied" with a
/// message naming the fix, so an unsnapshotted one is a message nobody checks.
///
/// An explicit mapping rather than a search for the trait's name, because half
/// the messages deliberately never spell it: `Handler`'s says "is not a Kynos
/// handler", which is the improvement being made and not something to grep for.
#[test]
fn every_guided_diagnostic_has_a_snapshot() {
    /// One row per `#[diagnostic::on_unimplemented]` in `crates/kynos/src`.
    const RECORDED: &[(&str, &str)] = &[
        ("Alternative", "traits/alternative.stderr"),
        ("ByteSource", "traits/byte_source.stderr"),
        ("Carries", "traits/carries.stderr"),
        ("Describe", "traits/describe.stderr"),
        ("EndpointMeta", "traits/endpoint_meta.stderr"),
        ("FromRequest", "antipattern/raw_request_extractor.stderr"),
        (
            "FromRequestParts",
            "antipattern/raw_header_map_extractor.stderr",
        ),
        ("Handler", "traits/handler.stderr"),
        ("IntoEndpoints", "traits/into_endpoints.stderr"),
        ("IntoResponse", "antipattern/bare_status_code.stderr"),
        ("Languages", "traits/languages.stderr"),
        ("MapKey", "traits/map_key.stderr"),
        ("Provides", "antipattern/inject_without_provider.stderr"),
        ("Rangeable", "traits/rangeable.stderr"),
        ("RequestContent", "traits/request_content.stderr"),
        ("Responses", "antipattern/problem_as_return_type.stderr"),
        ("Schema", "schema/serde_json_value.stderr"),
        ("ShortCircuit", "traits/short_circuit.stderr"),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (trait_name, snapshot) in RECORDED {
        let path = root.join("tests/ui").join(snapshot);
        let recorded = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("`{trait_name}` names {snapshot}, which is not there"));
        assert!(
            !recorded.trim().is_empty(),
            "`{trait_name}`'s snapshot is empty, so it records no diagnostic"
        );
    }

    let guided = guided_traits_in_source(&root.join("src"));
    assert_eq!(
        guided,
        RECORDED.len(),
        "`crates/kynos/src` guides {guided} trait(s) and {} have a snapshot; a diagnostic nobody \
         snapshotted is a diagnostic nobody checked",
        RECORDED.len()
    );
}

/// The `#[diagnostic::on_unimplemented]` attributes under `directory`.
fn guided_traits_in_source(directory: &Path) -> usize {
    let mut found = 0;
    for entry in fs::read_dir(directory).expect("the crate's own source is readable") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            found += guided_traits_in_source(&path);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        found += fs::read_to_string(&path)
            .expect("readable source")
            .matches("#[diagnostic::on_unimplemented")
            .count();
    }
    found
}

/// Every `Router` and `Group` builder carries all four type parameters across.
///
/// Both types are parameterised by `<C, P, I, S>`, and two of those are the
/// phantom lists the compile-time conflict check reads: `I`, the interceptors
/// mounted on this scope, and `S`, what the scopes mounted here brought with
/// them. A builder naming fewer than four in its return type resets the ones it
/// omits to their default of `()`, and the check then compares a newcomer
/// against an empty list and finds nothing to collide with.
///
/// This has gone wrong twice. `catch_panics` returned `Router<C, Catch>` and
/// dropped `I`. That fix landed before `S` existed, and the commit adding `S`
/// updated the group's half and not the router's, so it returned
/// `Router<C, Catch, I>` and dropped `S` instead. Both were silent by
/// construction: the signature is well-formed, every caller still compiles, and
/// only a program that *should* have been refused shows the difference.
///
/// A count of the arguments rather than a mapping of the methods, for the
/// reason `every_rejected_schema_type_has_a_case` gives: it catches the drift
/// that actually happens -- a parameter dropped from a return type -- and does
/// not pretend to check that the parameter carried is the right one. Methods
/// returning `Self` need no case: every `impl` block declaring one is
/// `impl<C, P, I, S>`, so `Self` is all four by construction.
#[test]
fn every_builder_preserves_the_type_parameters() {
    const PARAMETERS: usize = 4;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;

    for relative in ["src/router/mod.rs", "src/router/group.rs"] {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).expect("the crate's own source is readable");

        for (number, line) in source.lines().enumerate() {
            // Comments are skipped so a rustdoc paragraph naming a return type
            // -- this file's own prose does -- is not read as one.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }

            for type_name in ["Router", "Group"] {
                let opening = format!("-> {type_name}<");
                let Some(at) = line.find(&opening) else {
                    continue;
                };

                let arguments = &line[at + opening.len()..];
                let count = top_level_arguments(arguments).unwrap_or_else(|| {
                    panic!(
                        "{relative}:{} returns `{type_name}<` whose argument list does not close \
                         on one line; this test reads return types a line at a time",
                        number + 1
                    )
                });

                assert_eq!(
                    count,
                    PARAMETERS,
                    "{relative}:{} returns `{type_name}<` with {count} type argument(s) and \
                     `{type_name}` has {PARAMETERS}; the omitted one falls back to its default of \
                     `()`, which silently empties a phantom list the conflict check reads. Name \
                     all {PARAMETERS}, or return `Self`.",
                    number + 1
                );

                checked += 1;
            }
        }
    }

    assert!(
        checked > 0,
        "no `-> Router<` or `-> Group<` return type was found; this test is looking in \
         `src/router/mod.rs` and `src/router/group.rs`, and finding none means it has stopped \
         checking anything"
    );
}

/// The comma-separated arguments of a generic list whose opening `<` is already
/// consumed, or `None` when it does not close in `text`.
///
/// Depth-aware, so `Router<C, P, I, <E::Stacks as Flatten<S>>::Out>` counts
/// four rather than five: only a comma directly inside the outer list
/// separates an argument.
fn top_level_arguments(text: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut separators = 0;

    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(separators + 1);
                }
            }
            ',' if depth == 1 => separators += 1,
            _ => {}
        }
    }

    None
}
