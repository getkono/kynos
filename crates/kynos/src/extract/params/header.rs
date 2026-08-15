//! Declared request headers.

use kynos_openapi::{
    Examples, Header, Map, Parameter, ParameterShape, RefOr, Schema,
    model::parameter::header::{is_ignored_header, is_ignored_header_parameter},
};

use crate::{
    error::rejection::HeaderRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, HeaderName, HeaderValue, Parts},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// Declared request headers.
///
/// `T` derives `HeaderParams`. Declaring `Accept`, `Content-Type` or `Authorization`
/// is a compile error: the specification says a parameter definition for those
/// is ignored, so accepting one would put a claim in the description that no
/// consumer will honour. Use content negotiation for the first two and
/// [`Auth`](crate::security::auth::Auth) for the third.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Headers<T>(pub T);

/// A group of declared request or response headers.
///
/// The same derived contract is used by [`Headers`] while extracting and by
/// [`WithHeaders`](crate::response::headers::WithHeaders) while responding.
/// Encoding returns a sequence rather than a map so fields such as `Set-Cookie`
/// can be emitted more than once without comma joining.
pub trait HeaderParams: Sized {
    /// The header names this group declares.
    ///
    /// Read by the compiler as well as by the emitter: two interceptors
    /// covering one route and naming the same header here is a compile error,
    /// which is why it is a `const` rather than something a builder decides.
    const NAMES: &'static [&'static str];

    /// Whether these headers appear in the emitted description.
    ///
    /// Separate from [`NAMES`](HeaderParams::NAMES) because the two answer
    /// different questions. `NAMES` is what the *conflict check* compares, and
    /// every header an interceptor sets belongs there whether or not a
    /// consumer needs to be told about it. This says whether being told is
    /// useful.
    ///
    /// `false` suits the headers HTTP itself defines and every client already
    /// handles — `Vary`, `Content-Encoding`, the CORS set. Setting it does not
    /// weaken the check: a second interceptor touching the same header still
    /// fails to compile.
    const DESCRIBED: bool = true;

    /// Decodes this group from request headers.
    ///
    /// # Panics
    ///
    /// The default panics. Derive `HeaderParams`, or write this by hand, before
    /// extracting the group — an interceptor that only *adds* headers writes
    /// [`encode`](HeaderParams::encode) and needs no decoder, which is why this
    /// is a default rather than a requirement.
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let _ = headers;
        unimplemented!(
            "`{}` does not decode headers: derive `HeaderParams` on it, or implement `decode` by \
             hand",
            std::any::type_name::<Self>()
        )
    }

    /// Encodes this group as response header values.
    ///
    /// # Panics
    ///
    /// The default panics, for the reason [`decode`](HeaderParams::decode)'s
    /// does — mirrored, since a group read but never written needs no encoder.
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        unimplemented!(
            "`{}` does not encode headers: derive `HeaderParams` on it, or implement `encode` by \
             hand",
            std::any::type_name::<Self>()
        )
    }

    /// Describes the declared OpenAPI header parameters.
    ///
    /// The default describes the declared [`NAMES`](HeaderParams::NAMES) with an
    /// unconstrained schema, minus the three the specification says a parameter
    /// definition for shall be ignored: declaring one would put a claim in the
    /// description that no consumer honours, and `NAMES` admits them because the
    /// conflict check still has to see them.
    ///
    /// Nothing is marked required. A group that has not said which of its
    /// headers a request must carry has not said they all are, and claiming so
    /// would make a description stricter than the service.
    fn parameters(registry: &mut Registry) -> Vec<Parameter> {
        let _ = registry;
        Self::NAMES
            .iter()
            .copied()
            .filter(|name| !is_ignored_header_parameter(name))
            .map(|name| Parameter::header(name, Schema::any()))
            .collect()
    }

    /// Describes the headers when this group is attached to a response.
    ///
    /// The default rewrites [`parameters`](HeaderParams::parameters), which is
    /// the same description in the shape a response's `headers` map takes.
    /// `Content-Type` drops out: a response states its media type in `content`,
    /// so the specification says an entry for it here shall be ignored.
    fn response_headers(registry: &mut Registry) -> Map<RefOr<Header>> {
        Self::parameters(registry)
            .iter()
            .filter(|parameter| !is_ignored_header(&parameter.name))
            .map(|parameter| (parameter.name.clone(), RefOr::Item(header_from(parameter))))
            .collect()
    }
}

/// The empty group: no headers read, none added, nothing declared.
///
/// What an interceptor names when it reads no header, or adds none.
impl HeaderParams for () {
    const NAMES: &'static [&'static str] = &[];

    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let _ = headers;
        Ok(())
    }

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        Vec::new()
    }

    fn parameters(registry: &mut Registry) -> Vec<Parameter> {
        let _ = registry;
        Vec::new()
    }

    fn response_headers(registry: &mut Registry) -> Map<RefOr<Header>> {
        let _ = registry;
        Map::new()
    }
}

impl<C: Sync, T: HeaderParams + Send> FromRequestParts<C> for Headers<T> {
    type Rejection = HeaderRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(&parts.headers).map(Headers)
    }
}

/// Honours [`DESCRIBED`](HeaderParams::DESCRIBED): a group that is wire-visible
/// but contract-neutral declares its names so the conflict check sees them, and
/// contributes nothing here.
impl<T: HeaderParams> Describe for Headers<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        if !T::DESCRIBED {
            return;
        }
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

/// Rewrites a parameter as the header of the same value.
///
/// A Header Object is a Parameter Object without `name` and `in`, so the two
/// descriptions are one description written twice, and deriving the second from
/// the first is what keeps them from disagreeing.
///
/// `style` is not carried across: `simple` is the only style a header may take
/// and also the one it takes when none is stated, so omitting it says the same
/// thing.
fn header_from(parameter: &Parameter) -> Header {
    let mut header = match parameter.shape() {
        ParameterShape::Schema { schema, .. } => Header::new(schema.clone()),
        ParameterShape::Content { media_type, value } => {
            Header::with_content(media_type.clone(), (**value).clone())
        }
    };

    header.description.clone_from(&parameter.description);
    header.required = parameter.required;
    header.deprecated = parameter.deprecated;

    match parameter.examples() {
        Some(Examples::Inline(value)) => header = header.with_example(value.clone()),
        Some(Examples::Named(named)) => {
            for (name, example) in named {
                header = match example {
                    RefOr::Item(example) => {
                        header.with_named_example(name.clone(), example.clone())
                    }
                    RefOr::Ref(reference) => {
                        header.with_named_example_ref(name.clone(), reference.clone())
                    }
                };
            }
        }
        None => {}
    }

    header
}
