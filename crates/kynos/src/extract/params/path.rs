//! Variables captured from the path template.

use std::{borrow::Cow, str::Utf8Error};

use crate::{
    error::rejection::PathRejection,
    extract::{FromRequestParts, describe::Describe},
    http::Parts,
    router::operation::OperationCx,
    schema::registry::Registry,
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
/// Read by [`Path`] and [`captured`](crate::unchecked::captured). One span into
/// the request path per variable the template declares, in its order, so a
/// match copies no value. A route of up to [`INLINE`] variables on a path of
/// at most `u16::MAX` octets holds its spans in place and allocates nothing;
/// any other allocates once, for all of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathCaptures {
    names: &'static [&'static str],
    spans: Spans,
}

/// Each variable's `[start, end)` octet offsets into the request path, narrow
/// so that a record stays small enough to move without a copy loop.
///
/// A variable the match did not capture holds a start past its end, which no
/// path slices.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Spans {
    Short([[u16; 2]; INLINE]),
    Long(Box<[[u32; 2]]>),
}

impl PathCaptures {
    /// Records what a match captured out of `path` for each of `names`, which
    /// `captured` looks up.
    ///
    /// # Panics
    ///
    /// Panics if a value is not a subslice of `path`.
    pub(crate) fn new<'a>(
        path: &str,
        names: &'static [&'static str],
        mut captured: impl FnMut(&str) -> Option<&'a str>,
    ) -> Self {
        let base = path.as_ptr() as usize;
        let mut span = |name: &str| {
            let value = captured(name)?;
            let start = value.as_ptr() as usize;
            assert!(
                start >= base && start + value.len() <= base + path.len(),
                "a path capture must borrow the path it was matched against"
            );
            let start = start - base;
            Some([start, start + value.len()])
        };

        let spans = if names.len() <= INLINE && u16::try_from(path.len()).is_ok() {
            let mut spans = [[u16::MAX, 0]; INLINE];
            for (slot, name) in spans.iter_mut().zip(names) {
                if let Some(narrowed) = span(name).and_then(narrow) {
                    *slot = narrowed;
                }
            }
            Spans::Short(spans)
        } else {
            // A path past `u32::MAX` octets captures nothing readable.
            Spans::Long(
                names
                    .iter()
                    .map(|name| span(name).and_then(narrow).unwrap_or([u32::MAX, 0]))
                    .collect(),
            )
        };
        Self { names, spans }
    }

    /// The value captured for `name`, borrowed back out of `path`.
    ///
    /// `None` when the span does not fit, as for a path rewritten after
    /// matching.
    pub(crate) fn get<'p>(&self, path: &'p str, name: &str) -> Option<&'p str> {
        let index = self.names.iter().position(|captured| *captured == name)?;
        let [start, end] = match &self.spans {
            Spans::Short(spans) => spans.get(index)?.map(usize::from),
            Spans::Long(spans) => narrow(*spans.get(index)?)?,
        };
        path.get(start..end)
    }
}

/// `span` in another integer width, or `None` where either end does not fit.
fn narrow<F, N: TryFrom<F>>([start, end]: [F; 2]) -> Option<[N; 2]> {
    Some([N::try_from(start).ok()?, N::try_from(end).ok()?])
}

/// How many variables a match records in place, and a group decodes on the
/// stack, before either takes the heap.
///
/// One past `matchit`'s own inline capacity of three, so the shortest route
/// that spills here already paid one allocation for its variables there.
const INLINE: usize = 4;

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

        let names = T::NAMES;
        if names.len() <= INLINE {
            let mut decoded: [Cow<'_, str>; INLINE] = Default::default();
            let mut values = [("", ""); INLINE];
            let values = decode_captures(names, captures, path, &mut decoded, &mut values)?;
            T::decode(values).map(Path)
        } else {
            let mut decoded = vec![Cow::Borrowed(""); names.len()];
            let mut values = vec![("", ""); names.len()];
            let values = decode_captures(names, captures, path, &mut decoded, &mut values)?;
            T::decode(values).map(Path)
        }
    }
}

/// Decodes the value `captures` holds for each of `names` into `decoded`,
/// then pairs each name with its value in `values`, which it returns.
///
/// The caller provides both, so a group of up to [`INLINE`] variables keeps
/// them on its stack.
fn decode_captures<'v, 'p: 'v>(
    names: &'static [&'static str],
    captures: Option<&PathCaptures>,
    path: &'p str,
    decoded: &'v mut [Cow<'p, str>],
    values: &'v mut [(&'v str, &'v str)],
) -> Result<&'v [(&'v str, &'v str)], PathRejection> {
    for (name, slot) in names.iter().zip(decoded.iter_mut()) {
        let raw = captures
            .and_then(|captures| captures.get(path, name))
            .ok_or_else(|| PathRejection::Invalid {
                name: (*name).to_owned(),
                detail: "the matched route captured no value for this variable".to_owned(),
            })?;
        *slot = decode_capture(raw).map_err(|error| PathRejection::Invalid {
            name: (*name).to_owned(),
            detail: format!("the percent-decoded value is not valid UTF-8: {error}"),
        })?;
    }

    let decoded = &*decoded;
    for (pair, (name, value)) in values.iter_mut().zip(names.iter().zip(decoded)) {
        *pair = (name, value.as_ref());
    }
    Ok(&values[..names.len()])
}

impl<T: PathParams> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_parameters(operation, T::parameters);
    }
}

#[cfg(test)]
mod tests;
