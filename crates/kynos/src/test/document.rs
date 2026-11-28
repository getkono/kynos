//! Holding a committed description to the one the code builds.
//!
//! An application that commits its OpenAPI description, for a client
//! generator or as a reviewed contract, tests that the file has not drifted
//! from the router: [`assert_file_matches`] is that test, over the
//! [canonical form](kynos_openapi::Document::to_canonical_json), whose bytes
//! no build and no reordering of the code changes.

use std::{
    ffi::OsStr,
    fmt::Write as _,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use kynos_openapi::Document;

/// The variable that, set to `1`, has [`assert_file_matches`] rewrite the file.
const UPDATE_VARIABLE: &str = "KYNOS_UPDATE_DOCUMENTS";

/// Lines of unchanged text shown on each side of a difference.
const CONTEXT: usize = 3;

/// Lines shown of each side of a difference before the rest is counted.
const SHOWN: usize = 40;

/// Asserts that the file at `path` holds `document`, as
/// [`Document::to_canonical_json`] writes it.
///
/// A relative `path` is read from the package under test, the directory
/// holding its `Cargo.toml`, which Cargo names in `CARGO_MANIFEST_DIR` when it
/// runs a test; a test run outside Cargo passes an absolute one.
///
/// On a difference this panics with a line diff of the file against the
/// document. Run the test again with `KYNOS_UPDATE_DOCUMENTS=1` and it writes the document to the file instead
/// — and still panics, saying so, so a run that changed the file is never a
/// passing one, in CI or anywhere else. Review the change, commit it, and run
/// the test again without the variable. A file that already matches is left
/// alone and passes either way.
///
/// ```no_run
/// use kynos::prelude::*;
///
/// #[test]
/// fn the_committed_description_is_current() -> kynos::Result<()> {
///     let document = Router::<()>::new().openapi()?;
///     kynos::test::document::assert_file_matches("openapi.json", &document);
///     Ok(())
/// }
/// ```
///
/// A test holding a [`TestClient`](crate::test::TestClient) passes
/// [`client.openapi()`](crate::test::TestClient::openapi), and one committing
/// a 3.1 description from an `openapi32` build passes
/// [`Router::openapi_as`](crate::router::Router::openapi_as)'s document.
///
/// # Panics
///
/// Where the file differs from the document or is missing; where
/// `KYNOS_UPDATE_DOCUMENTS` is set to anything but `1` or the empty string;
/// where `path` is relative and `CARGO_MANIFEST_DIR` is unset; where the file
/// cannot be read or written; and where the document has no canonical form.
#[track_caller]
pub fn assert_file_matches(path: impl AsRef<Path>, document: &Document) {
    check(
        path.as_ref(),
        std::env::var_os("CARGO_MANIFEST_DIR").as_deref(),
        std::env::var_os(UPDATE_VARIABLE).as_deref(),
        document,
    );
}

/// [`assert_file_matches`] over the two variables' values as read.
///
/// Every panic is raised in this frame or a `#[track_caller]` one, never in
/// a closure, which `#[track_caller]` does not reach.
#[track_caller]
fn check(path: &Path, manifest_dir: Option<&OsStr>, update: Option<&OsStr>, document: &Document) {
    let path = resolve(path, manifest_dir);
    let mode = Mode::from_variable(update);
    let emitted = match document.to_canonical_json() {
        Ok(emitted) => emitted,
        Err(error) => panic!("the document has no canonical JSON: {error}"),
    };
    let committed = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => panic!("`{}` could not be read: {error}", path.display()),
    };

    match judge(&path, committed.as_deref(), &emitted, mode) {
        Verdict::Matches => {}
        Verdict::Rewrite => {
            if let Err(error) = std::fs::write(&path, &emitted) {
                panic!("`{}` could not be written: {error}", path.display());
            }
            panic!(
                "`{}` now holds the document this code builds, since {UPDATE_VARIABLE}=1; a run \
                 that rewrites the file fails, so it is never mistaken for one that checked \
                 it. Review and commit the change, then run again without {UPDATE_VARIABLE}.",
                path.display()
            );
        }
        Verdict::Differs(report) => panic!("{report}"),
    }
}

