//! The static group's routes: `plaintext`'s octets as an embedded asset, and
//! the description the router serves of itself.

use kynos::{prelude::*, router::docs::Docs};

use super::App;

kynos::assets! {
    /// One file holding `plaintext`'s thirteen octets, so that an asset row
    /// and `plaintext` write the same body.
    struct Fixtures;
    dir = "src/app/fixtures",
}

/// Mounts the asset set under `/static`, and the description at
/// `/openapi.json`.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router
        .group(Group::new("/static").mount(Fixtures::assets()))
        .docs(Docs::description_only())
}
