//! The compression sweep's table: one JSON document at five sizes, behind
//! `Compression`.

use kynos::{middleware::compression::Compression, prelude::*};

use super::{Large, entries, json_large, json_small};
use crate::driver::Served;

/// The compression sweep's middle sizes: about 1, 2 and 4 KiB of the
/// `json-large` shape. `json-small` and `json-large` are its two ends.
#[kynos::get("/json/1k")]
async fn json_1k() -> Json<Large> {
    Json(entries(5))
}

#[kynos::get("/json/2k")]
async fn json_2k() -> Json<Large> {
    Json(entries(10))
}

#[kynos::get("/json/4k")]
async fn json_4k() -> Json<Large> {
    Json(entries(21))
}

/// The JSON documents at every size the compression sweep reads, behind
/// `Compression`.
///
/// A table of its own rather than the catalog's with more routes, so that the
/// sweep's counts are of these five routes and move with nothing the catalog
/// mounts.
///
/// `min_size(0)` rather than the default, because the sweep measures what
/// encoding a body costs at each length, and the default's threshold is what
/// that measurement is read to choose: measured through it, the rows below the
/// threshold would report the skip instead.
#[must_use]
pub(crate) fn compressed() -> Box<dyn Served> {
    Box::new(
        Router::<()>::new()
            .mount(kynos::routes![
                json_small, json_1k, json_2k, json_4k, json_large
            ])
            .intercept(Compression::new().min_size(0))
            .build(())
            .expect("a describable router"),
    )
}
