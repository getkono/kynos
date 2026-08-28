use super::{Document, SpecVersion};
use crate::model::{info::Info, schema::dialect::OAS_DIALECT};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

#[test]
fn a_new_document_declares_its_version() {
    assert_eq!(document().openapi, "3.1.2");
    assert_eq!(document().spec_version(), Some(SpecVersion::V3_1));
}

#[test]
fn the_version_is_matched_on_major_and_minor_only() {
    let mut doc = document();
    doc.openapi = "3.1.0".to_owned();
    assert_eq!(doc.spec_version(), Some(SpecVersion::V3_1));
}

#[test]
fn an_unmodelled_version_is_reported_as_unknown() {
    let mut doc = document();
    doc.openapi = "3.0.4".to_owned();
    assert_eq!(doc.spec_version(), None);
}

#[test]
fn the_dialect_defaults_to_the_oas_dialect() {
    assert_eq!(document().effective_dialect(), OAS_DIALECT);
}

#[test]
fn three_one_does_not_claim_three_two_support() {
    assert!(!SpecVersion::V3_1.supports_3_2());
}
