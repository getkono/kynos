//! The routing group's route and tables: `path-params` with one capture more,
//! and [`router`](super::router) under a policy that changes what a request no
//! operation answers gets.

use kynos::{
    prelude::*,
    router::policy::{FallbackPolicy, TrailingSlashPolicy},
};

use super::{App, Page, built, router};
use crate::driver::Served;

/// `path-params`' captures and one more: past the three a matcher may hold
/// inline.
#[derive(Schema, PathParams)]
struct Four {
    a: u64,
    b: u64,
    c: u64,
    d: u64,
}

/// `path-params`, at four captures.
#[kynos::get("/items/{a}/{b}/{c}/{d}")]
async fn captures_4(Path(item): Path<Four>, Query(page): Query<Page>) -> Json<u64> {
    Json(item.a + item.b + item.c + item.d + page.after.unwrap_or(0) + u64::from(page.per))
}

/// Mounts this group's route.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![captures_4])
}

/// The table answering a trailing-slash variant with 308.
///
/// A policy only a missed path reads, so the matched requests the catalog's
/// rows send cost what they cost under the default.
pub(crate) fn redirecting() -> Box<dyn Served> {
    built(router().trailing_slashes(TrailingSlashPolicy::Redirect))
}

/// The table answering an unmatched path with the status alone.
pub(crate) fn bare_not_found() -> Box<dyn Served> {
    built(router().not_found(FallbackPolicy::Empty))
}
