//! A `TextAs` and a `JsonAs` under one `text/*` `+json` type cannot be
//! alternatives.
//!
//! A `+json` marker may sit on a `text/*` type, which `TextAs` accepts too, so
//! a `text/vnd.example+json` request would reach both and dispatch order would
//! decide which won. The pair is refused in a `const` `OneOf` evaluates, so the
//! build fails rather than the description.

use kynos::{
    extract::body::{OneOf, json_as::JsonAs, text_as::TextAs},
    http::media::MediaType,
    prelude::*,
};

struct Example;

impl MediaType for Example {
    const MEDIA_TYPE: &'static str = "text/vnd.example+json";
}

#[kynos::post("/examples")]
async fn create(_example: OneOf<TextAs<Example>, JsonAs<Example, u32>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
