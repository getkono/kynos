//! What `#[derive(Reply)]` writes a variant with.
//!
//! Functions rather than emitted tokens, since the deriving crate need not
//! depend on `serde_json`.

use kynos_openapi::model::body::mime_names;

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::IntoResponse,
};

/// The status a variant declared, which the derive checked is one.
fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).expect("`#[derive(Reply)]` rejects a status outside 200..=599")
}

/// A variant carrying no body: the declared status and nothing else.
#[must_use]
pub fn empty(status: u16) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status_code(status);
    response
}

/// A variant carrying a body, as the `application/json` the derive described it
/// as.
///
/// Serialized in full before anything is written, so a failure is the
/// documented RFC 9457 500 rather than a truncated success.
#[must_use]
pub fn json<T: serde::Serialize>(status: u16, body: &T) -> Response {
    let Ok(bytes) = serde_json::to_vec(body) else {
        return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_detail("the response body could not be serialized")
            .into_response();
    };

    let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(bytes)));
    *response.status_mut() = status_code(status);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime_names::APPLICATION_JSON),
    );
    response
}
