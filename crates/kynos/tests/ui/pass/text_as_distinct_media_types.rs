//! The control for `traits/text_as_shared_media_type.rs` and
//! `traits/text_as_plain_beside_text.rs`: the same handlers and the same
//! mount, differing only in that each marker is a subtype of its own.

use kynos::{
    extract::body::{OneOf, text::Text, text_as::TextAs},
    http::media::MediaType,
    prelude::*,
};

struct Csv;

impl MediaType for Csv {
    const MEDIA_TYPE: &'static str = "text/csv";
}

struct Markdown;

impl MediaType for Markdown {
    const MEDIA_TYPE: &'static str = "text/markdown";
}

#[kynos::post("/imports")]
async fn import(_rows: OneOf<TextAs<Csv>, TextAs<Markdown>>) {}

#[kynos::post("/notes")]
async fn note(_note: OneOf<Text, TextAs<Markdown>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![import, note]);
}
