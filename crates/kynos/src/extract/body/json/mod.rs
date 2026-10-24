//! The `application/json` body codec.

// Private: what it declares is how `from_slice` reads, which no handler names.
mod strict;

use std::{cell::Cell, collections::BTreeMap};

use kynos_openapi::model::body::mime_names;

use self::strict::Strict;
use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
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
/// Pointer of each member that broke one. An array is not a struct, at the
/// top level or nested in it, though `serde` would read one from its members.
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
        let value = from_slice(&bytes).map_err(rejection)?;
        super::checked(value, Pointer::root()).map(Self)
    }
}

/// Reads one JSON document as [`serde_json::from_slice`] does, except that a
/// struct is read only from an object, wherever the document holds one.
///
/// `serde`'s derive also reads a struct from an array of its members, which
/// the `type: object` its schema declares refuses; where every member
/// defaults, it reads `[]` as `{}`. The refusal comes before any member is
/// read, and reaches every position `serde_json` hands out: a member, a
/// sequence element, a map value, `Option`, a newtype and an enum variant.
/// What `serde` buffers before choosing a type — an untagged or internally
/// tagged enum, every member of a struct that flattens one — is `serde`'s to
/// read. Bytes that are not one JSON document keep their syntax error, so the
/// 400/422 split holds.
pub(super) fn from_slice<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, serde_json::Error> {
    let refused = Cell::new(false);
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = T::deserialize(Strict {
        inner: &mut deserializer,
        refused: &refused,
    })
    .map_err(|error| {
        // A syntax error wins: the refusal is only for well-formed JSON.
        if refused.get() {
            serde_json::from_slice::<serde::de::IgnoredAny>(bytes).err()
        } else {
            None
        }
        .unwrap_or(error)
    })?;
    deserializer.end()?;
    Ok(value)
}

/// Malformed JSON is a 400; well-formed JSON that does not fit `T` is a 422.
///
/// Keyed at the root pointer: serde reports a line and column, not a location.
// By value to fit `map_err`.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn rejection(error: serde_json::Error) -> BodyRejection {
    if is_schema_failure(&error) {
        BodyRejection::Schema {
            failures: BTreeMap::from([(String::new(), error.to_string())]),
        }
    } else {
        BodyRejection::Syntax {
            detail: error.to_string(),
        }
    }
}

/// Where the 400/422 line falls for every JSON codec: serde's `Data` category
/// is a value that does not fit the type; the rest are bytes that are not JSON.
pub(super) fn is_schema_failure(error: &serde_json::Error) -> bool {
    match error.classify() {
        serde_json::error::Category::Data => true,
        serde_json::error::Category::Io
        | serde_json::error::Category::Syntax
        | serde_json::error::Category::Eof => false,
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
