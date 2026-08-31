//! Compile and runtime checks for route-generated typed URIs.
//!
//! One reason: `relative_uri` is named for what it renders -- the route
//! attribute's own path template, filled in. A `Group` or `nest` prefix is
//! applied while the router is built and is not visible here.
//!
//! Both parameter types are hand-written, so the fixture is half of what is
//! under test: a `Schema` that describes something other than what `encode`
//! writes is a fixture disagreeing with itself, and one that is never executed
//! cannot disagree with anything. `the_fixture_describes_the_query_it_encodes`
//! is what holds it to that, and is here rather than anywhere else because the
//! fixture is.

// The route attribute lives behind `macros`, and the feature powerset check
// runs with `--no-dev-deps`, so without this gate the build breaks in a
// configuration CI never compiles.
#![cfg(feature = "macros")]

use kynos::{
    extract::params::{
        path::{DecodePath, EncodePath, Path, PathParams},
        query::{DecodeQuery, EncodeQuery, Query, QueryParams},
    },
    openapi::{
        self,
        model::schema::types::{SchemaType, TypeSet},
    },
    schema::{Schema, registry::Registry},
};

struct ReportPath;

impl Schema for ReportPath {
    /// The one variable the template names, described by hand for the reason
    /// [`ReportQuery::schema`] gives.
    fn schema(registry: &mut Registry) -> openapi::Schema {
        object(registry.resolve::<String>(), "name")
    }
}

impl PathParams for ReportPath {
    const NAMES: &'static [&'static str] = &["name"];
}

impl EncodePath for ReportPath {
    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "annual/2026".to_owned())]
    }
}

/// The handler below binds `Path<ReportPath>`, so the group has to decode.
///
/// It did not, and nothing said so: `decode` was a defaulted method with an
/// `unimplemented!()` body, so this fixture was mounted as an extractor while
/// supplying no decoder and would have panicked on the first request to it.
/// The suite never made one. `DecodePath` is what makes that a compile error.
impl DecodePath for ReportPath {
    fn decode(_: &[(&str, &str)]) -> Result<Self, kynos::error::rejection::PathRejection> {
        Ok(Self)
    }
}

struct ReportQuery;

impl Schema for ReportQuery {
    /// The one member `encode` renders, described by hand.
    ///
    /// Hand-written rather than derived: what is under test is the route
    /// attribute's rendering, so both parameter types have to be ones whose
    /// `encode` this file controls — and a `Schema` describing something other
    /// than what `encode` writes is a fixture that disagrees with itself.
    fn schema(registry: &mut Registry) -> openapi::Schema {
        object(registry.resolve::<bool>(), "download")
    }
}

/// A single-member object schema, which is all either fixture needs.
fn object(member: openapi::Schema, name: &str) -> openapi::Schema {
    openapi::Schema::Object(Box::new(openapi::SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        properties: [(name.to_owned(), member)].into_iter().collect(),
        required: Some(vec![name.to_owned()]),
        ..openapi::SchemaObject::default()
    }))
}

impl QueryParams for ReportQuery {}

impl EncodeQuery for ReportQuery {
    fn encode(&self) -> String {
        "download=true".to_owned()
    }
}

/// As `ReportPath`'s: the handler binds `Query<ReportQuery>`, so it decodes.
impl DecodeQuery for ReportQuery {
    fn decode(_: Option<&str>) -> Result<Self, kynos::error::rejection::QueryRejection> {
        Ok(Self)
    }
}

#[allow(clippy::unused_async)]
#[kynos::get("/reports/{name}")]
async fn report(Path(_): Path<ReportPath>, Query(_): Query<ReportQuery>) {}

#[test]
fn route_attributes_generate_typed_percent_encoded_uris() {
    let uri = report::relative_uri(ReportPath, ReportQuery);
    assert_eq!(uri, "/reports/annual%2F2026?download=true");
}

/// The fixture describes, so `ReportQuery::schema` is a body something runs.
///
/// Without this the schema above would be written and never called, which is
/// the same unexercised state the `todo!()` it replaced was in. It asserts
/// about the fixture rather than about `relative_uri`, which is why the module
/// documentation above states it as the second half of this file's reason
/// rather than leaving it looking like a stray document check.
#[test]
fn the_fixture_describes_the_query_it_encodes() {
    let document = kynos::Router::<()>::new()
        .mount(kynos::routes![report])
        .openapi()
        .expect("a describable router");

    let emitted = format!("{document:?}");

    assert!(emitted.contains("download"), "{emitted}");
}
