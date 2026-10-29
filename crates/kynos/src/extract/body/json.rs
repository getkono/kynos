//! The `application/json` body codec.

use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
        strict_json,
    },
    http::{Parts, body::Body},
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// An `application/json` request or response body.
///
/// Requires the default-on `json` feature. Requests accept
/// `application/json` with no parameters or with `charset=utf-8`; a missing or
/// different content type rejects with 415. Malformed or incomplete JSON
/// rejects with 400, while valid JSON that cannot deserialize into `T` or
/// breaks a bound `T`'s schema declares rejects with 422, keyed by the JSON
/// Pointer of the position that did not deserialize, or of each member that
/// broke a bound. An array is not a struct, at the top level or nested in it,
/// though `serde` would read one from its members.
/// Which bounds are enforced is [`constraints`](crate::schema::constraints)'
/// to say.
///
/// Extracting one therefore requires `T: Schema` as well as
/// `T: DeserializeOwned`; a type extracted outside a described handler
/// derives [`Schema`] for the bounds it is held to.
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

/// The media type decoded and described.
pub(super) const MEDIA_TYPE: &str = mime_names::APPLICATION_JSON;

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is deserialized.
impl<C: Sync, T: serde::de::DeserializeOwned + Schema + Send> FromRequest<C> for Json<T> {
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(parts, body, MEDIA_TYPE).await?;
        decode(&bytes).map(Self)
    }
}

/// Decodes a body read whole as one JSON document and holds it to the bounds
/// `T` declares: what `Json` and `JsonAs` share once the body is read.
pub(super) fn decode<T: serde::de::DeserializeOwned + Schema>(
    bytes: &[u8],
) -> Result<T, BodyRejection> {
    let value = strict_json::from_slice(bytes).map_err(rejection)?;
    super::checked(value, Pointer::root())
}

/// Malformed JSON is a 400; well-formed JSON that does not fit `T` is a 422,
/// keyed at the JSON Pointer of the position that did not.
// By value to fit `map_err`.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn rejection(failure: strict_json::Failure) -> BodyRejection {
    if failure.is_schema_failure() {
        BodyRejection::schema_at(failure.pointer(Pointer::root()), failure.error.to_string())
    } else {
        BodyRejection::Syntax {
            detail: failure.error.to_string(),
        }
    }
}

impl<T: Schema> Describe for Json<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
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
