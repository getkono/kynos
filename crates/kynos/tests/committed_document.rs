//! `test::document::assert_file_matches` over real files and a real process
//! environment.
//!
//! The comparison and its report are unit-tested beside the function; this is
//! the half they cannot reach: the file read and written, and
//! `KYNOS_UPDATE_DOCUMENTS` read from the environment, which a test sets only
//! for a child process, since setting it in its own would be `unsafe`.

#![cfg(feature = "test-util")]

use std::{path::PathBuf, process::Command};

use kynos::{Router, openapi::Document, test::document::assert_file_matches};

/// Where the child test reads the file it is to rewrite.
const CHILD_FILE: &str = "KYNOS_TEST_COMMITTED_DOCUMENT";

fn document() -> Document {
    Router::<()>::new()
        .openapi()
        .expect("an empty router describes itself")
}

/// A file of this test's own under Cargo's scratch directory for the target.
fn file(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("committed_document")
        .join(format!("{name}.json"));
    std::fs::create_dir_all(path.parent().expect("a file has a directory"))
        .expect("the scratch directory is writable");
    path
}

#[test]
fn a_file_holding_the_canonical_form_passes() {
    let path = file("matching");
    let document = document();
    std::fs::write(&path, document.to_canonical_json().expect("serializable"))
        .expect("the file is writable");

    assert_file_matches(&path, &document);
}

/// `to_json`'s bytes are the same document in another order, which a
/// committed file is not allowed to be.
#[test]
#[should_panic(expected = "is not the document this code builds")]
fn a_file_holding_the_model_order_fails() {
    let path = file("model-order");
    let document = document();
    std::fs::write(&path, document.to_json().expect("serializable")).expect("the file is writable");

    assert_file_matches(&path, &document);
}

#[test]
#[should_panic(expected = "does not exist; run with KYNOS_UPDATE_DOCUMENTS=1")]
fn a_missing_file_fails() {
    let path = file("missing");
    let _ = std::fs::remove_file(&path);

    assert_file_matches(&path, &document());
}

/// The child half of [`an_update_rewrites_the_file_and_fails_the_run`],
/// which does nothing unless that test names it a file.
#[test]
fn rewrite_the_file_named_by_the_parent() {
    if let Some(path) = std::env::var_os(CHILD_FILE) {
        assert_file_matches(PathBuf::from(path), &document());
    }
}

/// `KYNOS_UPDATE_DOCUMENTS=1` writes the document, and the run still fails,
/// so no run that changed the file is ever green.
#[test]
fn an_update_rewrites_the_file_and_fails_the_run() {
    let path = file("update");
    std::fs::write(&path, "{}\n").expect("the file is writable");

    let output = Command::new(std::env::current_exe().expect("a test binary knows its path"))
        .args([
            "--exact",
            "rewrite_the_file_named_by_the_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KYNOS_UPDATE_DOCUMENTS", "1")
        .env(CHILD_FILE, &path)
        .output()
        .expect("the test binary re-executes");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "the rewriting run passed");
    assert!(
        stderr.contains("now holds the document this code builds"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file is readable"),
        document().to_canonical_json().expect("serializable")
    );
}
