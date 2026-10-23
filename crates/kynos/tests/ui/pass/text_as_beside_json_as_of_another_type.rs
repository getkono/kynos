//! The control for `traits/text_as_beside_json_as_of_its_type.rs`: the same
//! handler and the same mount, differing only in that the `TextAs` marker
//! drops the `+json` suffix, so it is a subtype of its own.

use kynos::{
    extract::body::{OneOf, json_as::JsonAs, text_as::TextAs},
    http::media::MediaType,
    prelude::*,
};

struct Example;

impl MediaType for Example {
    const MEDIA_TYPE: &'static str = "text/vnd.example";
}

struct ExampleJson;

impl MediaType for ExampleJson {
    const MEDIA_TYPE: &'static str = "text/vnd.example+json";
}

#[kynos::post("/examples")]
async fn create(_example: OneOf<TextAs<Example>, JsonAs<ExampleJson, u32>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
