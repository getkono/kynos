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
/// is a compile error, since the specification says a parameter definition for
/// those is ignored. Use content negotiation for the first two and
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
    /// Two interceptors covering one route and naming the same header here is
    /// a compile error.
    const NAMES: &'static [&'static str];

    /// Whether these headers appear in the emitted description.
    ///
    /// `false` suits the headers every client already handles — `Vary`,
    /// `Content-Encoding`, the CORS set. It does not weaken the
    /// [`NAMES`](HeaderParams::NAMES) conflict check.
    const DESCRIBED: bool = true;

    /// The request field names a response carrying this group depends on.
    ///
    /// Kept out of [`NAMES`](HeaderParams::NAMES) because `Vary` is an
    /// unordered set (RFC 9110 section 12.5.5): two interceptors' contributions
    /// union rather than conflict. Kynos merges these into the response's
    /// `Vary`, case-insensitively, and never describes them.
    ///
    /// ```
    /// use kynos::extract::params::header::{EncodeHeaders, HeaderParams};
    ///
    /// struct Encoding;
    ///
    /// impl HeaderParams for Encoding {
    ///     const NAMES: &'static [&'static str] = &["content-encoding"];
    ///     const VARIES: &'static [&'static str] = &["accept-encoding"];
    /// }
    ///
    /// impl EncodeHeaders for Encoding {
    ///     fn encode(&self) -> Vec<(kynos::http::HeaderName, kynos::http::HeaderValue)> {
    ///         Vec::new()
    ///     }
    /// }
    ///
    /// struct CrossOrigin;
    ///
    /// impl HeaderParams for CrossOrigin {
    ///     const NAMES: &'static [&'static str] = &["access-control-allow-origin"];
    ///     const VARIES: &'static [&'static str] = &["origin"];
    /// }
    ///
    /// impl EncodeHeaders for CrossOrigin {
    ///     fn encode(&self) -> Vec<(kynos::http::HeaderName, kynos::http::HeaderValue)> {
    ///         Vec::new()
    ///     }
    /// }
    ///
    /// // Neither names `vary` in `NAMES`, so an interceptor adding each is not
    /// // a conflict — and both contributions reach the response.
    /// assert_eq!(Encoding::VARIES, ["accept-encoding"]);
    /// assert_eq!(CrossOrigin::VARIES, ["origin"]);
    /// ```
    const VARIES: &'static [&'static str] = &[];

    /// Whether a field this group names may appear more than once on one
    /// response.
    ///
    /// `false` — the default — *inserts*, replacing whatever value was there.
    ///
    /// `true` *appends*, so a group naming `Set-Cookie` twice sends it twice
    /// rather than comma-joining two values RFC 6265 forbids joining.
    ///
    /// Honoured identically by
    /// [`Continued::with_headers`](crate::middleware::Continued::with_headers)
    /// and [`WithHeaders`](crate::response::headers::WithHeaders).
    const REPEATABLE: bool = false;

    /// Describes the declared OpenAPI header parameters.
    ///
    /// The default describes the declared [`NAMES`](HeaderParams::NAMES) with an
    /// unconstrained schema, minus the three the specification says a parameter
    /// definition for shall be ignored. Nothing is marked required.
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
    /// The default rewrites [`parameters`](HeaderParams::parameters) in the
    /// shape a response's `headers` map takes, dropping `Content-Type`, which
    /// the specification says shall be ignored there.
    fn response_headers(registry: &mut Registry) -> Map<RefOr<Header>> {
        Self::parameters(registry)
            .iter()
            .filter(|parameter| !is_ignored_header(&parameter.name))
            .map(|parameter| (parameter.name.clone(), RefOr::Item(header_from(parameter))))
            .collect()
    }
}

/// Reading a header group from a request.
///
/// `#[derive(HeaderParams)]` writes this. An interceptor that only *adds*
/// headers implements [`EncodeHeaders`] alone.
pub trait DecodeHeaders: HeaderParams {
    /// Decodes this group from request headers.
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection>;
}

/// Writing a header group onto a response.
///
/// The counterpart to [`DecodeHeaders`]. A group that is read but never written
/// implements that one alone.
pub trait EncodeHeaders: HeaderParams {
    /// Encodes this group as response header values.
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)>;
}

/// Writes `group` onto `fields`, honouring [`REPEATABLE`](HeaderParams::REPEATABLE)
/// and merging [`VARIES`](HeaderParams::VARIES).
///
/// The one writer for both `Continued::with_headers` and `WithHeaders`, so the
/// two cannot disagree.
pub(crate) fn write<G: EncodeHeaders>(fields: &mut crate::http::HeaderMap, group: &G) {
    for (name, value) in group.encode() {
        // Encoded fields must be a subset of `NAMES`, which the conflict check
        // compares. Debug-only: the response path does not panic.
        debug_assert!(
            G::NAMES
                .iter()
                .any(|declared| crate::middleware::stack::header_name_eq(declared, name.as_str())),
            "`{}` encodes `{}`, which its `NAMES` does not declare",
            std::any::type_name::<G>(),
            name.as_str(),
        );

        if G::REPEATABLE {
            fields.append(name, value);
        } else {
            fields.insert(name, value);
        }
    }

    // `VARIES` names are not in `NAMES`, so they skip the check above.
    crate::middleware::vary_on(fields, G::VARIES);
}

/// The empty group: no headers read, none added, nothing declared.
///
/// What an interceptor names when it reads no header, or adds none.
impl HeaderParams for () {
    const NAMES: &'static [&'static str] = &[];

    fn parameters(registry: &mut Registry) -> Vec<Parameter> {
        let _ = registry;
        Vec::new()
    }

    fn response_headers(registry: &mut Registry) -> Map<RefOr<Header>> {
        let _ = registry;
        Map::new()
    }
}

impl DecodeHeaders for () {
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        let _ = headers;
        Ok(())
    }
}

impl EncodeHeaders for () {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        Vec::new()
    }
}

impl<C: Sync, T: DecodeHeaders + Send> FromRequestParts<C> for Headers<T> {
    type Rejection = HeaderRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(&parts.headers).map(Headers)
    }
}

/// Honours [`DESCRIBED`](HeaderParams::DESCRIBED): an undescribed group
/// contributes nothing.
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
/// A Header Object is a Parameter Object without `name` and `in`. `style` is
/// dropped: `simple` is a header's only style and its default.
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
