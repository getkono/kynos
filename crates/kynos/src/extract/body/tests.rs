use kynos_openapi::Schema as OpenApiSchema;

use crate::{
    extract::{body::binary::Binary, describe::RequestContent},
    http::media::{OctetStream, Png},
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

/// A form body decodes the encoding its description states.
///
/// No Encoding Object is written, so each property takes the defaults:
/// `style: form` with `explode: true`, under which an array property is one
/// pair per item, all under the property's name. A field the description calls
/// an array therefore has to accept a repeated key, and a single pair as a
/// one-item array.
#[cfg(feature = "form")]
mod a_form_body_decodes_what_it_describes {
    use std::collections::BTreeMap;

    use crate::{
        error::rejection::BodyRejection,
        extract::{FromRequest, body::form::Form},
        http::{HeaderValue, Request, StatusCode, body::Body, header},
        schema::{Schema, registry::Registry},
    };

    /// Gives each type an open schema declaring no bound: these tests read
    /// the decoding, which no bound takes part in.
    macro_rules! unbounded {
        ($($ty:ty),+ $(,)?) => {$(
            impl Schema for $ty {
                fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
                    let _ = registry;
                    kynos_openapi::Schema::any()
                }
            }
        )+};
    }

    /// Reads `T` from a form body holding `body`.
    async fn read<T: serde::de::DeserializeOwned + Schema + Send>(
        body: &'static [u8],
    ) -> Result<T, BodyRejection> {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        Form::<T>::from_request(request, &())
            .await
            .map(|Form(value)| value)
    }

    #[derive(Debug, serde::Deserialize)]
    struct Tags {
        tag: Vec<String>,
    }
    unbounded!(Tags);

    #[tokio::test]
    async fn a_repeated_key_is_one_item_per_pair_in_order() {
        let tags = read::<Tags>(b"tag=a&tag=b").await.expect("an array");

        assert_eq!(tags.tag, ["a", "b"]);
    }

    #[tokio::test]
    async fn a_single_pair_is_a_one_item_array() {
        let tags = read::<Tags>(b"tag=a").await.expect("an array");

        assert_eq!(tags.tag, ["a"]);
    }

    /// The items are parsed as the item type, not only collected as text.
    #[tokio::test]
    async fn the_items_are_read_as_the_item_type() {
        #[derive(Debug, serde::Deserialize)]
        struct Ids {
            id: Vec<u32>,
        }
        unbounded!(Ids);

        let ids = read::<Ids>(b"id=3&other=x&id=5").await.expect("an array");

        assert_eq!(ids.id, [3, 5]);
    }

    /// A scalar property is one pair. A second one does not fit the schema,
    /// so it is refused as a 422 rather than resolved by picking one of the
    /// two values.
    #[tokio::test]
    async fn a_repeated_scalar_key_is_422() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Page {
            page: u32,
        }
        unbounded!(Page);

        let rejection = read::<Page>(b"page=1&page=2")
            .await
            .expect_err("a scalar takes one pair");

        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let BodyRejection::Schema { failures } = rejection else {
            panic!("expected a schema failure, got {rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), [""]);
    }

    /// A map of scalars is held to the same rule as a struct of them.
    #[tokio::test]
    async fn a_repeated_key_into_a_map_of_scalars_is_422() {
        let rejection = read::<BTreeMap<String, String>>(b"a=1&a=2")
            .await
            .expect_err("a scalar value takes one pair");

        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// An empty number input is submitted as `name=`, which is no number; an
    /// optional field reads it as absent rather than refusing the form.
    #[tokio::test]
    async fn an_empty_optional_number_is_none() {
        #[derive(Debug, serde::Deserialize)]
        struct Filter {
            limit: Option<u32>,
        }
        unbounded!(Filter);

        let filter = read::<Filter>(b"limit=").await.expect("an absent number");

        assert_eq!(filter.limit, None);
    }

    /// The two shapes `Form`'s documentation says are described but not
    /// decoded, held to the 422 it promises for them.
    #[tokio::test]
    async fn a_nested_struct_and_a_flattened_number_are_422() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Inner {
            n: u32,
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Nested {
            inner: Inner,
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Flattened {
            #[serde(flatten)]
            inner: Inner,
        }
        unbounded!(Nested, Flattened);

        let nested = read::<Nested>(b"n=1").await.expect_err("not decoded");
        let flattened = read::<Flattened>(b"n=1").await.expect_err("not decoded");

        assert_eq!(nested.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(flattened.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

/// The message a protobuf body carries is the one the extractor yields.
///
/// A non-default value, so an extractor that skipped the decode and yielded
/// `T::default()` cannot pass.
#[cfg(feature = "protobuf")]
#[tokio::test]
async fn a_protobuf_body_decodes_to_the_message_it_encodes() {
    use crate::{
        extract::{FromRequest, body::protobuf::Protobuf},
        http::{HeaderValue, Request, body::Body, header},
    };

    #[derive(Clone, PartialEq, prost::Message)]
    struct Tagged {
        #[prost(int32, tag = "1")]
        value: i32,
    }

    // Field 1, varint wire type, value 7 -- the whole message.
    let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(&[0x08, 0x07])));
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/protobuf"),
    );

    let Protobuf(message) = Protobuf::<Tagged>::from_request(request, &())
        .await
        .expect("a well-formed message");

    assert_eq!(message, Tagged { value: 7 });
}
