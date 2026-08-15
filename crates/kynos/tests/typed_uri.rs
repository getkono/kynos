//! Compile and runtime checks for route-generated typed URIs.
//!
//! `relative_uri` is named for what it renders: the route attribute's own path
//! template, filled in. A `Group` or `nest` prefix is applied while the router
//! is built and is not visible here.

// The route attribute lives behind `macros`, and the feature powerset check
// runs with `--no-dev-deps`, so without this gate the build breaks in a
// configuration CI never compiles.
#![cfg(feature = "macros")]

use kynos::{
    extract::params::{
        path::{Path, PathParams},
        query::{Query, QueryParams},
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

    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "annual/2026".to_owned())]
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

impl QueryParams for ReportQuery {
    fn encode(&self) -> String {
        "download=true".to_owned()
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
/// the same unexercised state the `todo!()` it replaced was in.
#[test]
fn the_fixture_describes_the_query_it_encodes() {
    let document = kynos::Router::<()>::new()
        .mount(kynos::routes![report])
        .openapi()
        .expect("a describable router");

    let emitted = format!("{document:?}");

    assert!(emitted.contains("download"), "{emitted}");
}
