//! A group implementing `PathParams` by hand is extracted once it binds its
//! names through `PathInput`, as the derive does.

use kynos::{
    error::rejection::PathRejection,
    extract::params::path::{DecodePath, EncodePath, Named, Path, PathInput, PathParams},
};

struct Report;

impl PathParams for Report {
    const NAMES: &'static [&'static str] = &["name"];
}

impl PathInput for Report {
    type Binding = Named<Self>;
}

impl DecodePath for Report {
    fn decode(_: &[(&str, &str)]) -> Result<Self, PathRejection> {
        Ok(Self)
    }
}

impl EncodePath for Report {
    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "annual".to_owned())]
    }
}

#[kynos::get("/reports/{name}")]
async fn report(Path(report): Path<Report>) {
    let _ = report;
}

fn main() {}
