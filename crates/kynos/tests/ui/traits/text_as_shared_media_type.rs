//! Two `TextAs` bodies under one type and subtype cannot be alternatives.
//!
//! Their markers differ only in a parameter, which a request need not repeat,
//! so a bare `text/csv` would reach both and dispatch order would decide which
//! won. The pair is refused in a `const` `OneOf` evaluates, so the build fails
//! rather than the description.

use kynos::{
    extract::body::{OneOf, text_as::TextAs},
    http::media::MediaType,
    prelude::*,
};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv";
}

struct HeaderedCsv;

impl MediaType for HeaderedCsv {
    const MEDIA_TYPE: &'static str = "text/csv; header=present";
}

#[kynos::post("/imports")]
async fn import(_rows: OneOf<TextAs<Csv>, TextAs<HeaderedCsv>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import]);
}
