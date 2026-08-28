//! Query string parameters, named and whole.

use crate::{
    error::rejection::QueryRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

#[cfg(feature = "openapi32")]
use crate::extract::media::MediaType;

/// Named query string parameters.
///
/// `T` derives `QueryParams`. Nested objects are rejected at compile time:
/// `deepObject` is defined only for objects whose properties are scalars, so a
/// deeper shape has no legal serialization. Under `openapi32`, reach for
/// [`QueryString`] instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Query<T>(pub T);

/// A group of query parameters, as the description sees it.
///
/// The two directions are [`DecodeQuery`] and [`EncodeQuery`], for the reason
/// [`PathParams`](crate::extract::params::path::PathParams)' are: a group is
/// often only ever read or only ever written, and expressing that as a
/// defaulted method with an `unimplemented!()` body made a group that supplied
/// neither satisfy this trait and panic on its first request.
pub trait QueryParams: Sized + Schema {
    /// Describes the individual OpenAPI query parameters.
    ///
    /// The default projects the group's own schema: one parameter per property,
    /// carrying that property's schema, required exactly when the object says
    /// it is. That is the whole of what a group of named query parameters is,
    /// which is why it needs no separate name list the way the other locations
    /// do.
    ///
    /// `style` is left unstated: `form` with `explode` is the default for a
    /// query parameter, so stating it would only repeat the location.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        // `Self::schema` rather than `registry.resolve::<Self>()`: the group is
        // not a component of the description, and a `$ref` has no properties to
        // split into parameters. The property schemas underneath still went
        // through the registry, which is where naming belongs.
        match Self::schema(registry) {
            kynos_openapi::Schema::Object(object) => {
                let required = object.required.unwrap_or_default();
                object
                    .properties
                    .into_iter()
                    .map(|(name, schema)| {
                        let mandatory = required.contains(&name);
                        let parameter = kynos_openapi::Parameter::query(name, schema);
                        if mandatory {
                            parameter.required(true)
                        } else {
                            parameter
                        }
                    })
                    .collect()
            }
            // A group whose schema constrains nothing names no parameters
            // either; there is nothing to enumerate.
            kynos_openapi::Schema::Bool(_) => Vec::new(),
        }
    }
}

/// Reading a query parameter group from a request.
///
/// `#[derive(QueryParams)]` writes this.
pub trait DecodeQuery: QueryParams {
    /// Decodes a raw query string.
    ///
    /// `None` when the request carried no `?` at all, which is distinct from
    /// the empty query string a bare `?` produces.
    fn decode(query: Option<&str>) -> Result<Self, QueryRejection>;
}

/// Writing a query parameter group into a typed endpoint URI.
///
/// The counterpart to [`DecodeQuery`]; see [`QueryParams`] for why the two are
/// apart.
pub trait EncodeQuery: QueryParams {
    /// Encodes this value as a query string without the leading `?`.
    fn encode(&self) -> String;
}

impl<C: Sync, T: DecodeQuery + Send> FromRequestParts<C> for Query<T> {
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(parts.uri.query()).map(Query)
    }
}

impl<T: QueryParams> Describe for Query<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

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
#[cfg(feature = "openapi32")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryString<T, M> {
    /// The decoded query string.
    pub value: T,
    media: std::marker::PhantomData<M>,
}

#[cfg(feature = "openapi32")]
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
#[cfg(feature = "openapi32")]
const QUERYSTRING_NAME: &str = "querystring";

/// Whether a media type carries a JSON document.
///
/// True for `application/json` and for any type using the `+json` structured
/// syntax suffix of RFC 6839, which is what lets a vendor marker —
/// `application/vnd.acme.filter+json` — be decoded as the JSON it is.
#[cfg(feature = "openapi32")]
fn is_json(media_type: &str) -> bool {
    let base = media_type
        .split(';')
        .next()
        .unwrap_or(media_type)
        .trim()
        .to_ascii_lowercase();

    base == "application/json" || base.ends_with("+json")
}

/// The whole query string is decoded as the document `M` names.
///
/// `T: DeserializeOwned` is what every sibling codec asks for — `Json<T>` and
/// `Form<T>` both do — and it is the bound this needs for the same reason: the
/// parameter *is* a document, so decoding it is deserialization rather than the
/// field-by-field walk a [`QueryParams`] group gets.
///
/// # Rejections
///
/// A media type Kynos has no decoder for is rejected rather than guessed at.
/// Every shape the type's own documentation names — search filters, JSON in the
/// query, RFC 9535 JSONPath — is carried as JSON, so JSON is what is decoded;
/// a marker naming anything else describes a query string this extractor
/// cannot read, and answering 400 says so rather than silently mis-parsing it.
#[cfg(feature = "openapi32")]
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

        // An absent query string is the empty one, so a `T` with no required
        // field still decodes; `serde_json` rejects the empty document itself
        // for every other `T`.
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

#[cfg(feature = "openapi32")]
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
mod tests {
    use super::{DecodeQuery, QueryParams};
    use crate::{
        error::rejection::QueryRejection,
        schema::{Schema, registry::Registry},
    };

    // Two `#[should_panic]` cases stood here, asserting that a group declaring
    // neither direction said so at run time. `DecodeQuery` and `EncodeQuery`
    // are separate traits now, so such a group does not compile and there is
    // nothing left to provoke. The control below stays: it is what proves the
    // decoder is reached, which is the half a compile-time guarantee cannot
    // say anything about.

    /// The control: a group that declares a decoder is not touched by the
    /// default, and sees the distinction the signature draws — `None` for a
    /// request with no `?` at all, `Some("")` for a bare one.
    #[test]
    fn a_group_that_declares_a_decoder_sees_the_query_it_was_given() {
        #[derive(Debug, PartialEq)]
        struct Recorded(Option<String>);

        impl Schema for Recorded {
            fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
                let _ = registry;
                kynos_openapi::Schema::any()
            }
        }

        impl QueryParams for Recorded {}

        impl DecodeQuery for Recorded {
            fn decode(query: Option<&str>) -> Result<Self, QueryRejection> {
                Ok(Self(query.map(str::to_owned)))
            }
        }

        assert_eq!(Recorded::decode(None).expect("decoded"), Recorded(None));
        assert_eq!(
            Recorded::decode(Some("")).expect("decoded"),
            Recorded(Some(String::new()))
        );
        assert_eq!(
            Recorded::decode(Some("a=1")).expect("decoded"),
            Recorded(Some("a=1".to_owned()))
        );
    }

    /// A structured syntax suffix is JSON, which is what lets a vendor media
    /// type be decoded as the JSON it is.
    #[cfg(feature = "openapi32")]
    #[test]
    fn a_json_suffixed_media_type_is_read_as_json() {
        use super::is_json;

        assert!(is_json("application/json"));
        assert!(is_json("application/json; charset=utf-8"));
        assert!(is_json("APPLICATION/JSON"));
        assert!(is_json("application/vnd.acme.filter+json"));

        assert!(!is_json("application/xml"));
        assert!(!is_json("text/plain"));
        // A suffix is a suffix of the base type, not of the parameters.
        assert!(!is_json("application/xml; note=+json"));
    }
}
