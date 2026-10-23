//! A `TextAs` naming `text/plain` cannot be an alternative to `Text`.
//!
//! Both are offered every `text/plain` request, whatever charset the marker
//! states, so dispatch order would decide which won. The pair is refused in a
//! `const` `OneOf` evaluates.

use kynos::{
    extract::body::{OneOf, text::Text, text_as::TextAs},
    http::media::MediaType,
    prelude::*,
};

struct Plain;

impl MediaType for Plain {
    const MEDIA_TYPE: &'static str = "text/plain; charset=utf-8";
}

#[kynos::post("/notes")]
async fn note(_note: OneOf<Text, TextAs<Plain>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![note]);
}
