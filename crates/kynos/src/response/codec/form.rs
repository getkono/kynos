//! Writing `application/x-www-form-urlencoded` as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    extract::body::form::Form,
    http::{HeaderValue, Response, body::Body},
    response::{
        IntoResponse, Responses,
        codec::{ok, unserializable, with_content_type},
        negotiate::representation::{Representation, sealed::SealedRepresentation},
    },
    schema::{Schema, registry::Registry},
};

impl<T: serde::Serialize> IntoResponse for Form<T> {
    fn into_response(self) -> Response {
        // Encoded in full first, so a failure can still change the status.
        let Ok(encoded) = serde_html_form::to_string(&self.0) else {
            return unserializable("the response body could not be encoded as a form");
        };

        with_content_type(
            Body::from_bytes(bytes::Bytes::from(encoded)),
            HeaderValue::from_static(mime_names::APPLICATION_FORM_URLENCODED),
        )
    }
}

impl<T: Schema> Responses for Form<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        ok(
            mime_names::APPLICATION_FORM_URLENCODED,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}

impl<T> SealedRepresentation for Form<T> {
    const MEDIA_TYPE: &'static str = mime_names::APPLICATION_FORM_URLENCODED;
}

impl<T: serde::Serialize + Schema> Representation for Form<T> {}
