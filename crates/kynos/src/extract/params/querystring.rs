//! The whole query string as one parameter, OpenAPI 3.2's `in: querystring`.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{
        Parts,
        media::{MediaType, is_json},
    },
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
///
/// `M` names JSON: `application/json`, or an RFC 6839 `+json` type such as
/// `application/vnd.acme.filter+json`, since JSON is the only media type a
/// query string is decoded as. Any other marker fails the build wherever the
/// parameter is extracted or described, rather than compiling into a parameter
/// that refuses every request.
///
/// Reach the decoded query through [`into_inner`](Self::into_inner) or the
/// public field.
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
/// Required of every Parameter Object but unused for this location (OpenAPI
/// 3.2); a constant keeps emitted documents byte-stable.
const QUERYSTRING_NAME: &str = "querystring";

impl<T, M: MediaType> QueryString<T, M> {
    /// Refuses, in the constant evaluation reading it, a marker that does not
    /// name JSON, the only media type a query string is decoded as.
    const JSON: () = assert!(
        is_json(M::MEDIA_TYPE),
        "a `QueryString` marker names neither `application/json` nor a `+json` type, and only \
         JSON has a decoder"
    );
}

/// The whole query string is decoded as the document `M` names.
///
/// `T: DeserializeOwned + Schema`, as for `Json<T>`: the parameter is a
/// document, deserialized whole rather than field by field as a
/// [`QueryParams`](crate::extract::params::query::QueryParams) group is, and
/// held to the bounds `T`'s schema declares. As for a body, a struct is read
/// only from an object, wherever the document holds one.
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
/// A document that decodes and breaks a bound is
/// [`QueryRejection::Schema`], a 400 keyed by JSON Pointer into the decoded
/// document, as a body's is into the body. One that does not decode is
/// [`QueryRejection::Invalid`], whose detail names the pointer of the position
/// that did not fit `T`.
impl<C: Sync, T: serde::de::DeserializeOwned + Schema + Send, M: MediaType + Send>
    FromRequestParts<C> for QueryString<T, M>
{
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let () = Self::JSON;

        let invalid = |detail: String| QueryRejection::Invalid {
            name: QUERYSTRING_NAME.to_owned(),
            detail,
        };

        // No `?` is `null` (see `describe`); a bare `?` is empty, not a document.
        let raw = parts.uri.query().unwrap_or(ABSENT);
        let decoded = crate::__private::uri::decode_path_value(raw).map_err(|error| {
            invalid(format!(
                "the percent-decoded query string is not valid UTF-8: {error}"
            ))
        })?;

        // A struct is read only from an object, as a `Json` body is.
        let value: T = crate::extract::strict_json::from_slice(decoded.as_bytes())
            .map_err(|failure| invalid(undecoded(&failure)))?;

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

/// An undecodable document's detail: where it did not fit `T`, when it was
/// well-formed JSON that did not.
fn undecoded(failure: &crate::extract::strict_json::Failure) -> String {
    let error = &failure.error;
    let at = failure.pointer(Pointer::root());
    match error.classify() {
        serde_json::error::Category::Data if !at.is_empty() => format!("at `{at}`: {error}"),
        _ => error.to_string(),
    }
}

impl<T: Schema, M: MediaType> Describe for QueryString<T, M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let () = Self::JSON;

        let schema = operation.registry().resolve::<T>();
        let required = !admits_null(&schema);
        let parameter = kynos_openapi::Parameter::with_content(
            QUERYSTRING_NAME,
            kynos_openapi::ParameterIn::Querystring,
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(schema),
        );
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
/// True for the `true` schema, a `type` naming `null`, and an `anyOf`/`oneOf`
/// with such a member. Anything else — a `$ref`, or `const`, `enum`, `allOf`,
/// `not` — answers false, erring towards `required`, which is merely
/// over-cautious (e.g. an inline `Option<T>` whose `T` carries an `allOf`).
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
