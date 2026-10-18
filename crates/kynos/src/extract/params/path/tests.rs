use super::{DecodePath, INLINE, Path, PathCaptures, PathParams, Small, decode_capture};
use crate::{
    error::rejection::PathRejection,
    extract::{FromRequestParts, describe::Describe},
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

/// Five variables, one past what is held in place, decoded by hand for the
/// reason [`Named`] is.
#[derive(Debug, PartialEq)]
struct Five(Vec<(String, String)>);

impl PathParams for Five {
    const NAMES: &'static [&'static str] = &["a", "b", "c", "d", "e"];
}

impl DecodePath for Five {
    fn decode(values: &[(&str, &str)]) -> Result<Self, PathRejection> {
        Ok(Self(
            values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        ))
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
    let forwarded = crate::http::forwarded::Forwarded::resolve(
        request.headers(),
        None,
        &crate::http::forwarded::TrustedProxies::none(),
    );
    request
        .extensions_mut()
        .insert(crate::router::dispatch::Routed {
            matched: Some(crate::extract::connection::MatchedPath(path)),
            captures: Some(recorded),
            forwarded,
            secured: false,
            links: None,
        });

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

/// Past the captures held in place, every value still reaches the group under
/// its own name and in declaration order: spilling to the heap moves what was
/// already recorded rather than dropping or reordering it.
#[tokio::test]
async fn a_group_past_the_inline_captures_keeps_every_value_in_order() {
    let decoded: Five = extract(matched(
        "/v/1/2/3/4/%35",
        &[("e", "%35"), ("d", "4"), ("c", "3"), ("b", "2"), ("a", "1")],
    ))
    .await
    .expect("a decodable capture");

    assert_eq!(
        decoded,
        Five(
            [("a", "1"), ("b", "2"), ("c", "3"), ("d", "4"), ("e", "5")]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect()
        )
    );
}

/// Each name in `names` paired with the segment after `/v/` it stands at, out
/// of an iterator whose length is not known up front, as the router's
/// `filter_map` is not.
fn segments(
    path: &'static str,
    names: &'static [&'static str],
) -> impl Iterator<Item = (&'static str, &'static str)> {
    names.iter().copied().zip(path.split('/').skip(2))
}

/// A record whose length is not known up front starts in place and spills on
/// the fifth capture: what it held moves to the heap whole and in order, and a
/// match is then read back by name as before.
#[test]
fn captures_of_unknown_count_spill_without_losing_one() {
    let path = "/v/1/2/3/4/5";
    let recorded = PathCaptures::new(path, segments(path, &["a", "b", "c", "d", "e"]));

    assert!(matches!(recorded.0, Small::Spilled(_)), "{recorded:?}");
    for (name, value) in [("a", "1"), ("b", "2"), ("c", "3"), ("d", "4"), ("e", "5")] {
        assert_eq!(recorded.get(path, name), Some(value), "{name}");
    }
}

/// Up to [`INLINE`] captures, the record holds them in place.
#[test]
fn captures_within_the_inline_count_stay_in_place() {
    let path = "/v/1/2/3/4";
    let recorded = PathCaptures::new(path, segments(path, &["a", "b", "c", "d"]));

    assert!(
        matches!(recorded.0, Small::Inline { len: INLINE, .. }),
        "{recorded:?}"
    );
    assert_eq!(recorded.get(path, "d"), Some("4"));
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

/// Witnesses that an extractor describes itself into an operation.
fn assert_describe<T: Describe>() {}

/// A group that is only `PathParams` describes itself: the description reads
/// [`PathParams::parameters`] alone, so `Named`, which implements no `Schema`,
/// is enough. Bound by `Schema` too, such a group compiled as a handler
/// argument and then failed at registration with an error blaming the
/// response.
#[test]
fn a_group_without_a_schema_describes_its_path() {
    assert_describe::<Path<Named>>();
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
