//! The whole query string as one parameter, OpenAPI 3.2's `in: querystring`.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{Parts, media::MediaType},
    router::operation::OperationCx,
    schema::{
        Schema,
        constraints::{Pointer, Violations},
        type_admits_null,
    },
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
/// `T: DeserializeOwned + Schema` is what every sibling codec asks for —
/// `Json<T>` and `Form<T>` both do — and it is the bound this needs for the
/// same reason: the parameter *is* a document, so decoding it is
/// deserialization rather than the field-by-field walk a
/// [`QueryParams`](crate::extract::params::query::QueryParams) group gets, and
/// the decoded document is held to the bounds `T`'s schema declares.
///
/// # Absence
///
/// A request with no `?` at all decodes as the JSON document `null`, so an
/// `Option<T>` reads it as `None` and the parameter is described as optional.
/// A `T` whose schema does not admit `null` refuses it with 400, and its
/// parameter is described as `required`. A bare `?` is a query string that is
/// present and empty, which is not a JSON document, and is refused for every
/// `T`.
///
/// # Rejections
///
/// A media type Kynos has no decoder for is rejected rather than guessed at.
/// Every shape the type's own documentation names — search filters, JSON in the
/// query, RFC 9535 JSONPath — is carried as JSON, so JSON is what is decoded;
/// a marker naming anything else describes a query string this extractor
/// cannot read, and answering 400 says so rather than silently mis-parsing it.
///
/// A document that decodes and breaks a bound is
/// [`QueryRejection::Schema`], a 400 keyed by JSON Pointer into the decoded
/// document, as a body's is into the body.
impl<C: Sync, T: serde::de::DeserializeOwned + Schema + Send, M: MediaType + Send>
    FromRequestParts<C> for QueryString<T, M>
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

        // No `?` at all is JSON's `null`, which is how `describe` decides
        // whether to call the parameter required. A bare `?` is a present,
        // empty query string, and is no document.
        let raw = parts.uri.query().unwrap_or(ABSENT);
        let decoded = crate::__private::uri::decode_path_value(raw).map_err(|error| {
            invalid(format!(
                "the percent-decoded query string is not valid UTF-8: {error}"
            ))
        })?;

        let value: T =
            serde_json::from_str(&decoded).map_err(|error| invalid(error.to_string()))?;

        let mut violations = Violations::new();
        value.check_constraints(Pointer::root(), &mut violations);
        if violations.is_empty() {
            Ok(Self::new(value))
        } else {
            Err(QueryRejection::Schema {
                name: QUERYSTRING_NAME.to_owned(),
                failures: violations.into_failures(),
            })
        }
    }
}

impl<T: Schema, M: MediaType> Describe for QueryString<T, M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let schema = operation.registry().resolve::<T>();
        let required = !admits_null(&schema);
        let parameter = kynos_openapi::Parameter::with_content(
            QUERYSTRING_NAME,
            kynos_openapi::ParameterIn::Querystring,
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(schema),
        );
        // `false` is the default, so it is left unstated, as `Query` does.
        operation.add_parameter(if required {
            parameter.required(true)
        } else {
            parameter
        });
    }
}

/// The document an absent query string decodes as.
const ABSENT: &str = "null";

/// Whether `schema` visibly admits the `null` an absent query string reads as.
///
/// True for the `true` schema, a `type` naming `null`, and an `anyOf` or
/// `oneOf` with such a member: the shapes `Option<T>` widens a schema to. A
/// `$ref` is not followed, and a schema carrying a keyword that can exclude
/// `null` whatever its `type` says — `const`, `enum`, `allOf` or `not` — is not
/// evaluated. Anything not recognised answers false, which errs towards
/// `required`: a client told to send a query string the server could have done
/// without is merely over-cautious, while one told it may omit a query string
/// the server refuses fails every time.
///
/// Some `Option<T>` is over-required this way: any `T` described inline as
/// `type: object` beside an `allOf`, which `Option` widens to
/// `type: [object, null]` keeping the `allOf`. That is refused on the `allOf`,
/// so the parameter is `required` although an absent query string decodes as
/// `None`. A generic derived struct is described inline, and gains an `allOf`
/// from a `#[serde(flatten)]` field or a field with a `#[serde(alias)]`.
fn admits_null(schema: &kynos_openapi::Schema) -> bool {
    let Some(object) = schema.as_object() else {
        return matches!(schema, kynos_openapi::Schema::Bool(true));
    };
    if object.reference.is_some()
        || object.const_value.is_some()
        || object.enumeration.is_some()
        || object.all_of.is_some()
        || object.not.is_some()
    {
        return false;
    }

    type_admits_null(schema)
        || [&object.any_of, &object.one_of]
            .into_iter()
            .flatten()
            .any(|members| members.iter().any(admits_null))
}

#[cfg(test)]
mod tests;
