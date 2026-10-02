use quote::quote;
use syn::parse_quote;

use crate::assets::{
    args::{AssetArgs, DEFAULT_WARN_OVER},
    expand_inner, suggest,
};

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
    // The fixture is a few bytes, so the suggested override is the smallest
    // one `suggest` offers, quoted the way the user would write it.
    assert!(
        expanded.contains(r#"Raise the threshold with `warn_over = \"1MiB\"`"#),
        "{expanded}"
    );
    assert!(
        expanded.contains(r#"turn the check off with `warn_over = \"none\"`"#),
        "{expanded}"
    );
}

/// The suggested threshold is the next power-of-two mebibyte at or above the
/// total, and never below one mebibyte.
#[test]
fn the_suggestion_is_the_next_power_of_two_mebibyte() {
    const MIB: usize = 1024 * 1024;

    for (total, suggested) in [
        (0, "1MiB"),
        (1, "1MiB"),
        (MIB, "1MiB"),
        (MIB + 1, "2MiB"),
        (2 * MIB, "2MiB"),
        (2 * MIB + 1, "4MiB"),
        (3 * MIB, "4MiB"),
        (5 * MIB, "8MiB"),
        (1024 * MIB, "1024MiB"),
    ] {
        assert_eq!(suggest(total), suggested, "for {total} bytes");
    }
}

/// The threshold `warn_over` resolves to, for one invocation.
fn warn_over(tokens: proc_macro2::TokenStream) -> Option<usize> {
    syn::parse2::<AssetArgs>(tokens)
        .expect("a well-formed invocation")
        .warn_over
}

/// Each IEC unit scales by its own power of 1024, and the default is two
/// mebibytes.
///
/// A multiplier of 3 rather than 1 keeps `number * scale` apart from
/// `number + scale` and from the bare scale.
#[test]
fn each_size_unit_scales_exactly() {
    assert_eq!(DEFAULT_WARN_OVER, 2_097_152);
    assert_eq!(
        warn_over(quote! {
            struct Fixture;
            dir = #FIXTURE,
        }),
        Some(DEFAULT_WARN_OVER),
        "an invocation without `warn_over` takes the default"
    );

    for (size, bytes) in [
        ("0B", Some(0)),
        ("3B", Some(3)),
        ("3KiB", Some(3_072)),
        ("3MiB", Some(3_145_728)),
        ("3GiB", Some(3_221_225_472)),
        ("none", None),
    ] {
        assert_eq!(
            warn_over(quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = #size,
            }),
            bytes,
            "`warn_over = {size:?}`"
        );
    }
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

/// One case per diagnostic, counted against the sites, each refused in its own
/// words.
#[test]
fn every_grammar_refusal_has_a_case() {
    const MALFORMED_SIZE: &str = "`warn_over` takes a size such as \"4MiB\" or the word \
                                  \"none\"; the units are B, KiB, MiB and GiB";

    // The expected message is a prefix where the rest is the operating
    // system's own wording, which no test here controls.
    let cases: &[(&str, proc_macro2::TokenStream, &str)] = &[
        (
            "no unit struct to name the set",
            quote! {
                dir = #FIXTURE,
            },
            "an asset set needs a unit struct to name it: `pub struct Site;`",
        ),
        (
            "no directory at all",
            quote! {
                struct Fixture;
            },
            "an asset set needs `dir = \"...\"`, relative to the crate root",
        ),
        (
            "a directory that is not there",
            quote! {
                struct Fixture;
                dir = "no-such-directory",
            },
            "`no-such-directory` could not be read: ",
        ),
        (
            "a key outside the grammar",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                nonsense = "x",
            },
            "`nonsense` is not part of the `assets!` grammar, which takes `dir`, `exclude` and \
             `warn_over`",
        ),
        (
            "a size in units nobody agrees on",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = "4KB",
            },
            MALFORMED_SIZE,
        ),
        (
            "a size that is not a number",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = "lots",
            },
            MALFORMED_SIZE,
        ),
        (
            "an empty exclusion",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                exclude = [""],
            },
            "an `exclude` entry is a file name or an extension beginning with a dot, and an \
             empty string is neither",
        ),
    ];

    for (description, tokens, message) in cases {
        let Err(error) = expand(tokens.clone()) else {
            panic!("{description} must be refused");
        };
        let reported = error.to_string();
        assert!(
            reported.starts_with(message),
            "{description} was refused as {reported:?}, not {message:?}"
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
