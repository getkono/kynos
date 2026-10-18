//! The control for `traits/querystring_non_json_marker.rs`: the same handler
//! and the same mount, differing only in that the marker's suffix is `+json`.

use kynos::{extract::params::querystring::QueryString, http::media::MediaType, prelude::*};

struct Filter;

impl MediaType for Filter {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.filter+json";
}

#[kynos::get("/search")]
async fn search(_filter: QueryString<Option<u32>, Filter>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![search]);
}
