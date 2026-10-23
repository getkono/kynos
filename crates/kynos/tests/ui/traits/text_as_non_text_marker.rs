//! A `TextAs` whose marker does not name a `text/*` type does not compile.
//!
//! The check is a `const` on `TextAs`, forced wherever the body is extracted,
//! written, described or offered, so the build fails rather than a body
//! decoding as text what its media type says is not. `application/csv` is the
//! readable mistake: a text format, registered under another type.

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
