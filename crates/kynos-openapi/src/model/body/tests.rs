use crate::model::{
    body::{RequestBody, media_type::MediaType, mime_names},
    schema::{Schema, types::SchemaType},
};

#[test]
fn a_json_body_is_required_by_default() {
    let body = RequestBody::json(Schema::of_type(SchemaType::Object));
    assert_eq!(body.required, Some(true));
    assert!(body.content.contains_key(mime_names::APPLICATION_JSON));
}

#[test]
fn a_body_can_offer_several_representations() {
    let body = RequestBody::json(Schema::of_type(SchemaType::Object)).with_media_type(
        mime_names::APPLICATION_FORM_URLENCODED,
        MediaType::new(Schema::of_type(SchemaType::Object)),
    );
    assert_eq!(body.content.len(), 2);
}

#[test]
fn optional_bodies_say_so() {
    let body = RequestBody::json(Schema::any()).optional();
    assert_eq!(body.required, Some(false));
}
