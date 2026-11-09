//! The reader every JSON input shares, which reads a struct only from an
//! object, in `extract/strict_json.rs`.
//!
//! The input is one document, read as a `serde_json::Value` and as a
//! recursive struct. As a `Value`, which holds no struct, it reads exactly as
//! `serde_json` reads it. As the struct, whatever it reads `serde_json` reads
//! the same, and whatever `serde_json` refuses it refuses; where `serde_json`
//! reads the struct, it refuses exactly when a walk of the document finds an
//! array at a position the struct is read from.
//!
//! A refusal is a schema failure, the 422 side of the 400/422 line, only
//! where `serde_json`'s own refusal is one, or where `serde_json` skims the
//! whole document as `IgnoredAny`, the well-formedness check the reader's
//! syntax-error-wins rule makes; so a `Value` is never refused as one. That
//! skim, like `serde_json` reading a member a struct skips, checks no string
//! for UTF-8. Its pointer is a JSON Pointer.

#![no_main]

use kynos::__private::fuzz::strict_json;
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;
use serde_json::Value;

/// A struct read at the root, through `Option`, `Box` and a sequence.
#[derive(Debug, Deserialize, PartialEq)]
struct Shape {
    #[serde(default)]
    child: Option<Box<Shape>>,
    #[serde(default)]
    items: Vec<Shape>,
    #[serde(default)]
    n: Option<i64>,
}

/// Whether every position `value` is read as a [`Shape`] from is not an
/// array.
fn objects_only(value: &Value) -> bool {
    match value {
        Value::Array(_) => false,
        Value::Object(members) => {
            members
                .get("child")
                .is_none_or(|child| child.is_null() || objects_only(child))
                && members
                    .get("items")
                    .and_then(Value::as_array)
                    .is_none_or(|items| items.iter().all(objects_only))
        }
        _ => true,
    }
}

/// Checks a refusal against `serde_json`'s of the same type.
fn refused(data: &[u8], (schema, pointer): &(bool, String), error: &serde_json::Error) {
    if *schema {
        let skims = serde_json::from_slice::<serde::de::IgnoredAny>(data).is_ok();
        assert!(
            error.is_data() || skims,
            "{data:?} refused as a schema failure, not as {error}"
        );
    }
    assert!(
        pointer.is_empty() || pointer.starts_with('/'),
        "{pointer:?}"
    );
}

fuzz_target!(|data: &[u8]| {
    let read = strict_json::<Value>(data);
    match (&read, serde_json::from_slice::<Value>(data)) {
        (Ok(read), Ok(expected)) => assert_eq!(*read, expected, "{data:?}"),
        (Err(failure), Err(error)) => refused(data, failure, &error),
        (read, expected) => panic!("{data:?} read {read:?}, expected {expected:?}"),
    }

    // The walk's view of the document. A member the struct skips is never
    // checked for UTF-8 or a valid escape, which a `Value` holds it to: lossy
    // text answers the first, and a document that fails the second is not
    // walked.
    let walked = || serde_json::from_str::<Value>(&String::from_utf8_lossy(data)).ok();
    let read = strict_json::<Shape>(data);
    match (read, serde_json::from_slice::<Shape>(data)) {
        (Ok(read), Ok(expected)) => {
            assert_eq!(read, expected, "{data:?}");
            if let Some(value) = walked() {
                assert!(objects_only(&value), "{data:?} read from an array");
            }
        }
        (Ok(read), Err(error)) => panic!("{data:?} read {read:?}, refused {error}"),
        (Err(failure), Ok(_)) => {
            if let Some(value) = walked() {
                assert!(!objects_only(&value), "{data:?} refused as {failure:?}");
            }
            assert!(failure.0, "{data:?} refused as {failure:?}");
        }
        (Err(failure), Err(error)) => refused(data, &failure, &error),
    }
});
