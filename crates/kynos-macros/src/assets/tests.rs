use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use quote::quote;
use syn::parse_quote;

use crate::assets::{
    args::{AssetArgs, DEFAULT_WARN_OVER},
    expand_inner, suggest,
    walk::{Limits, walk_within},
};

/// The fixture directory, beside this crate's manifest.
const FIXTURE: &str = "assets-fixture";

/// One invocation, expanded.
fn expand(tokens: proc_macro2::TokenStream) -> syn::Result<String> {
    let args: AssetArgs = syn::parse2(tokens)?;
    expand_inner(&args).map(|expanded| expanded.to_string())
}

/// The paths an expansion serves, in the order it lists them.
fn served(expanded: &str) -> Vec<&str> {
    expanded
        .split(":: kynos :: router :: assets :: Asset :: embedded")
        .skip(1)
        .filter_map(|entry| entry.split('"').nth(1))
        .collect()
}

/// The whole directory reaches the expansion, sorted, with dotfiles skipped.
#[test]
fn a_directory_becomes_one_asset_per_file() {
    let expanded = expand(quote! {
        struct Fixture;
        dir = #FIXTURE,
    })
    .expect("a walkable directory");

    // Exactly these, in this order: the walk finds them in whatever order
    // the file system lists them, and the set is sorted after it.
    assert_eq!(
        served(&expanded),
        ["a.txt", "c.map", "nested/b.css"],
        "{expanded}"
    );
    assert!(expanded.contains("COUNT : usize = 3usize"), "{expanded}");

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

const MALFORMED_SIZE: &str = "`warn_over` takes a size such as \"4MiB\" or the word \"none\"; \
                              the units are B, KiB, MiB and GiB";

/// One row per `syn::Error::new` site in `args.rs`.
fn grammar_cases() -> Vec<(&'static str, proc_macro2::TokenStream, &'static str)> {
    vec![
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
            "an empty exclusion",
            quote! {
                struct Fixture;
                dir = #FIXTURE,
                exclude = [""],
            },
            "an `exclude` entry is a file name or an extension beginning with a dot, and an \
             empty string is neither",
        ),
    ]
}

/// Asserts `outcome` was refused, in words beginning with `message`.
///
/// A prefix, because where a message goes on it is the operating system's own
/// wording, which no test here controls.
fn assert_refused(description: &str, outcome: &syn::Result<String>, message: &str) {
    let Err(error) = outcome else {
        panic!("{description} must be refused");
    };
    let reported = error.to_string();
    assert!(
        reported.starts_with(message),
        "{description} was refused as {reported:?}, not {message:?}"
    );
}