/// What [`UPDATE_VARIABLE`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Unset or empty: compare only.
    Compare,
    /// `1`: write the document where the file differs.
    Update,
}

impl Mode {
    /// Reads the variable's value, refusing one that reads as either.
    #[track_caller]
    fn from_variable(value: Option<&OsStr>) -> Self {
        match value.map(OsStr::to_str) {
            None | Some(Some("")) => Self::Compare,
            Some(Some("1")) => Self::Update,
            Some(_) => panic!(
                "{UPDATE_VARIABLE} is `{}`; set it to `1` to rewrite the file, or unset it to \
                 compare",
                value.map(OsStr::to_string_lossy).unwrap_or_default()
            ),
        }
    }
}

/// `path` as read: against `manifest_dir` where it is relative.
#[track_caller]
fn resolve(path: &Path, manifest_dir: Option<&OsStr>) -> PathBuf {
    if path.is_absolute() {
        return path.to_owned();
    }
    let Some(manifest_dir) = manifest_dir else {
        panic!(
            "`{}` is relative, and CARGO_MANIFEST_DIR, which it is read against, is unset; run \
             the test through Cargo or pass an absolute path",
            path.display()
        );
    };
    Path::new(manifest_dir).join(path)
}

/// What becomes of one comparison.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// The file holds the document.
    Matches,
    /// The file does not, and the run asked for it to be written.
    Rewrite,
    /// The file does not, reported for a reader.
    Differs(String),
}

/// Compares the file's bytes, or their absence, with the emitted document.
fn judge(path: &Path, committed: Option<&[u8]>, emitted: &str, mode: Mode) -> Verdict {
    if committed == Some(emitted.as_bytes()) {
        return Verdict::Matches;
    }
    if mode == Mode::Update {
        return Verdict::Rewrite;
    }
    let Some(committed) = committed else {
        return Verdict::Differs(format!(
            "`{}` does not exist; run with {UPDATE_VARIABLE}=1 to write the document there",
            path.display()
        ));
    };
    let committed = String::from_utf8_lossy(committed);
    let mut report = format!(
        "`{}` is not the document this code builds; if the change is intended, run with \
         {UPDATE_VARIABLE}=1 and review the rewrite",
        path.display()
    );
    if committed.contains('\r') {
        report.push_str(
            "\nthe file has carriage returns, which the document never does: check that git \
             does not convert its line endings",
        );
    }
    report.push('\n');
    report.push_str(&line_diff(&committed, emitted));
    Verdict::Differs(report)
}

/// The lines `committed` and `emitted` differ in, as one unified-diff hunk.
///
/// The hunk spans from the first differing line to the last, so it is exact
/// for one change and covers everything between where there are several;
/// either side beyond [`SHOWN`] lines is counted rather than printed.
fn line_diff(committed: &str, emitted: &str) -> String {
    let old: Vec<&str> = committed.split('\n').collect();
    let new: Vec<&str> = emitted.split('\n').collect();

    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);

    let start = prefix.saturating_sub(CONTEXT);
    let after = suffix.min(CONTEXT);
    let mut diff = format!(
        "--- the file\n+++ the document\n@@ -{},{} +{},{} @@\n",
        start + 1,
        old_end - start + after,
        start + 1,
        new_end - start + after,
    );
    for line in &old[start..prefix] {
        let _ = writeln!(diff, " {line}");
    }
    for (sign, lines) in [('-', &old[prefix..old_end]), ('+', &new[prefix..new_end])] {
        for line in lines.iter().take(SHOWN) {
            let _ = writeln!(diff, "{sign}{line}");
        }
        if lines.len() > SHOWN {
            let _ = writeln!(diff, "{sign}... {} more lines", lines.len() - SHOWN);
        }
    }
    for line in &old[old_end..old_end + after] {
        let _ = writeln!(diff, " {line}");
    }
    diff
}

#[cfg(test)]
mod tests;
