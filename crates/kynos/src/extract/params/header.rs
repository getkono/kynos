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

    /// The request field names a response carrying this group depends on.
    ///
    /// Separate from [`NAMES`](HeaderParams::NAMES) because `Vary` is the one
    /// response header two interceptors may both contribute to. RFC 9110
    /// section 12.5.5 defines it as an unordered set of field names, so two
    /// contributions *union* where two `Content-Encoding` values would
    /// conflict — and naming it in `NAMES` would make the legitimate pairing of
    /// `Compression` with `Cors` a compile error.
    ///
    /// Kynos merges what is declared here into whatever `Vary` the response
    /// already carries, case-insensitively, and never describes it: a shared
    /// cache reads `Vary`, a client generator has no use for it.
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
    /// That is right for almost every field: a response carrying two
    /// `Content-Encoding` values is one no client can decode.
    ///
    /// `true` *appends*, so a group naming `Set-Cookie` twice sends it twice
    /// rather than comma-joining two values RFC 6265 forbids joining.
    ///
    /// A property of the group rather than a table of field names, because a
    /// per-name allow-list is a table that goes wrong — and the group already
    /// knows whether its own fields comma-join. Read by the one writer both
    /// [`Continued::with_headers`](crate::middleware::Continued::with_headers)
    /// and [`WithHeaders`](crate::response::headers::WithHeaders) go through,
    /// which is what makes "the two cannot disagree" true rather than intended.
    const REPEATABLE: bool = false;

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

/// Reading a header group from a request.
///
/// `#[derive(HeaderParams)]` writes this. An interceptor that only *adds*
/// headers implements [`EncodeHeaders`] alone and never this — `AssetHeaders`
/// and `FileHeaders` are exactly that, and both carried a reachable panic while
/// `decode` was a defaulted method here.
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

/// The empty group: no headers read, none added, nothing declared.
///
/// What an interceptor names when it reads no header, or adds none.
/// Writes `group` onto `fields`, honouring [`REPEATABLE`](HeaderParams::REPEATABLE)
/// and merging [`VARIES`](HeaderParams::VARIES).
///
/// The one writer. Both ways a group reaches the wire —
/// [`Continued::with_headers`](crate::middleware::Continued::with_headers) on an
/// interceptor's response and
/// [`WithHeaders`](crate::response::headers::WithHeaders) on a handler's — go
/// through here, because "the two cannot disagree" is only true when they are
/// one function. They were two, and they did.
pub(crate) fn write<G: EncodeHeaders>(fields: &mut crate::http::HeaderMap, group: &G) {
    for (name, value) in group.encode() {
        // A subset rather than an equality: a group legitimately writes fewer
        // fields than it declares -- `ContentEncoding` with no coding chosen,
        // `CacheHeaders` without a tag, a `Cors` permitting no origin. What is
        // refused is the other direction, a field on the wire that
        // [`NAMES`](HeaderParams::NAMES) never named, because `NAMES` is what
        // the conflict check compares and a field outside it is one no second
        // interceptor can be stopped from adding too.
        //
        // `debug_assert` because the response path does not panic, for the
        // reason `vary_on` gives: every group Kynos ships passes, so this only
        // ever fires on a hand-written one under development, where a debug
        // build is what the author is running.
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

    // Outside the loop, and deliberately not checked above: a `VARIES` name is
    // not in `NAMES`, because `Vary` is the one field two interceptors may both
    // contribute to.
    crate::middleware::vary_on(fields, G::VARIES);
}

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