/// Each grammar rule refuses in its own words.
#[test]
fn every_grammar_refusal_has_a_case() {
    for (description, tokens, message) in grammar_cases() {
        assert_refused(description, &expand(tokens), message);
    }

    // Two more ways to the size site, which `grammar_cases` reaches once.
    for size in ["lots", "99999999999GiB"] {
        assert_refused(
            size,
            &expand(quote! {
                struct Fixture;
                dir = #FIXTURE,
                warn_over = #size,
            }),
            MALFORMED_SIZE,
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

// --- What the walk refuses ------------------------------------------------

/// A directory of this test's own under the system's temporary directory,
/// removed when dropped.
///
/// Named for the process and numbered within it, so no two share one whether
/// tests run in one process or many.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "kynos-macros-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // A directory left by a process that reused this id and was killed.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temporary directory is writable");
        // Canonical, so the walk's own paths and the expected messages agree
        // where the temporary directory is reached through a symlink.
        Self(path.canonicalize().expect("it was just created"))
    }

    /// Writes `relative`, creating its parents, with its own path as contents.
    fn file(&self, relative: &str) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent"))
            .expect("the temporary directory is writable");
        std::fs::write(&path, relative).expect("the temporary directory is writable");
        self
    }

    fn dir(&self) -> &str {
        self.0.to_str().expect("the temporary directory is UTF-8")
    }

    /// An invocation over this directory, with `options` after `dir`.
    fn args(&self, options: &proc_macro2::TokenStream) -> AssetArgs {
        let dir = self.dir();
        syn::parse2(quote! {
            struct Fixture;
            dir = #dir,
            #options
        })
        .expect("a well-formed invocation")
    }

    /// The expansion of an invocation over this directory.
    fn expand(&self, options: &proc_macro2::TokenStream) -> syn::Result<String> {
        expand_inner(&self.args(options)).map(|expanded| expanded.to_string())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Limits a test passes with three files.
const SMALL: Limits = Limits { depth: 1, files: 2 };

/// The walk's outcome under [`SMALL`].
fn walk_small(args: &AssetArgs) -> syn::Result<String> {
    walk_within(args, SMALL).map(|walked| walked.files.len().to_string())
}

/// One row per `syn::Error::new` site in `walk.rs` a test can reach: what
/// was walked, how it came out, and the start of what it must say.
///
/// Each row's directory is gone by the time the row is read; the outcome was
/// taken while it stood.
fn walk_cases() -> Vec<(&'static str, syn::Result<String>, String)> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut cases = vec![
        (
            "a directory that is not there",
            expand(quote! {
                struct Fixture;
                dir = "no-such-directory",
            }),
            "`no-such-directory` could not be read: ".to_owned(),
        ),
        (
            "a file given as the directory",
            expand(quote! {
                struct Fixture;
                dir = "Cargo.toml",
            }),
            format!(
                "`{}` could not be read: ",
                manifest
                    .join("Cargo.toml")
                    .canonicalize()
                    .expect("this crate has a manifest")
                    .display()
            ),
        ),
    ];

    let empty = Scratch::new();
    empty.file(".hidden").file("notes.txt");
    cases.push((
        "a directory holding only a dotfile and an excluded file",
        empty.expand(&quote!(exclude = ["notes.txt"],)),
        format!(
            "`{}` holds no servable file; an asset set that serves nothing is one no route needs",
            empty.dir()
        ),
    ));

    let deep = Scratch::new();
    deep.file("one/two/deep.txt");
    cases.push((
        "a file nested past the depth limit",
        walk_small(&deep.args(&quote!())),
        format!("`{}` nests deeper than 1 levels", deep.dir()),
    ));

    let many = Scratch::new();
    many.file("a.txt").file("b.txt").file("c.txt");
    cases.push((
        "more files than the limit",
        walk_small(&many.args(&quote!())),
        format!("`{}` holds more than 2 files", many.dir()),
    ));

    let inexpressible = Scratch::new();
    inexpressible.file("nested/a b.txt");
    cases.push((
        "a file whose name no path template can express",
        inexpressible.expand(&quote!()),
        "`nested/a b.txt` has a name no path template can express, so it cannot be described; \
         rename it, or add it to `exclude`"
            .to_owned(),
    ));

    // A socket is neither a directory nor a symlink, and has no contents to
    // read: what a build output holding one would hit.
    #[cfg(unix)]
    {
        let unreadable = Scratch::new();
        let socket = unreadable.0.join("s");
        let _listener =
            std::os::unix::net::UnixListener::bind(&socket).expect("a socket can be bound");
        cases.push((
            "a file that cannot be read",
            unreadable.expand(&quote!()),
            format!("`{}` could not be read: ", socket.display()),
        ));
    }

    cases
}

/// The sites in `walk.rs` no test can reach, each named.
///
/// `CARGO_MANIFEST_DIR` is set for every test `cargo` runs; a directory entry
/// and its file type fail only where the file system does mid-walk; and every
/// entry is under the root it was listed from, so `strip_prefix` cannot fail.
/// Off Unix there is no socket to make a file unreadable with.
const UNREACHABLE_HERE: usize = if cfg!(unix) { 4 } else { 5 };

/// Each walk refusal says what it refused, and the control beside it passes.
#[test]
fn every_walk_refusal_has_a_case() {
    for (description, outcome, message) in walk_cases() {
        assert_refused(description, &outcome, &message);
    }

    // The controls: one level less, one file fewer, and the inexpressible
    // file excluded, the way out its message names.
    let shallow = Scratch::new();
    shallow.file("one/shallow.txt").file("top.txt");
    assert_eq!(
        walk_small(&shallow.args(&quote!())).expect("within both limits"),
        "2"
    );

    let excluded = Scratch::new();
    excluded.file("a b.txt").file("a.txt");
    let expanded = excluded
        .expand(&quote!(exclude = ["a b.txt"],))
        .expect("the inexpressible file is excluded");
    assert_eq!(served(&expanded), ["a.txt"], "{expanded}");
}

/// A count, not a mapping, over both files `assets!` raises diagnostics from.
#[test]
fn every_assets_diagnostic_has_a_case() {
    let source = [include_str!("args.rs"), include_str!("walk.rs")].concat();
    let sites = source.matches("syn::Error::new(").count() + source.matches("meta.error(").count();
    let covered = grammar_cases().len() + walk_cases().len() + UNREACHABLE_HERE;
    assert_eq!(
        covered, sites,
        "`args.rs` and `walk.rs` raise {sites} diagnostic(s) and {covered} have a case or a \
         reason none can"
    );
}

/// Symlinks are skipped, so a set cannot reach outside its own directory.
#[cfg(unix)]
#[test]
fn a_symlink_is_not_followed() {
    let outside = Scratch::new();
    outside.file("secret.txt").file("tree/inner.txt");

    let set = Scratch::new();
    set.file("index.html");
    std::os::unix::fs::symlink(outside.0.join("secret.txt"), set.0.join("secret.txt"))
        .expect("a symlink can be made");
    std::os::unix::fs::symlink(outside.0.join("tree"), set.0.join("tree"))
        .expect("a symlink can be made");

    let expanded = set.expand(&quote!()).expect("a walkable directory");
    assert_eq!(served(&expanded), ["index.html"], "{expanded}");
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
