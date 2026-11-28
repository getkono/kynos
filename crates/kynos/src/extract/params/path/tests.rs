use super::{DecodePath, Path, PathBinding, PathInput, PathParams, captures::tests::recorded};
use crate::{
    error::rejection::PathRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{Request, body::Body},
    router::{
        error::RouterError,
        operation::{OperationCx, OperationIdCase},
    },
    schema::{
        ParamValue, Schema,
        constraints::{Pointer, Violations},
        registry::Registry,
    },
};

/// Binds each hand-written group below by name, as the derive does.
macro_rules! named {
    ($($group:ty),*) => {
        $(impl PathInput for $group {
            type Binding = super::Named<Self>;
        })*
    };
}

named!(Named, Absent, Five);

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

    let names: Vec<&'static str> = captures.iter().map(|(name, _)| *name).collect();
    let recorded = recorded(path, names.leak(), |name| {
        captures
            .iter()
            .find(|(captured, _)| *captured == name)
            .map(|(_, value)| *value)
    });
    let forwarded = crate::http::forwarded::Forwarded::resolve(
        request.headers(),
        None,
        &crate::http::forwarded::TrustedProxies::none(),
    );
    request
        .extensions_mut()
        .insert(crate::router::dispatch::Routed {
            matched: Some(crate::router::dispatch::Matched::intern(
                &crate::extract::connection::MatchedPath(path),
                None,
            )),
            captures: Some(recorded),
            forwarded,
            secured: false,
            links: None,
        });

    request
}

async fn extract<T: PathInput<Binding: PathBinding<T>> + Send>(
    request: Request,
) -> Result<T, PathRejection> {
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
        crate::__private::uri::decode_path_value("plain"),
        Ok(std::borrow::Cow::Borrowed("plain"))
    ));
}

/// A count of at most ten, its bound checked by hand where a derive would
/// check it, so a lone value's constraint check is reached without one.
#[derive(Debug, PartialEq)]
struct Small(u8);

impl Schema for Small {
    fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
        <u8 as Schema>::schema(registry)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        if self.0 > 10 {
            violations.report(at, "must be at most 10");
        }
    }
}

impl std::str::FromStr for Small {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}

impl std::fmt::Display for Small {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ParamValue for Small {}

/// One value reads the operation's own variable, which follows every
/// variable a prefix declared, so a mount prefix's capture is not read in
/// its place.
#[tokio::test]
async fn a_lone_value_reads_the_variable_after_every_prefix_variable() {
    let decoded: u32 = extract(matched("/orgs/7/users/42", &[("org", "7"), ("id", "42")]))
        .await
        .expect("a decodable capture");

    assert_eq!(decoded, 42);
}

/// A lone value is percent-decoded before it is parsed, as a group's field
/// is.
#[tokio::test]
async fn a_lone_value_arrives_percent_decoded() {
    let decoded: String = extract(matched(
        "/reports/annual%2F2026",
        &[("title", "annual%2F2026")],
    ))
    .await
    .expect("a decodable capture");

    assert_eq!(decoded, "annual/2026");
}

/// Text the value does not read is refused naming the template's variable,
/// with the parser's own reason.
#[tokio::test]
async fn a_lone_value_that_does_not_parse_is_rejected_by_its_variable() {
    let rejection = extract::<u32>(matched("/users/ada", &[("id", "ada")]))
        .await
        .expect_err("a rejection");

    assert!(
        matches!(
            &rejection,
            PathRejection::Invalid { name, detail }
                if name == "id" && detail == "invalid digit found in string"
        ),
        "{rejection:?}"
    );
}

/// A value its type's own bounds refuse is a schema rejection at the
/// variable, carrying the type's problem, as a group's field is.
#[tokio::test]
async fn a_lone_value_breaking_its_bound_is_a_schema_rejection() {
    assert_eq!(
        extract::<Small>(matched("/pages/3", &[("page", "3")]))
            .await
            .ok(),
        Some(Small(3))
    );

    let rejection = extract::<Small>(matched("/pages/11", &[("page", "11")]))
        .await
        .expect_err("a rejection");

    assert!(
        matches!(
            &rejection,
            PathRejection::Schema { name, failures, problem: None }
                if name == "page"
                    && failures.values().any(|failure| failure.contains("at most 10"))
        ),
        "{rejection:?}"
    );
}

/// The parameters `Path<T>` describes at an operation declared at `template`,
/// and the refusals it recorded there.
fn described<T: PathInput<Binding: PathBinding<T>>>(
    template: &str,
) -> (
    Vec<kynos_openapi::RefOr<kynos_openapi::Parameter>>,
    Vec<RouterError>,
) {
    let template = kynos_openapi::PathTemplate::parse(template).expect("a valid template");
    let mut registry = Registry::new();
    let mut operation = OperationCx::at(&mut registry, &template, OperationIdCase::AsWritten);
    Path::<T>::describe(&mut operation);
    let parameters = operation.finish().parameters;
    (parameters, registry.take_parameter_refusals())
}

/// One value is described as a required path parameter named after its
/// operation's one variable, with its type's schema.
#[test]
fn a_lone_value_is_described_under_its_operation_variable() {
    let (parameters, refusals) = described::<u32>("/users/{user_id}");

    let expected = kynos_openapi::Parameter::path("user_id", Registry::new().resolve::<u32>());
    assert_eq!(parameters, [kynos_openapi::RefOr::Item(expected)]);
    assert_eq!(refusals, []);
}

/// An operation whose own template declares other than one variable cannot
/// name one value's parameter, which only an operation a builder assembled
/// reaches: the router is told why rather than handed an undescribed
/// variable or a parameter named after nothing.
#[test]
fn a_lone_value_refuses_a_template_without_exactly_one_variable() {
    for (template, variables) in [("/a/{x}/{y}", vec!["x", "y"]), ("/a", vec![])] {
        let (parameters, refusals) = described::<u32>(template);

        assert_eq!(parameters, [], "{template}");
        assert_eq!(
            refusals,
            [RouterError::LonePathValue {
                value: "u32",
                variables: variables.into_iter().map(str::to_owned).collect(),
            }],
            "{template}"
        );
    }
}
