//! A `TextAs` whose marker declares a charset other than UTF-8 does not
//! compile.
//!
//! Kynos decodes only UTF-8, so a Latin-1 marker would compile into a body
//! that mis-decodes every request it accepts. The check is the same `const` as
//! the `text/*` one, and fails the build instead.

use kynos::{extract::body::text_as::TextAs, http::media::MediaType, prelude::*};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv; charset=iso-8859-1";
}

#[kynos::post("/imports")]
async fn import(_rows: TextAs<Csv>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import]);
}
