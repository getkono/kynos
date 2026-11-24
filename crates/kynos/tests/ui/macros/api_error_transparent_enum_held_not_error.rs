//! A transparent enum is an `Error` whose source is the held error's, so a
//! held error that displays but is no `Error` is reported at its field, and
//! an enum with no `Debug`, which every `Error` is, at the derive.
//!
//! The control is `SharedSourced` in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, ApiError)]
#[problem(status = 404)]
struct LookupError;

impl std::fmt::Display for LookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("no user with that id")
    }
}

#[derive(Debug, ApiError)]
#[problem(transparent)]
enum HeldNotError {
    Lookup(LookupError),
}

#[derive(thiserror::Error, ApiError)]
#[error("the journal could not be written")]
#[problem(status = 503)]
struct JournalUnwritable;

impl std::fmt::Debug for JournalUnwritable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JournalUnwritable")
    }
}

#[derive(ApiError)]
#[problem(transparent)]
enum NotDebug {
    Journal(JournalUnwritable),
}

fn main() {}
