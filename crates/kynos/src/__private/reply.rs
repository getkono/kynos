//! What `#[derive(Reply)]` writes a variant with.
//!
//! Emitted code cannot name `serde_json`: the crate deriving `Reply` need not
//! depend on it, and Kynos already does. So the two shapes a variant takes are
//! functions here rather than tokens there.

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
/// Serialized in full before anything is written, for the reason
/// [`Json`](crate::extract::body::json::Json) is: a body written as it
/// serializes has already committed the status it would then have to retract.
/// A failure is therefore the documented RFC 9457 500 rather than a truncated
/// success.
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
        HeaderValue::from_static("application/json"),
    );
    response
}
