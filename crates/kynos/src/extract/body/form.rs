//! The `application/x-www-form-urlencoded` body codec.

use std::collections::BTreeMap;

use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// An `application/x-www-form-urlencoded` request body.
///
/// Requires the `form` feature. A body that is not UTF-8 rejects with 400,
/// and pairs that cannot deserialize into `T` or break a bound `T`'s schema
/// declares reject with 422, as [`Json`](super::json::Json)'s do. Extracting
/// one therefore requires `T: Schema` as well as `T: DeserializeOwned`.
///
/// The body is described with no Encoding Object, so every property takes the
/// default `form` style with `explode`, and both directions follow it:
///
/// - A sequence field, such as `Vec<String>`, is one pair per item under the
///   field's name, so `tag=a&tag=b` is two items and `tag=a` is one. With no
///   pair at all it is missing, as any field is; `#[serde(default)]` reads that
///   as empty.
/// - Any other field is one pair. A second pair under its name does not fit the
///   description and is refused with 422 rather than resolved to either value.
/// - An empty value for an optional number or `bool`, as an empty number input
///   submits, reads as `None`.
///
/// Two shapes are described but not yet decoded, and are refused with 422: a
/// nested struct field, and a member that is not a string reached through
/// `#[serde(flatten)]`, which serde buffers as text and then cannot read as
/// the number or `bool` it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form<T>(pub T);

/// The media type decoded and described.
const MEDIA_TYPE: &str = mime_names::APPLICATION_FORM_URLENCODED;

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is deserialized.
impl<C: Sync, T: serde::de::DeserializeOwned + Schema + Send> FromRequest<C> for Form<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(request, MEDIA_TYPE).await?;

        // `serde_html_form` would replace non-UTF-8 octets, so refuse them
        // first with the 400 `Query<T>` gives, through the same pair reader.
        let text = std::str::from_utf8(&bytes).map_err(|error| BodyRejection::Syntax {
            detail: format!("the form body is not valid UTF-8: {error}"),
        })?;
        for (name, value) in crate::__private::uri::query_pairs(Some(text)) {
            for half in [name, value] {
                std::str::from_utf8(&half).map_err(|error| BodyRejection::Syntax {
                    detail: format!("a percent-decoded form pair is not valid UTF-8: {error}"),
                })?;
            }
        }

        // Form syntax has no malformed input, so any failure is a 422. Keyed
        // at the root: serde names the field only inside its message.
        let value = serde_html_form::from_str(text).map_err(|error| BodyRejection::Schema {
            failures: BTreeMap::from([(String::new(), error.to_string())]),
        })?;
        super::checked(value, Pointer::root()).map(Self)
    }
}

impl<T: Schema> Describe for Form<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for Form<T> {
    fn media_types() -> Vec<&'static str> {
        vec![MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
