//! Serving files, both ways, and what each costs the description.
//!
//! ```text
//! cargo run -p kynos --example assets --no-default-features \
//!   --features openapi31,macros,server,http1,assets-fs
//! ```
//!
//! Two things share the name "static assets", and only one of them is a
//! catch-all.
//!
//! A **known set of files** — a build output — is finite and enumerable. Every
//! path is a literal, so there is no wildcard, nothing is waived, and each file
//! is an ordinary described operation. `assets!` compiles the set into the
//! binary, and compile-time embedding is what *makes* the set knowable.
//!
//! A **directory as a live namespace** — drop a file in and it serves — matches
//! a set of paths no template describes. `assets_directory` serves one behind
//! `unchecked`, recorded at the document root where no client generator can act
//! on it.
//!
//! Six things are worth noticing:
//!
//! * **The embedded half is fully described.** Run this and read the document:
//!   `/static/index.html` and `/static/css/app.css` are `paths` keys with their
//!   media types, a 200, a 304 and an `ETag`. Nothing about them is opaque.
//! * **The filesystem half has no `paths` key at all.** It appears once, under
//!   `x-kynos-opaque-routes`, with `reason: static-assets`. A generator emits
//!   nothing for it *by construction* — which is stronger than a `paths` entry
//!   marked with a vendor extension a generator may or may not honour.
//! * **The waiver names itself.** `unchecked_reasons` reports
//!   `[StaticAssets]`, so a CI job can assert that this is the only waiver the
//!   service takes and keep catching an accidental `layer_unchecked`.
//!   `has_unchecked` alone would have to be deleted.
//! * **Traversal is unrepresentable rather than defended against.** The
//!   embedded set joins no request input onto anything: the paths are literals
//!   fixed at compile time. The directory resolver examines every component and
//!   accepts only plain names, so `..` never climbs and an absolute segment
//!   never replaces the base.
//! * **Adding a file to the embedded directory does not rebuild.**
//!   `include_bytes!` tracks a file's *contents*, not a directory's membership.
//!   The `build.rs` below is how that is closed, and it is three lines.
//! * **An oversized set warns at the `dir` literal.** Two mebibytes by default.
//!   Raise it with `warn_over = "8MiB"` or turn it off with `"none"` — and note
//!   that under `-D warnings` it becomes an error, which is arguably right.
//!
//! ```text
//! // build.rs
//! fn main() {
//!     println!("cargo::rerun-if-changed=examples/assets");
//! }
//! ```

use std::net::Ipv4Addr;

use kynos::{Router, router::group::Group, server::Server};

kynos::assets! {
    /// The built front end, compiled into this binary.
    ///
    /// A doc comment here reaches the type the macro mints, which is where a
    /// reader of `Site` looks first.
    struct Site;
    dir = "examples/assets",
    // A source map is for a developer with the sources, not for a browser.
    exclude = [".map"],
    warn_over = "4MiB",
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // Described: one operation per file, mounted like anything else. A
        // `Group` already supplies the prefix, so there is no `Router::assets`
        // to learn.
        .group(Group::new("/static").mount(Site::assets().immutable()))
        // Waived: the same directory, served live. Reach for this second.
        .assets_directory(
            "/files",
            kynos::router::assets::fs::Directory::new("examples/assets"),
        );

    println!("{}", router.openapi()?.to_json()?);

    // What a CI job asserts on. Not `!has_unchecked()`, which this service
    // cannot satisfy and which would therefore be deleted.
    assert_eq!(
        router.unchecked_reasons(),
        [kynos::openapi::OpaqueReason::StaticAssets],
        "the only waiver this service takes is the file tree"
    );

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
