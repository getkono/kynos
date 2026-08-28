//! Counts held against `kynos-macros`, which only this repository can make.
//!
//! Every other target here asserts something about `kynos` alone. These two
//! read the macro crate's source to count what it declares, and compare that
//! against a set the suites next door witness — so a macro added without a
//! witness fails the build rather than expanding to whatever it likes.
//!
//! That is why they are not in the files whose witnesses they count.
//! `crates/kynos/Cargo.toml` keeps this target out of the published archive:
//! `cargo package` carries a package directory and nothing beside it, so
//! `../../kynos-macros/` does not exist in a tarball and an assertion written
//! against it could only fail there. The claim is a property of the workspace,
//! and the workspace is where it is checked.

use std::fs;

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
