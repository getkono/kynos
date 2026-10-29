//! Writing `application/json`, or the `+json` type a `JsonAs` names, as a
//! response.

use kynos_openapi::model::body::mime_names;

use crate::{
    http::{HeaderValue, Response, body::Body},
    response::{
        IntoResponse, Responses,
        codec::{json_bytes, ok, unserializable, with_content_type},
        negotiate::representation::{Representation, sealed::SealedRepresentation},
    },
    schema::{Schema, registry::Registry},
};

use crate::{
    extract::body::{json::Json, json_as::JsonAs},
    http::media::MediaType,
};

/// `value` as a JSON body under `media_type`, or the 500 a value that cannot
/// be serialized answers.
fn json_response<T: serde::Serialize>(value: &T, media_type: &'static str) -> Response {
    // Serialized in full first, so a failure can still change the status.
    let Ok(bytes) = json_bytes(value) else {
        return unserializable("the response body could not be serialized");
    };

    with_content_type(
        Body::from_bytes(bytes),
        HeaderValue::from_static(media_type),
    )
}

/// The one 200 a JSON body describes: `T`'s schema under `media_type`.
fn json_responses<T: Schema>(
    registry: &mut Registry,
    media_type: &'static str,
) -> kynos_openapi::Responses {
    ok(
        media_type,
        kynos_openapi::MediaType::new(registry.resolve::<T>()),
    )
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        json_response(&self.0, mime_names::APPLICATION_JSON)
    }
}

impl<T: Schema> Responses for Json<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        json_responses::<T>(registry, mime_names::APPLICATION_JSON)
    }
}

impl<M: MediaType, T: serde::Serialize> IntoResponse for JsonAs<M, T> {
    fn into_response(self) -> Response {
        let () = Self::JSON;
        json_response(&self.value, M::MEDIA_TYPE)
    }
}

impl<M: MediaType, T: Schema> Responses for JsonAs<M, T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let () = Self::JSON;
        json_responses::<T>(registry, M::MEDIA_TYPE)
    }
}

impl<T> SealedRepresentation for Json<T> {
    const MEDIA_TYPE: &'static str = mime_names::APPLICATION_JSON;
}

impl<T: serde::Serialize + Schema> Representation for Json<T> {}

impl<M: MediaType, T> SealedRepresentation for JsonAs<M, T> {
    const MEDIA_TYPE: &'static str = {
        let () = Self::JSON;
        M::MEDIA_TYPE
    };
}

impl<M: MediaType, T: serde::Serialize + Schema> Representation for JsonAs<M, T> {}
