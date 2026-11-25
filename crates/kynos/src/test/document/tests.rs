use std::{ffi::OsStr, path::Path};

use kynos_openapi::{Document, Info, SpecVersion};

use super::{Mode, Verdict, check, judge, line_diff, resolve};
use crate::test::tests::fails_here;

const PATH: &str = "/spec/openapi.json";

fn judged(committed: Option<&str>, emitted: &str, mode: Mode) -> Verdict {
    judge(Path::new(PATH), committed.map(str::as_bytes), emitted, mode)
}

fn report(verdict: Verdict) -> String {
    match verdict {
        Verdict::Differs(report) => report,
        other => panic!("expected a difference, got {other:?}"),
    }
}

/// Only `1` asks for a rewrite; unset or empty compares.
#[test]
fn the_update_variable_reads_one_as_update_and_absence_as_compare() {
    assert_eq!(Mode::from_variable(None), Mode::Compare);
    assert_eq!(Mode::from_variable(Some(OsStr::new(""))), Mode::Compare);
    assert_eq!(Mode::from_variable(Some(OsStr::new("1"))), Mode::Update);
}

/// A value that might mean either, as `true` or `0`, is refused by name
/// rather than read as one of them.
#[test]
#[should_panic(expected = "KYNOS_UPDATE_DOCUMENTS is `true`; set it to `1`")]
fn any_other_update_value_is_refused_by_name() {
    Mode::from_variable(Some(OsStr::new("true")));
}

/// A relative path is read from the package's manifest directory, and an
/// absolute one as written.
#[test]
fn a_relative_path_is_read_against_the_manifest_directory() {
    let manifest = Some(OsStr::new("/work/app"));
    assert_eq!(
        resolve(Path::new("../spec/openapi.json"), manifest),
        Path::new("/work/app/../spec/openapi.json")
    );
    assert_eq!(resolve(Path::new(PATH), manifest), Path::new(PATH));
    assert_eq!(resolve(Path::new(PATH), None), Path::new(PATH));
}

/// Without the manifest directory a relative path has no base, so it is
/// refused rather than read against wherever the process happens to be.
#[test]
#[should_panic(expected = "`openapi.json` is relative, and CARGO_MANIFEST_DIR")]
fn a_relative_path_without_a_manifest_directory_is_refused() {
    resolve(Path::new("openapi.json"), None);
}

/// A file holding the document passes in either mode, so an update run over
/// a current file writes nothing and passes.
#[test]
fn a_matching_file_passes_in_either_mode() {
    for mode in [Mode::Compare, Mode::Update] {
        assert_eq!(judged(Some("{}\n"), "{}\n", mode), Verdict::Matches);
    }
}

/// An update run rewrites a file that differs, and one that is missing.
#[test]
fn an_update_rewrites_a_differing_or_missing_file() {
    assert_eq!(judged(Some("{}"), "{}\n", Mode::Update), Verdict::Rewrite);
    assert_eq!(judged(None, "{}\n", Mode::Update), Verdict::Rewrite);
}

/// A missing file names how to write it.
#[test]
fn a_missing_file_is_reported_with_how_to_write_it() {
    assert_eq!(
        report(judged(None, "{}\n", Mode::Compare)),
        "`/spec/openapi.json` does not exist; run with KYNOS_UPDATE_DOCUMENTS=1 to write the \
         document there"
    );
}

/// A differing file is reported with the diff, and with line endings named
/// where the file has carriage returns, since the diff cannot show them.
#[test]
fn a_differing_file_is_reported_with_its_diff() {
    let report = report(judged(Some("{\r\n}\r\n"), "{\n}\n", Mode::Compare));
    assert!(
        report.starts_with(
            "`/spec/openapi.json` is not the document this code builds; if the change is \
             intended, run with KYNOS_UPDATE_DOCUMENTS=1 and review the rewrite\nthe file has \
             carriage returns"
        ),
        "{report}"
    );
    assert!(
        report.ends_with(&line_diff("{\r\n}\r\n", "{\n}\n")),
        "{report}"
    );
}

/// One changed line is shown with three lines of context on each side, and
/// the hunk header counts what it shows.
#[test]
fn a_line_diff_shows_one_change_in_its_context() {
    let committed = "1\n2\n3\n4\nold\n6\n7\n8\n9\n";
    let emitted = "1\n2\n3\n4\nnew\n6\n7\n8\n9\n";
    assert_eq!(
        line_diff(committed, emitted),
        "--- the file\n+++ the document\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-old\n+new\n 6\n 7\n 8\n"
    );
}

/// Lines only one side has, at either end, are shown without inventing a
/// change on the other.
#[test]
fn a_line_diff_shows_an_insertion_and_a_truncation() {
    assert_eq!(
        line_diff("a\nc\n", "a\nb\nc\n"),
        "--- the file\n+++ the document\n@@ -1,3 +1,4 @@\n a\n+b\n c\n \n"
    );
    assert_eq!(
        line_diff("a\nb", "a\n"),
        "--- the file\n+++ the document\n@@ -1,2 +1,2 @@\n a\n-b\n+\n"
    );
}

/// A side longer than the limit is counted past it, not printed whole.
#[test]
fn a_long_side_is_counted_past_the_limit() {
    let committed = "x\n".repeat(50);
    let diff = line_diff(&committed, "");
    assert_eq!(diff.matches("\n-x").count(), super::SHOWN);
    assert!(diff.ends_with("\n-... 10 more lines\n \n"), "{diff}");
}

/// Every way the assertion fails is reported at the test's own line, not
/// inside kynos: the document without a canonical form, the variable, the
/// relative path, the read, the write, and the difference.
#[test]
fn every_failure_is_reported_at_the_callers_line() {
    let document = Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"));
    let mut twice = document.clone();
    twice.extensions.insert("info", 1);
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Neither exists, so reading finds nothing and writing has no directory.
    let missing = package.join("no-such-directory/openapi.json");
    let update = Some(OsStr::new("1"));

    fails_here!(check(&missing, None, None, &twice));
    fails_here!(check(&missing, None, Some(OsStr::new("true")), &document));
    fails_here!(check(Path::new("openapi.json"), None, None, &document));
    fails_here!(check(package, None, None, &document));
    fails_here!(check(&missing, None, update, &document));
    fails_here!(check(&missing, None, None, &document));
}
