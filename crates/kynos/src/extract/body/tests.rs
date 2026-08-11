use kynos_openapi::Schema as OpenApiSchema;

use crate::{
    extract::{
        body::binary::Binary,
        describe::RequestContent,
        media::{OctetStream, Png},
    },
    schema::registry::Registry,
};

/// Raw binary is described by what is *left out*, which is easy to get wrong in
/// the direction of saying too much.
///
/// `type` is absent because raw binary sits outside the types JSON Schema
/// describes, and `contentMediaType` is absent because it would only repeat the
/// key the content sits under. What remains is the empty Schema Object.
#[test]
fn a_raw_binary_body_states_no_type_and_repeats_no_media_type() {
    let body = Binary::<Png>::request_body(&mut Registry::default());

    let content = body
        .content
        .get("image/png")
        .expect("the body is keyed by its own media type");

    let Some(OpenApiSchema::Object(schema)) = &content.schema else {
        panic!("expected a keyword-carrying schema rather than a boolean or nothing");
    };
    assert_eq!(schema.ty, None, "raw binary is outside `type`");
    assert_eq!(
        schema.format, None,
        "`format: binary` is the OpenAPI 3.0 spelling and is deprecated"
    );
    assert_eq!(
        schema.content_media_type, None,
        "a `contentMediaType` repeating the content key is redundant, and a \
         contradicting one is ignored by the specification"
    );
    assert_eq!(schema.content_encoding, None, "these bytes are not encoded");
}

/// The media type reaches the description from the marker, so two markers
/// produce two different keys from one type.
#[test]
fn the_marker_chooses_the_content_key() {
    let png = Binary::<Png>::request_body(&mut Registry::default());
    let bytes = Binary::<OctetStream>::request_body(&mut Registry::default());

    assert!(png.content.contains_key("image/png"));
    assert!(bytes.content.contains_key("application/octet-stream"));
}

/// The documented remedy for a variable number of uploads has to compile.
///
/// `multipart.rs` tells a reader to declare one field of type `Vec<FilePart>`,
/// and `MultipartForm<T>` requires `T: Schema` — so a `FilePart` without one
/// makes the only advice the module gives impossible to take.
#[cfg(feature = "multipart")]
mod a_file_part_describes_itself {
    use kynos_openapi::Schema as OpenApiSchema;

    use crate::{
        extract::body::multipart::FilePart,
        schema::{Schema, registry::Registry},
    };

    /// A part's bytes are raw binary, and its media type belongs to the
    /// Encoding Object rather than to the schema — where a `contentMediaType`
    /// would contradict it and be ignored.
    #[test]
    fn a_part_is_raw_binary() {
        let OpenApiSchema::Object(schema) = FilePart::schema(&mut Registry::default()) else {
            panic!("expected a keyword-carrying schema rather than a boolean");
        };
        assert_eq!(schema.ty, None, "raw binary is outside `type`");
        assert_eq!(schema.format, None, "`format: binary` is the 3.0 spelling");
        assert_eq!(
            schema.content_media_type, None,
            "the Encoding Object carries the part's media type"
        );
    }

    /// An anonymous schema is inlined; a part has no component name to `$ref`.
    #[test]
    fn a_part_is_not_a_component() {
        assert!(FilePart::name().is_none());
    }
}
