//! A `TextAs` whose marker names no `text/*` type and leaves its charset
//! unstated does not compile.
//!
//! The check is a `const` on `TextAs`, forced wherever the body is extracted,
//! written, described or offered, so the build fails rather than a body
//! decoding as UTF-8 text what its media type never says is. `application/csv`
//! is the readable mistake: a text format, registered under another type, which
//! `charset=utf-8` would make admissible.

use kynos::{extract::body::text_as::TextAs, http::media::MediaType, prelude::*};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "application/csv";
}

#[kynos::post("/imports")]
async fn import(_rows: TextAs<Csv>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import]);
}
