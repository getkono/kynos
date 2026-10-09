//! Variables captured from the path template.

use std::{borrow::Cow, ops::Range, str::Utf8Error};

use crate::{
    error::rejection::PathRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

/// Variables captured from the path template.
///
/// `T` derives `PathParams`, and its wire names, each a field's `rename` or
/// else its name under any `rename_all` as the `Schema` derive applies it,
/// are checked in order against the route template's variables at compile
/// time — a mismatch is a compile error, not a runtime 500.
///
/// # Where the values come from
///
/// Each value the router captured is percent-decoded before it reaches
/// [`DecodePath::decode`], so a variable holding `%2F` arrives as `/`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path<T>(pub T);

/// Where in the request path a matched route found each of its variables.
///
/// Read by [`Path`] and [`captured`](crate::unchecked::captured). Ranges into
/// the request path, so a match allocates nothing per variable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PathCaptures(Vec<(&'static str, Range<usize>)>);

impl PathCaptures {
    /// Records what a match captured out of `path`.
    ///
    /// # Panics
    ///
    /// Panics if a value is not a subslice of `path`.
    pub(crate) fn new<'a>(
        path: &str,
        captures: impl IntoIterator<Item = (&'static str, &'a str)>,
    ) -> Self {
        let base = path.as_ptr() as usize;
        Self(
            captures
                .into_iter()
                .map(|(name, value)| {
                    let start = value.as_ptr() as usize;
                    assert!(
                        start >= base && start + value.len() <= base + path.len(),
                        "a path capture must borrow the path it was matched against"
                    );
                    let start = start - base;
                    (name, start..start + value.len())
                })
                .collect(),
        )
    }

    /// The value captured for `name`, borrowed back out of `path`.
    ///
    /// `None` when the range does not fit, as for a path rewritten after
    /// matching.
    pub(crate) fn get<'p>(&self, path: &'p str, name: &str) -> Option<&'p str> {
        self.0
            .iter()
            .find(|(captured, _)| *captured == name)
            .and_then(|(_, range)| path.get(range.clone()))
    }
}

/// Percent-decodes one captured value, through the one module allowed
/// `percent-encoding`.
fn decode_capture(value: &str) -> Result<Cow<'_, str>, Utf8Error> {
    crate::__private::uri::decode_path_value(value)
}

/// A group of path parameters, as the description sees it.
///
/// [`NAMES`](PathParams::NAMES) is what the route attribute compares against
/// the path template. A typed URI needs only [`EncodePath`]; an extracted
/// group needs only [`DecodePath`].
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Describes each captured value as an OpenAPI path parameter.
    ///
    /// The default describes the declared [`NAMES`](PathParams::NAMES) with an
    /// unconstrained schema, and leaves `style` at its `simple` default.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        Self::NAMES
            .iter()
            .copied()
            .map(|name| kynos_openapi::Parameter::path(name, kynos_openapi::Schema::any()))
            .collect()
    }
}

/// Reading a path parameter group from a matched route.
///
/// `#[derive(PathParams)]` writes this. Implement it by hand only for a group
/// that is extracted; one that only ever appears in a typed URI implements
/// [`EncodePath`] instead.
///
/// A group that encodes but does not decode cannot be extracted:
///
/// ```compile_fail
/// # use kynos::extract::params::path::{EncodePath, Path, PathParams};
/// struct Report;
///
/// impl PathParams for Report {
///     const NAMES: &'static [&'static str] = &["name"];
/// }
///
/// impl EncodePath for Report {
///     fn encode(&self) -> Vec<(&'static str, String)> {
///         vec![("name", "annual".to_owned())]
///     }
/// }
///
/// fn extracted<T: kynos::extract::FromRequestParts<()>>() {}
/// extracted::<Path<Report>>();
/// ```
///
/// The same group with a decoder, which is what a derive writes:
///
/// ```
/// # use kynos::{
/// #     error::rejection::PathRejection,
/// #     extract::params::path::{DecodePath, Path, PathParams},
/// # };
/// struct Report;
///
/// impl PathParams for Report {
///     const NAMES: &'static [&'static str] = &["name"];
/// }
///
/// impl DecodePath for Report {
///     fn decode(_: &[(&str, &str)]) -> Result<Self, PathRejection> {
///         Ok(Self)
///     }
/// }
///
/// fn extracted<T: kynos::extract::FromRequestParts<()>>() {}
/// extracted::<Path<Report>>();
/// ```
pub trait DecodePath: PathParams {
    /// Decodes the named captures from a matched route.
    fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection>;
}

/// Writing a path parameter group into a typed endpoint URI.
///
/// The counterpart to [`DecodePath`].
pub trait EncodePath: PathParams {
    /// Encodes this value for a typed endpoint URI.
    fn encode(&self) -> Vec<(&'static str, String)>;
}

impl<C: Sync, T: DecodePath + Send> FromRequestParts<C> for Path<T> {
    type Rejection = PathRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let path = parts.uri.path();
        let captures = parts
            .extensions
            .get::<crate::router::dispatch::Routed>()
            .and_then(|routed| routed.captures.as_ref());

        let mut decoded: Vec<(&'static str, Cow<'_, str>)> = Vec::with_capacity(T::NAMES.len());
        for name in T::NAMES {
            let raw = captures
                .and_then(|captures| captures.get(path, name))
                .ok_or_else(|| PathRejection::Invalid {
                    name: (*name).to_owned(),
                    detail: "the matched route captured no value for this variable".to_owned(),
                })?;
            let value = decode_capture(raw).map_err(|error| PathRejection::Invalid {
                name: (*name).to_owned(),
                detail: format!("the percent-decoded value is not valid UTF-8: {error}"),
            })?;
            decoded.push((*name, value));
        }

        let values: Vec<(&str, &str)> = decoded
            .iter()
            .map(|(name, value)| (*name, value.as_ref()))
            .collect();
        T::decode(&values).map(Path)
    }
}

impl<T: PathParams + Schema> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

#[cfg(test)]
mod tests;
