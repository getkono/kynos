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
/// [`PathParams::decode`], so a variable holding `%2F` arrives as `/` rather
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
    #[allow(dead_code)]
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
    fn get<'p>(&self, path: &'p str, name: &str) -> Option<&'p str> {
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

/// A group of path parameters.
///
/// Derived, never implemented by hand. [`NAMES`](PathParams::NAMES) is what the
/// route attribute compares against the path template.
///
/// The two value-shaped methods have panicking defaults, because a group that
/// has not said how its fields are spelled cannot be decoded or encoded on its
/// behalf. They are defaults rather than requirements so that a group written
/// out by hand for one direction — a typed URI needs only
/// [`encode`](PathParams::encode) — need not write the other.
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Decodes the named captures from a matched route.
    ///
    /// # Panics
    ///
    /// The default panics. Derive `PathParams`, or write this by hand, before
    /// extracting the group.
    fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
        let _ = values;
        unimplemented!(
            "`{}` does not decode path parameters: derive `PathParams` on it, or implement \
             `decode` by hand",
            std::any::type_name::<Self>()
        )
    }

    /// Encodes this value for a typed endpoint URI.
    ///
    /// # Panics
    ///
    /// The default panics, for the reason [`decode`](PathParams::decode)'s
    /// does.
    fn encode(&self) -> Vec<(&'static str, String)> {
        unimplemented!(
            "`{}` does not encode path parameters: derive `PathParams` on it, or implement \
             `encode` by hand",
            std::any::type_name::<Self>()
        )
    }

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

impl<C: Sync, T: PathParams + Send> FromRequestParts<C> for Path<T> {
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
    use super::{Path, PathCaptures, PathParams, decode_capture};
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

        fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
            Ok(Self(values[0].1.to_owned()))
        }
    }

    /// A group naming a variable the route does not capture.
    #[derive(Debug, PartialEq)]
    struct Absent(String);

    impl PathParams for Absent {
        const NAMES: &'static [&'static str] = &["missing"];

        fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
            Ok(Self(values[0].1.to_owned()))
        }
    }

    /// A group that has said nothing about how it is spelled.
    #[derive(Debug)]
    struct Undecodable;

    impl PathParams for Undecodable {
        const NAMES: &'static [&'static str] = &["name"];
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

    async fn extract<T: PathParams + Send>(request: Request) -> Result<T, PathRejection> {
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

    /// The decoding half of the trait panics by default, so a group written for
    /// one direction only says so rather than silently decoding to nothing.
    #[test]
    #[should_panic(expected = "does not decode path parameters")]
    fn a_group_that_declares_no_decoder_says_so() {
        let _ = Undecodable::decode(&[("name", "ada")]);
    }

    /// Its control: a group that *does* declare one is not touched by the
    /// default. Without this the case above would pass against a trait whose
    /// every method panicked.
    #[test]
    fn a_group_that_declares_a_decoder_uses_it() {
        assert_eq!(
            Named::decode(&[("name", "ada")]).expect("a decoded group"),
            Named("ada".to_owned())
        );
    }

    /// The other direction has the same default, and the same control.
    #[test]
    #[should_panic(expected = "does not encode path parameters")]
    fn a_group_that_declares_no_encoder_says_so() {
        let _ = Undecodable.encode();
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
