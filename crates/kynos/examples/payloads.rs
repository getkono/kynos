//! Every request body codec, and the three shapes binary content takes.
//!
//! Run it with the non-default codecs on:
//!
//! ```text
//! cargo run -p kynos --example payloads --features form,multipart
//! ```
//!
//! Four things are worth noticing:
//!
//! * **One codec is one type.** `Json<T>`, `Form<T>`, `MultipartForm<T>`,
//!   `Text` and `Binary<M>` each name their media type, so the operation's
//!   `content` map follows from the signature. There is no codec that decides
//!   at run time what it decoded.
//! * **`OneOf` needs proof that two bodies are distinguishable.** `Alternative`
//!   is implemented per codec pair, never blanket, so `OneOf<Json<A>,
//!   Json<B>>` fails to compile. Two bodies sharing a media type would make
//!   dispatch order observable, and no description can say "whichever the
//!   router tried first".
//! * **Binary takes three shapes, decided by where the bytes sit.** They are
//!   normative in [`docs/schema.md`](../../../docs/schema.md#binary-content),
//!   and all three appear below. None of them is `format: binary`: that is the
//!   OAS 3.0 spelling, and 3.1 replaced it with `contentEncoding` and
//!   `contentMediaType`. Binary itself is fully in scope.
//! * **A media type is a marker type.** `MediaType` has no `dyn` form and no
//!   registry, so a vendor type is a unit struct and one associated constant —
//!   see `Manifest` below.
//!
//! Kynos ships no base64 wrapper, because binary in a *text* format is a
//! property of the field rather than of the body. `Thumbnail` shows the
//! hand-written form, which is a dozen lines and stays honest about what it
//! encodes.

use std::net::Ipv4Addr;

use kynos::{
    extract::{
        body::{OneOf, binary::Binary, form::Form, multipart::MultipartForm, text::Text},
        media::{MediaType, Png},
    },
    openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType},
    prelude::*,
    schema::registry::Registry,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A vendor media type this service defines.
///
/// A marker rather than a string, so a typo is a compile error and the type
/// appears in the operation's `content` key.
struct Manifest;

impl MediaType for Manifest {
    const MEDIA_TYPE: &'static str = "application/vnd.example.manifest+json";
}

/// An image thumbnail carried inside a JSON document.
///
/// The third binary shape: bytes embedded in a text format. `type: string` with
/// `contentEncoding: base64`, which is what a consumer needs to decode it —
/// there is no `format` involved, because 3.1 moved encoding out of `format`
/// entirely.
///
/// `contentMediaType` names what the *decoded* bytes are, which is a different
/// question from what the enclosing body is.
#[derive(Serialize, Deserialize)]
struct Thumbnail(String);

impl kynos::schema::Schema for Thumbnail {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        let mut schema = OpenApiSchema::of_type(SchemaType::String);
        if let OpenApiSchema::Object(object) = &mut schema {
            object.content_encoding = Some("base64".to_owned());
            object.content_media_type = Some("image/png".to_owned());
            // Counted in characters, because this is the encoded form. For raw
            // binary the same keyword counts octets.
            object.max_length = Some(1_398_101);
        }
        schema
    }
}

/// A product, as JSON.
#[derive(Schema, Serialize, Deserialize)]
struct Product {
    id: u64,
    name: String,
    /// Binary in a text format, and the only place `contentEncoding` belongs.
    thumbnail: Option<Thumbnail>,
}

/// The same product as an HTML form submission.
///
/// A separate type rather than the same one, because a form encodes scalars and
/// a nested `thumbnail` object has no faithful form representation. Saying that
/// in the type is better than emitting a schema the encoding cannot honour.
#[derive(Schema, Serialize, Deserialize)]
struct ProductForm {
    id: u64,
    name: String,
}

/// An uploaded product image with its metadata.
///
/// Each field becomes a part with its own `Encoding`. There is no
/// dynamic-part iterator: a handler accepting arbitrary part names cannot
/// describe them, so a variable number of uploads is one `Vec<FilePart>` field.
///
/// No serde derives: a multipart body is decoded part by part rather than
/// through a `Deserializer`, so `MultipartForm` is what says how the parts
/// travel — in both directions, from this one declaration — and `Schema` what
/// puts them in the description.
#[allow(dead_code)]
#[derive(Schema, MultipartForm)]
struct Upload {
    name: String,
    #[schema(max_items = 8)]
    images: Vec<kynos::extract::body::multipart::FilePart>,
}

/// Accepts a product as JSON, or as a form.
///
/// `OneOf` compiles because the two media types are distinct and `Alternative`
/// says so. Both arms appear in the operation's `content` map, and a request
/// carrying neither media type is a documented 415.
#[kynos::post("/products")]
async fn create_product(body: OneOf<Json<Product>, Form<ProductForm>>) -> NoContent {
    // Which arm arrived is a `match`, not a media-type string comparison: the
    // decision was made while decoding, and this is where it lands.
    match body {
        OneOf::Left(Json(product)) => println!("json: {}", product.name),
        OneOf::Right(Form(form)) => println!("form: {}", form.name),
    }

    NoContent
}

/// Accepts a plain-text note.
#[kynos::post("/products/notes")]
async fn create_note(Text(note): Text) -> NoContent {
    println!("note of {} bytes", note.len());
    NoContent
}

/// Accepts an image upload.
#[kynos::post("/products/images")]
async fn upload_images(MultipartForm(upload): MultipartForm<Upload>) -> NoContent {
    println!("upload: {}", upload.name);
    for image in &upload.images {
        println!("image of {} bytes", image.bytes.len());
    }

    NoContent
}

/// Accepts a raw PNG.
///
/// The first binary shape: a raw message body emits **no `type` at all**,
/// because raw binary sits outside JSON Schema's type system. The media type is
/// the `content` key, so `contentMediaType` would only repeat it — which is the
/// second shape, and the reason this schema is empty rather than wrong.
///
/// `Binary` is a named struct rather than a newtype, because `M` is a marker
/// with no value: the handler binds the whole thing and calls `into_inner`.
#[kynos::put("/products/{id}/image")]
async fn replace_image(Path(path): Path<ProductPath>, body: Binary<Png>) -> NoContent {
    println!("{}: {} bytes of png", path.id, body.into_inner().len());
    NoContent
}

/// Accepts a vendor manifest, or a plain-text one.
///
/// `Binary<M>` is generic over the marker, so a new media type costs a unit
/// struct. What this pair cannot be is *two* `Binary`s: both media types would
/// come from a marker, and nothing at the implementation site could tell
/// `Binary<Manifest>` beside `Binary<Pdf>` from `Binary<Pdf>` beside itself.
/// `Text` fixes its media type in its type, which is what makes this provable.
#[kynos::post("/products/manifests")]
async fn upload_manifest(body: OneOf<Binary<Manifest>, Text>) -> NoContent {
    match body {
        OneOf::Left(manifest) => println!("vendor manifest: {} bytes", manifest.into_inner().len()),
        OneOf::Right(Text(manifest)) => println!("plain manifest: {} bytes", manifest.len()),
    }

    NoContent
}

/// What `/products/{id}/image` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct ProductPath {
    id: u64,
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![
        create_product,
        create_note,
        upload_images,
        replace_image,
        upload_manifest,
    ]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
