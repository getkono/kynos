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
/// `T` derives `PathParams`, and its field names are checked against the route
/// template at compile time — a mismatch is a compile error, not a runtime 500,
/// which is the failure mode every other Rust framework has here.
///
/// # Where the values come from
///
/// The router records what a match captured, and this is the only reader of
/// that record. Each value is percent-decoded before it reaches
/// [`DecodePath::decode`], so a variable holding `%2F` arrives as `/` rather
/// than as the two segments it was encoded to avoid becoming.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path<T>(pub T);

/// Where in the request path a matched route found each of its variables.
///
/// Internal, and the contract between the router and [`Path`]: the router
/// inserts one into [`Parts::extensions`] for every request whose route
/// template has variables, and nothing else reads it.
///
/// Ranges into the request's own path rather than owned strings, because a
/// capture *is* a slice of that path — keeping it one means a match costs one
/// allocation for the vector and none per variable, which is what
/// `docs/nfr.md`'s routing budget is written against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PathCaptures(Vec<(&'static str, Range<usize>)>);

impl PathCaptures {
    /// Records what a match captured out of `path`.
    ///
    /// Each value must be a subslice of `path`, which is exactly what the
    /// router yields: a capture borrows the path it was matched against.
    ///
    /// # Panics
    ///
    /// Panics if a value does not lie inside `path`, which would mean the
    /// captures and the path came from two different requests.
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
    /// `None` rather than a panic when the range does not fit, so that a path
    /// rewritten between matching and extraction produces a rejection rather
    /// than taking the process down.
    pub(crate) fn get<'p>(&self, path: &'p str, name: &str) -> Option<&'p str> {
        self.0
            .iter()
            .find(|(captured, _)| *captured == name)
            .and_then(|(_, range)| path.get(range.clone()))
    }
}

/// Percent-decodes one captured value.
///
/// Delegates to [`__private::uri`](crate::__private::uri), which is the one
/// path the dependency table gives `percent-encoding`; that module renders a
/// typed URI and this is the inverse, so both directions stay in one place.
fn decode_capture(value: &str) -> Result<Cow<'_, str>, Utf8Error> {
    crate::__private::uri::decode_path_value(value)
}

/// A group of path parameters, as the description sees it.
///
/// [`NAMES`](PathParams::NAMES) is what the route attribute compares against
/// the path template.
///
/// # Why the directions are separate traits
///
/// A group is read on the way in and written on the way out, and a given one
/// may only ever do one: a typed URI needs only [`EncodePath`], and an
/// extracted group needs only [`DecodePath`]. Both used to be defaulted methods
/// here with `unimplemented!()` bodies, so a hand-written group that supplied
/// neither satisfied this trait and panicked on its first request.
///
/// Splitting them keeps the one-direction case — that is the whole reason the
/// defaults existed — and moves the failure to where the framework's other
/// bounds put theirs. `AssetHeaders` and `FileHeaders` are the in-tree proof:
/// both are response-side header groups that never decode, and both carried a
/// reachable panic until [`HeaderParams`](super::header::HeaderParams) was
/// split the same way.
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Describes each captured value as an OpenAPI path parameter.
    ///
    /// The default describes the declared [`NAMES`](PathParams::NAMES) with an
    /// unconstrained schema. That is less than a derive emits and never more
    /// than is true: a group that has not said what its values look like has a
    /// description saying only that they exist, which is the honest reading of
    /// a path template that names them.
    ///
    /// `style` is left unstated: `simple` is the default for a path parameter,
    /// so stating it would only repeat what the location already says.
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
/// A group that encodes but does not decode cannot be extracted. That is the
/// whole of what this split buys — it used to compile and panic on the first
/// request:
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
/// Its control: the same group with a decoder, which is what a derive writes.
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
/// The counterpart to [`DecodePath`]; see that trait for why the two are apart.
pub trait EncodePath: PathParams {
    /// Encodes this value for a typed endpoint URI.
    fn encode(&self) -> Vec<(&'static str, String)>;
}

impl<C: Sync, T: DecodePath + Send> FromRequestParts<C> for Path<T> {
    type Rejection = PathRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        let path = parts.uri.path();
        let captures = parts.extensions.get::<PathCaptures>();

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
mod tests {
    use super::{DecodePath, Path, PathCaptures, PathParams, decode_capture};
    use crate::{
        error::rejection::PathRejection,
        extract::FromRequestParts,
        http::{Request, body::Body},
    };

