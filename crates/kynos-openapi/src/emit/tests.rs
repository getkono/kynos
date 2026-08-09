use crate::{
    emit::downgrade,
    model::{
        document::{Document, SpecVersion},
        info::Info,
    },
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

#[test]
fn a_bare_document_emits_only_the_required_fields() {
    let json = document().to_json().expect("serializable");
    assert!(json.contains(r#""openapi": "3.1.2""#));
    assert!(json.contains(r#""title": "Orders""#));
    assert!(!json.contains("paths"));
    assert!(!json.contains("components"));
}

#[test]
fn emitting_the_declared_version_is_a_no_op() {
    let emitted = document()
        .emit(SpecVersion::V3_1)
        .expect("no 3.2 constructs");
    assert_eq!(emitted.openapi, "3.1.2");
}

#[test]
fn a_document_using_no_three_two_construct_has_no_blockers() {
    assert!(downgrade::three_two_only_constructs(&document()).is_empty());
}
