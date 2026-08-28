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
