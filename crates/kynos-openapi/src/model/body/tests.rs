use crate::model::{
    body::{RequestBody, media_type::MediaType, mime_names},
    schema::{Schema, types::SchemaType},
};

#[test]
fn a_body_can_offer_several_representations() {
    let body = RequestBody::json(Schema::of_type(SchemaType::Object)).with_media_type(
        mime_names::APPLICATION_FORM_URLENCODED,
        MediaType::new(Schema::of_type(SchemaType::Object)),
    );
    assert_eq!(body.content.len(), 2);
}

/// A `content` entry carrying `$ref` is the Reference Object 3.2 admits there
/// (`references/3.2.0.md:1210`), never a Media Type Object whose extensions
/// absorbed the key, and it writes back as the bare reference it was read as.
#[test]
fn a_content_entry_carrying_a_ref_reads_and_writes_as_a_reference() {
    use crate::model::reference::{Ref, RefOr};

    let wire = serde_json::json!({
        "content": {"application/json": {"$ref": "#/components/mediaTypes/Json"}}
    });
    let body: RequestBody = serde_json::from_value(wire.clone()).expect("a request body");

    assert_eq!(
        body.content["application/json"],
        RefOr::Ref(Ref::new("#/components/mediaTypes/Json"))
    );
    assert_eq!(serde_json::to_value(&body).expect("serializable"), wire);
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
fn a_body_states_on_the_wire_whether_it_is_required() {
    // The exact-JSON case the Value type row owes. A round-trip cannot stand
    // in for it: nothing here sets `deny_unknown_fields`, so a misspelled
    // `required` would be absorbed by the flattened extensions, written back
    // unchanged, and compare equal -- while the real field stayed `None` from
    // end to end. `required` is the field that would go silently, because it
    // is what says a body may be omitted.
    assert_eq!(
        serde_json::to_string(&RequestBody::json(Schema::any())).expect("serializable"),
        r#"{"content":{"application/json":{"schema":true}},"required":true}"#
    );

    assert_eq!(
        serde_json::to_string(&RequestBody::json(Schema::any()).optional()).expect("serializable"),
        r#"{"content":{"application/json":{"schema":true}},"required":false}"#
    );
}
