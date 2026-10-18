//! A `QueryString` whose marker does not name JSON does not compile.
//!
//! The check is a `const` on `QueryString`, forced where the parameter is
//! extracted and where it is described, so the build fails rather than a
//! parameter refusing every request with 400. A `+xml` suffix is the readable
//! mistake: a structured document, but not one a query string is decoded as.

use kynos::{extract::params::querystring::QueryString, http::media::MediaType, prelude::*};

struct Filter;

impl MediaType for Filter {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.filter+xml";
}

#[kynos::get("/search")]
async fn search(_filter: QueryString<Option<u32>, Filter>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![search]);
}
