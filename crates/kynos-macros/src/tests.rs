//! The sweep over every diagnostic ledger in this crate.
//!
//! Each ledger counts its rows against the `syn::Error::new` and `meta.error`
//! sites of the files it names with `include_str!`. A count only holds for a
//! file somebody named, so this walks the source tree, rather than a list of
//! it, and fails for a file that raises a diagnostic no ledger names, or that
//! raises one in a spelling no ledger counts.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// Every `.rs` file under `directory`, recursively.
fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("the source tree is readable") {
        let path = entry.expect("the source tree is readable").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The files a test module names with `include_str!`, resolved the way the
/// macro resolves them: against the directory of the file it is written in.
fn included(test: &Path, source: &str) -> Vec<PathBuf> {
    let directory = test.parent().expect("a file has a parent");
    source
        .split("include_str!(\"")
        .skip(1)
        .map(|rest| {
            let (relative, _) = rest.split_once('"').expect("a closed string literal");
            directory
                .join(relative)
                .canonicalize()
                .unwrap_or_else(|error| panic!("`{relative}` in {test:?}: {error}"))
        })
        .collect()
}

#[test]
fn every_file_raising_a_diagnostic_is_named_by_a_ledger() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);

    let (tests, implementation): (Vec<PathBuf>, Vec<PathBuf>) = files
        .into_iter()
        .partition(|path| path.file_name().is_some_and(|name| name == "tests.rs"));

    let mut counted = BTreeSet::new();
    for test in &tests {
        let source = fs::read_to_string(test).expect("the source tree is readable");
        counted.extend(included(test, &source));
    }

    let mut raising = 0;
    for path in implementation {
        let source = fs::read_to_string(&path).expect("the source tree is readable");
        let name = path
            .strip_prefix(&root)
            .expect("walked from the root")
            .display();

        // Each ledger counts these two spellings and no other, so another
        // way to build the same error would be a site no count sees.
        assert_eq!(
            source.matches("Error::new(").count(),
            source.matches("syn::Error::new(").count(),
            "`{name}` builds a `syn::Error` under another path; write `syn::Error::new(`, the \
             spelling every ledger counts"
        );
        assert!(
            !source.contains("Error::new_spanned("),
            "`{name}` calls `syn::Error::new_spanned`; write `syn::Error::new(tokens.span(), \
             ..)`, the spelling every ledger counts"
        );
        assert_eq!(
            source.matches(".error(").count(),
            source.matches("meta.error(").count(),
            "`{name}` raises an error through a receiver other than `meta`; build it with \
             `syn::Error::new(`, the spelling every ledger counts"
        );

        if source.contains("syn::Error::new(") || source.contains("meta.error(") {
            raising += 1;
            assert!(
                counted.contains(&path.canonicalize().expect("it was just read")),
                "`{name}` raises a diagnostic and no ledger names it with `include_str!`; a rule \
                 nobody wrote a case for is a rule that can stop firing silently"
            );
        }
    }

    // The control: the walk found the files this crate is known to raise from.
    assert!(raising > 20, "the sweep found only {raising} raising files");
}
