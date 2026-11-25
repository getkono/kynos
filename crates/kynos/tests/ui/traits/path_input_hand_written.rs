//! A group implementing `PathParams` by hand, without the `PathInput` the
//! derive writes beside it, is not extracted from the path; the diagnostic
//! names the impl that binds it.

use kynos::{
    error::rejection::PathRejection,
    extract::params::path::{DecodePath, EncodePath, Path, PathParams},
};

struct Report;

impl PathParams for Report {
    const NAMES: &'static [&'static str] = &["name"];
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
