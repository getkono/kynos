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

#[test]
fn an_encoding_declaring_a_style_no_query_parameter_could_take_is_refused() {
    use crate::model::body::encoding::Encoding;

    // An encoded property is serialized the way a query parameter is, so the
    // path, header and cookie styles are not a combination to report but words
    // `EncodingStyle` cannot read.
    for style in ["matrix", "label", "simple", "cookie"] {
        let json = format!(r#"{{"style":"{style}"}}"#);
        serde_json::from_str::<Encoding>(&json)
            .expect_err("an encoding takes only the query styles");
    }
}

#[test]
fn each_query_style_round_trips_through_an_encoding() {
    use crate::model::{body::encoding::Encoding, parameter::style::EncodingStyle};

    for style in [
        EncodingStyle::Form,
        EncodingStyle::SpaceDelimited,
        EncodingStyle::PipeDelimited,
        EncodingStyle::DeepObject,
    ] {
        let encoding = Encoding {
            style: Some(style),
            ..Encoding::default()
        };

        let json = serde_json::to_string(&encoding).expect("serializable");
        let parsed: Encoding = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(parsed, encoding);
    }
}
