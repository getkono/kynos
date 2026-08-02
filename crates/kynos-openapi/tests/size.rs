//! Size guards for the document model.
//!
//! A `PathItem` holds one slot per HTTP method, and an `Operation` is over a
//! kilobyte. Storing operations inline made a `PathItem` 8.7 KB, which every
//! `Paths` entry then paid for on insert and on rehash. The operation slots are
//! boxed to avoid that, and these assertions keep it that way.
//!
//! The exact numbers may drift as fields are added; the ratios are what matter,
//! so the bounds are deliberately loose.

use kynos_openapi::{Operation, PathItem, Schema, SchemaObject};

#[test]
fn a_path_item_does_not_inline_its_operations() {
    let path_item = size_of::<PathItem>();
    let operation = size_of::<Operation>();

    assert!(
        path_item < operation,
        "PathItem ({path_item} bytes) should not inline Operation ({operation} bytes); \
         the per-method slots must stay boxed"
    );
    assert!(
        path_item <= 512,
        "PathItem grew to {path_item} bytes; box any large field that was added"
    );
}

#[test]
fn a_schema_does_not_inline_its_keywords() {
    let schema = size_of::<Schema>();
    let object = size_of::<SchemaObject>();

    assert!(
        schema < object,
        "Schema ({schema} bytes) should not inline SchemaObject ({object} bytes); \
         schemas nest deeply and are cloned often"
    );
    assert!(
        schema <= 32,
        "Schema grew to {schema} bytes; it is the most-copied type in the model"
    );
}
