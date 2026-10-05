//! The whole query string as one parameter, OpenAPI 3.2's `in: querystring`.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe, media::MediaType},
    http::Parts,
    router::operation::OperationCx,
    schema::Schema,
};

/// The whole query string, described by media type.
///
/// Introduced by OpenAPI 3.2's `in: querystring`. This is the sanctioned way to
/// describe search filters, JSON in the query, or RFC 9535 JSONPath — shapes a
/// list of named parameters cannot express. It must be the only query-related
/// input on its handler.
/// The media type is a marker rather than a field, so this is a named struct
/// and not the newtype every other parameter extractor is: a handler binds the
/// whole value and reaches the decoded query through
/// [`into_inner`](Self::into_inner) or the public field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryString<T, M> {
    /// The decoded query string.
    pub value: T,
    media: std::marker::PhantomData<M>,
}

impl<T, M> QueryString<T, M> {
    /// Wraps a decoded whole-query-string value with its declared media type.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value,
            media: std::marker::PhantomData,
        }
    }

    /// Takes the decoded value out.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

/// The `name` an `in: querystring` parameter carries.
///
/// The field is required of every Parameter Object, and OpenAPI 3.2 states that
/// its value is not used in the serialization of this location — the parameter
/// *is* the whole query string, so there is no key to match. A constant label
/// keeps emitted documents byte-stable, and an `in: querystring` parameter may
/// not share an operation with any `in: query` one, so it can collide with
/// nothing.
const QUERYSTRING_NAME: &str = "querystring";

/// Whether a media type carries a JSON document.
///
/// True for `application/json` and for any type using the `+json` structured
/// syntax suffix of RFC 6839, which is what lets a vendor marker —
/// `application/vnd.acme.filter+json` — be decoded as the JSON it is.
fn is_json(media_type: &str) -> bool {
    let base = media_type
        .split(';')
        .next()
        .unwrap_or(media_type)
        .trim()
        .to_ascii_lowercase();

    base == kynos_openapi::model::body::mime_names::APPLICATION_JSON || base.ends_with("+json")
}

/// The whole query string is decoded as the document `M` names.
///
/// `T: DeserializeOwned` is what every sibling codec asks for — `Json<T>` and
/// `Form<T>` both do — and it is the bound this needs for the same reason: the
/// parameter *is* a document, so decoding it is deserialization rather than the
/// field-by-field walk a
/// [`QueryParams`](crate::extract::params::query::QueryParams) group gets.
///
/// # Rejections
///
/// A media type Kynos has no decoder for is rejected rather than guessed at.
/// Every shape the type's own documentation names — search filters, JSON in the
/// query, RFC 9535 JSONPath — is carried as JSON, so JSON is what is decoded;
/// a marker naming anything else describes a query string this extractor
/// cannot read, and answering 400 says so rather than silently mis-parsing it.
impl<C: Sync, T: serde::de::DeserializeOwned + Send, M: MediaType + Send> FromRequestParts<C>
    for QueryString<T, M>
{
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let invalid = |detail: String| QueryRejection::Invalid {
            name: QUERYSTRING_NAME.to_owned(),
            detail,
        };

        if !is_json(M::MEDIA_TYPE) {
            return Err(invalid(format!(
                "the query string is declared as `{}`, which has no decoder",
                M::MEDIA_TYPE
            )));
        }

        // An absent query string is the empty one. That is not a JSON
        // document, so `serde_json` refuses it for every `T` — one whose
        // fields are all optional, and an `Option`, included.
        let raw = parts.uri.query().unwrap_or_default();
        let decoded = crate::__private::uri::decode_path_value(raw).map_err(|error| {
            invalid(format!(
                "the percent-decoded query string is not valid UTF-8: {error}"
            ))
        })?;

        serde_json::from_str(&decoded)
            .map(Self::new)
            .map_err(|error| invalid(error.to_string()))
    }
}

impl<T: Schema, M: MediaType> Describe for QueryString<T, M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let schema = operation.registry().resolve::<T>();
        operation.add_parameter(kynos_openapi::Parameter::with_content(
            QUERYSTRING_NAME,
            kynos_openapi::ParameterIn::Querystring,
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(schema),
        ));
    }
}

#[cfg(test)]
mod tests;
