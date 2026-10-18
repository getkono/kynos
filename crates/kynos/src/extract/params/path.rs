//! Variables captured from the path template.

use std::{borrow::Cow, ops::Range, str::Utf8Error};

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
/// Read by [`Path`] and [`captured`](crate::unchecked::captured). Ranges into
/// the request path, so a match allocates nothing per variable, and held in
/// place up to [`INLINE`] variables, so below that it allocates nothing at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PathCaptures(Small<(&'static str, Range<usize>)>);

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
            .as_slice()
            .iter()
            .find(|(captured, _)| *captured == name)
            .and_then(|(_, range)| path.get(range.clone()))
    }
}

/// How many variables a match records, and a group decodes, before either
/// spills to the heap.
///
/// One past `matchit`'s own inline capacity of three, so the shortest route
/// that spills here already paid one allocation for its variables there.
const INLINE: usize = 4;

/// A vector whose first [`INLINE`] items are held in place.
///
/// What keeps a match's captures and their decoding off the heap on a common
/// route, where each vector here would otherwise allocate once per request.
#[derive(Clone, Debug)]
enum Small<T> {
    Inline { len: usize, items: [T; INLINE] },
    Spilled(Vec<T>),
}

impl<T: Default> Small<T> {
    /// Room for `capacity` items, in place where they fit.
    fn with_capacity(capacity: usize) -> Self {
        if capacity <= INLINE {
            Self::default()
        } else {
            Self::Spilled(Vec::with_capacity(capacity))
        }
    }

    /// Appends `item`, moving every item to the heap once the inline slots
    /// are full.
    fn push(&mut self, item: T) {
        match self {
            Self::Inline { len, items } if *len < INLINE => {
                items[*len] = item;
                *len += 1;
            }
            Self::Inline { items, .. } => {
                let mut spilled = Vec::with_capacity(INLINE * 2);
                spilled.extend(items.iter_mut().map(std::mem::take));
                spilled.push(item);
                *self = Self::Spilled(spilled);
            }
            Self::Spilled(items) => items.push(item),
        }
    }
}

impl<T> Small<T> {
    fn as_slice(&self) -> &[T] {
        match self {
            Self::Inline { len, items } => &items[..*len],
            Self::Spilled(items) => items,
        }
    }
}

impl<T: Default> Default for Small<T> {
    fn default() -> Self {
        Self::Inline {
            len: 0,
            items: std::array::from_fn(|_| T::default()),
        }
    }
}

impl<T: Default> FromIterator<T> for Small<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let mut small = Self::with_capacity(iter.size_hint().0);
        for item in iter {
            small.push(item);
        }
        small
    }
}

impl<T: PartialEq> PartialEq for Small<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for Small<T> {}

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

        let mut decoded: Small<Cow<'_, str>> = Small::with_capacity(T::NAMES.len());
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
            decoded.push(value);
        }

        let values: Small<(&str, &str)> = T::NAMES
            .iter()
            .zip(decoded.as_slice())
            .map(|(name, value)| (*name, value.as_ref()))
            .collect();
        T::decode(values.as_slice()).map(Path)
    }
}

impl<T: PathParams> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

#[cfg(test)]
mod tests;
