//! Compile and runtime checks for route-generated typed URIs.

use kynos::{
    extract::params::{
        path::{Path, PathParams},
        query::{Query, QueryParams},
    },
    openapi,
    schema::{Schema, registry::Registry},
};

struct ReportPath;

impl PathParams for ReportPath {
    const NAMES: &'static [&'static str] = &["name"];

    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "annual/2026".to_owned())]
    }
}

struct ReportQuery;

impl Schema for ReportQuery {
    fn schema(registry: &mut Registry) -> openapi::Schema {
        let _ = registry;
        todo!()
    }
}

impl QueryParams for ReportQuery {
    fn encode(&self) -> String {
        "download=true".to_owned()
    }
}

#[allow(dead_code, clippy::unused_async)]
#[kynos::get("/reports/{name}")]
async fn report(Path(_): Path<ReportPath>, Query(_): Query<ReportQuery>) {}

#[test]
fn route_attributes_generate_typed_percent_encoded_uris() {
    let uri = report::uri(ReportPath, ReportQuery);
    assert_eq!(uri, "/reports/annual%2F2026?download=true");
}
