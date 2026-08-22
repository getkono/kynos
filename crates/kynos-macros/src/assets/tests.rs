use quote::quote;
use syn::parse_quote;

use crate::assets::{args::AssetArgs, expand_inner};

/// The fixture directory, beside this crate's manifest.
const FIXTURE: &str = "assets-fixture";

/// One invocation, expanded.
fn expand(tokens: proc_macro2::TokenStream) -> syn::Result<String> {
    let args: AssetArgs = syn::parse2(tokens)?;
    expand_inner(&args).map(|expanded| expanded.to_string())
}

/// The whole directory reaches the expansion, sorted, with dotfiles skipped.
#[test]
fn a_directory_becomes_one_asset_per_file() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
    })
    .expect("a walkable directory");

    assert!(expanded.contains("\"a.txt\""), "{expanded}");
    assert!(expanded.contains("\"nested/b.css\""), "{expanded}");
    assert!(expanded.contains("\"c.map\""), "{expanded}");

    // A dotfile is not part of a build output, and `.git` is the case that
    // matters: embedding it would put a repository in a binary.
    assert!(!expanded.contains(".hidden"), "{expanded}");

    // The contents arrive through `include_bytes!`, so a changed file rebuilds.
    assert!(expanded.contains("include_bytes"), "{expanded}");
}

/// `exclude` drops a file by extension.
#[test]
fn an_excluded_extension_is_not_embedded() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
        exclude = [".map"],
    })
    .expect("a walkable directory");

    assert!(!expanded.contains("\"c.map\""), "{expanded}");
    assert!(expanded.contains("\"a.txt\""), "{expanded}");
}

/// And by name.
#[test]
fn an_excluded_name_is_not_embedded() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
        exclude = ["a.txt"],
    })
    .expect("a walkable directory");

    assert!(!expanded.contains("\"a.txt\""), "{expanded}");
}

/// Two files with the same contents get the same tag; different contents do
/// not.
///
/// The whole obligation on an entity tag, per RFC 9110 section 8.8.3: it
/// changes when the representation does.
#[test]
fn the_tag_follows_the_contents() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
    })
    .expect("a walkable directory");

    let tags: Vec<&str> = expanded
        .split("\" , :: core :: include_bytes")
        .skip(1)
        .filter_map(|chunk| chunk.split('"').nth(1))
        .collect();

    let mut unique = tags.clone();
    unique.sort_unstable();
    unique.dedup();

    assert_eq!(
        tags.len(),
        unique.len(),
        "three files with different contents shared a tag: {tags:?}"
    );
}

// --- The size guard -------------------------------------------------------

/// A set past the threshold emits a warning at the `dir` literal.
///
/// `proc_macro::Diagnostic` is nightly, so the warning is produced by *using*
/// an item this expansion marked `#[deprecated]`. Asserting on the tokens
/// because that is what the macro crate can see; what the message reads like is
/// this test's other half.
#[test]
fn an_oversized_set_emits_a_deprecation_the_compiler_reports() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
        warn_over = "1B",
    })
    .expect("a walkable directory");

    assert!(expanded.contains("deprecated"), "{expanded}");
    assert!(
        expanded.contains("this_embedded_asset_set_is_large"),
        "{expanded}"
    );
    // And the use is *not* allowed, because using it is the whole mechanism.
    assert!(
        !expanded.contains("allow (deprecated)"),
        "the expansion silences the warning it exists to produce: {expanded}"
    );
    // The message names the cost, the way out, and the override.
    assert!(expanded.contains("slow to link"), "{expanded}");
    assert!(expanded.contains("warn_over"), "{expanded}");
}

/// The control: a set inside the threshold emits nothing.
///
/// Without it, "past the threshold warns" would read as "every set warns".
#[test]
fn a_set_within_the_threshold_emits_no_deprecation() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
        warn_over = "1MiB",
    })
    .expect("a walkable directory");

    assert!(!expanded.contains("deprecated"), "{expanded}");
}

/// And the check can be turned off outright.
#[test]
fn the_guard_can_be_turned_off() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
        warn_over = "none",
    })
    .expect("a walkable directory");

    assert!(!expanded.contains("deprecated"), "{expanded}");
    // The count is still emitted, so a build script can assert on it.
    assert!(expanded.contains("TOTAL_BYTES"), "{expanded}");
}

// --- What the grammar refuses ---------------------------------------------

/// One case per diagnostic, counted against the sites.
#[test]
fn every_grammar_refusal_has_a_case() {
    let cases: &[(&str, proc_macro2::TokenStream)] = &[
        (
            "no unit struct to name the set",
            quote! {
                dir = #FIXTURE,
            },
        ),
        (
            "no directory at all",
            quote! {
                struct Fixture;
            },
        ),
        (
            "a directory that is not there",
            quote! {
                struct Fixture;
                dir = "no-such-directory",
            },
        ),
        (
            "a key outside the grammar",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                nonsense = "x",
            },
        ),
        (
            "a size in units nobody agrees on",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = "4KB",
            },
        ),
        (
            "a size that is not a number",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = "lots",
            },
        ),
        (
            "an empty exclusion",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                exclude = [""],
            },
        ),
    ];

    for (description, tokens) in cases {
        assert!(
            expand(tokens.clone()).is_err(),
            "{description} must be refused"
        );
    }

    // The control: the same shape, legal.
    assert!(
        expand(quote! {
            struct Fixture;
            dir = #FIXTURE,
            exclude = [".map"],
            warn_over = "4MiB",
        })
        .is_ok()
    );
}

/// A trailing comma is allowed, which is what makes the last option look like
/// every other one.
#[test]
fn a_trailing_comma_is_accepted_and_so_is_its_absence() {
    for tokens in [
        quote! {
            struct Fixture;
            dir = #FIXTURE,
        },
        quote! {
            struct Fixture;
            dir = #FIXTURE
        },
    ] {
        assert!(expand(tokens).is_ok());
    }
}

/// The visibility and doc comments reach the type the macro mints.
#[test]
fn the_minted_type_keeps_what_it_was_given() {
    let expanded = expand(quote! {
        /// The built single-page app.
        pub struct Site;
        dir = #FIXTURE,
    })
    .expect("a walkable directory");

    assert!(expanded.contains("pub struct Site"), "{expanded}");
    assert!(expanded.contains("The built single-page app"), "{expanded}");
}

/// A parse that never reaches the walk is still a parse.
#[test]
fn the_grammar_is_read_before_the_directory_is() {
    let args: syn::Result<AssetArgs> = syn::parse2(quote! {
        struct Fixture;
        dir = "no-such-directory",
    });

    assert!(args.is_ok(), "the grammar is fine; the directory is not");
    let _: AssetArgs = parse_quote! {
        struct Fixture;
        dir = "no-such-directory",
    };
}
