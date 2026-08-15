//! The `application/json` body codec.

use std::collections::BTreeMap;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

/// An `application/json` request or response body.
///
/// Requires the default-on `json` feature. Requests accept
/// `application/json` with no parameters or with `charset=utf-8`; a missing or
/// different content type rejects with 415. Malformed or incomplete JSON
/// rejects with 400, while valid JSON that cannot deserialize into `T` or
/// violates derived schema constraints rejects with 422.
///
/// ```no_run
/// use kynos::extract::body::json::Json;
///
/// async fn echo(Json(message): Json<String>) -> Json<String> {
///     Json(message)
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Json<T>(pub T);

/// One spelling, read by both halves: what is decoded and what is described.
const MEDIA_TYPE: &str = "application/json";

impl<C: Sync, T: serde::de::DeserializeOwned + Send> FromRequest<C> for Json<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(request, MEDIA_TYPE).await?;
        serde_json::from_slice(&bytes).map(Self).map_err(rejection)
    }
}

/// Malformed JSON is a 400; well-formed JSON that does not fit `T` is a 422.
///
/// serde reports a line and column rather than a location within the document,
/// so a schema failure is attributed to the root JSON Pointer — the empty
/// string — rather than to a pointer invented from a byte offset.
fn rejection(error: serde_json::Error) -> BodyRejection {
    match error.classify() {
        serde_json::error::Category::Data => BodyRejection::Schema {
            failures: BTreeMap::from([(String::new(), error.to_string())]),
        },
        serde_json::error::Category::Io
        | serde_json::error::Category::Syntax
        | serde_json::error::Category::Eof => BodyRejection::Syntax {
            detail: error.to_string(),
        },
    }
}

impl<T: Schema> Describe for Json<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for Json<T> {
    fn media_types() -> Vec<&'static str> {
        vec![MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::json(registry.resolve::<T>())
    }
}