    /// One variable, decoded by hand.
    ///
    /// Hand-written rather than derived: the derived decoder is the macro
    /// crate's to test, and `docs/testing.md` allocates it there. What is under
    /// test here is what reaches `decode` — the capture lookup, the
    /// percent-decoding and the two rejections — not what a derive does with it.
    #[derive(Debug, PartialEq)]
    struct Named(String);

    impl PathParams for Named {
        const NAMES: &'static [&'static str] = &["name"];
    }

    impl DecodePath for Named {
        fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
            Ok(Self(values[0].1.to_owned()))
        }
    }

    /// A group naming a variable the route does not capture.
    #[derive(Debug, PartialEq)]
    struct Absent(String);

    impl PathParams for Absent {
        const NAMES: &'static [&'static str] = &["missing"];
    }

    impl DecodePath for Absent {
        fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
            Ok(Self(values[0].1.to_owned()))
        }
    }

    /// Builds a request whose extensions hold what a match would have captured.
    fn matched(path: &'static str, captures: &[(&'static str, &'static str)]) -> Request {
        let mut request = Request::new(Body::empty());
        *request.uri_mut() = path.parse().expect("a usable path");

        let recorded = PathCaptures::new(
            path,
            captures.iter().map(|(name, value)| {
                // Borrowed out of `path` itself, which is what the router
                // yields and what `PathCaptures::new` asserts.
                let start = path.find(value).expect("a capture inside the path");
                (*name, &path[start..start + value.len()])
            }),
        );
        request.extensions_mut().insert(recorded);

        request
    }

    async fn extract<T: DecodePath + Send>(request: Request) -> Result<T, PathRejection> {
        let (mut parts, _) = request.into_parts();
        Path::<T>::from_request_parts(&mut parts, &())
            .await
            .map(|Path(value)| value)
    }

    #[tokio::test]
    async fn a_captured_value_reaches_the_group_that_declared_it() {
        let decoded: Named = extract(matched("/users/ada", &[("name", "ada")]))
            .await
            .expect("a decodable capture");

        assert_eq!(decoded, Named("ada".to_owned()));
    }

    /// A variable holding `%2F` arrives as `/` rather than as the two segments
    /// it was encoded to avoid becoming.
    #[tokio::test]
    async fn a_percent_encoded_capture_arrives_decoded() {
        let decoded: Named = extract(matched(
            "/reports/annual%2F2026",
            &[("name", "annual%2F2026")],
        ))
        .await
        .expect("a decodable capture");

        assert_eq!(decoded, Named("annual/2026".to_owned()));
    }

    /// A capture the route never made is a rejection naming the variable, not a
    /// panic and not an empty string.
    #[tokio::test]
    async fn a_variable_the_route_did_not_capture_is_rejected_by_name() {
        let rejection = extract::<Absent>(matched("/users/ada", &[("name", "ada")]))
            .await
            .expect_err("a rejection");

        assert!(
            matches!(&rejection, PathRejection::Invalid { name, .. } if name == "missing"),
            "{rejection:?}"
        );
    }

    /// A percent-escape that decodes to bytes no `str` can hold is a rejection
    /// rather than a lossy replacement: a caller told the service one thing and
    /// would otherwise be answered about another.
    #[tokio::test]
    async fn a_capture_that_is_not_utf8_once_decoded_is_rejected() {
        let rejection = extract::<Named>(matched("/users/%FF", &[("name", "%FF")]))
            .await
            .expect_err("a rejection");

        assert!(
            matches!(&rejection, PathRejection::Invalid { detail, .. } if detail.contains("UTF-8")),
            "{rejection:?}"
        );
    }

    /// A group that declares a decoder uses it.
    ///
    /// This used to be the control for two `#[should_panic]` cases asserting
    /// that a group declaring *neither* direction said so at run time. Those
    /// are gone with the defaults that made them possible: `DecodePath` and
    /// `EncodePath` are separate traits now, so a group missing the one it is
    /// used for does not compile, and the guarantee is stated as a
    /// `compile_fail` doctest on `DecodePath` rather than as a panic here.
    #[test]
    fn a_group_that_declares_a_decoder_uses_it() {
        assert_eq!(
            Named::decode(&[("name", "ada")]).expect("a decoded group"),
            Named("ada".to_owned())
        );
    }

    /// A capture with nothing to decode is handed back untouched, which is what
    /// keeps the common case allocation-free.
    #[test]
    fn a_capture_needing_no_decoding_is_not_copied() {
        assert!(matches!(
            decode_capture("plain"),
            Ok(std::borrow::Cow::Borrowed("plain"))
        ));
    }
}
