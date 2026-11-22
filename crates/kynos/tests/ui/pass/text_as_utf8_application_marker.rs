//! The control for `traits/text_as_non_text_marker.rs`: the same handler, the
//! same mount and the same `application/csv` type, differing only in that the
//! marker declares `charset=utf-8`, which is what admits a type outside
//! `text/*`.

use kynos::{extract::body::text_as::TextAs, http::media::MediaType, prelude::*};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "application/csv; charset=utf-8";
}

#[kynos::post("/imports")]
async fn import(_rows: TextAs<Csv>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import]);
}
