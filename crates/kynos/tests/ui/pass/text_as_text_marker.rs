//! The control for `traits/text_as_non_text_marker.rs` and
//! `traits/text_as_non_utf8_charset.rs`: the same handler and the same mount,
//! differing only in that the marker is a `text/*` type stating UTF-8.

use kynos::{extract::body::text_as::TextAs, http::media::MediaType, prelude::*};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv; charset=utf-8";
}

#[kynos::post("/imports")]
async fn import(_rows: TextAs<Csv>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import]);
}
