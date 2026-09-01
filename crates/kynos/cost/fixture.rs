//! The fixed fixture the two cost sweeps build.
//!
//! Not a teaching example: it is the artifact [`scripts/cost_features.py`]
//! measures, which is why it is declared with a `path` rather than left in
//! `examples/` for `examples/README.md` to catalogue.
//!
//! Feature-blind by construction. It holds no `#[cfg]`, names nothing behind a
//! flag, and reaches for no crate but `kynos`, so the *same* program is built
//! at every feature the sweep visits and the difference between two builds is
//! the feature rather than the program. `docs/performance.md`'s taxonomy asks
//! for exactly that: "They build the same fixture at each feature and compare
//! artifacts."
//!
//! It never serves. `server` is one of the features the sweep excludes -- for
//! `features:targets`' reason, that it does not compile without an HTTP
//! protocol -- so a fixture that bound a socket could not be built at the
//! baseline the deltas are taken against.
//!
//! [`scripts/cost_features.py`]: ../../../scripts/cost_features.py

use kynos::{
    Router,
    middleware::limits::BodySize,
    openapi::{Info, Method, PathTemplate},
    response::status::NoContent,
    router::endpoint::builder::EndpointBuilder,
};

async fn health() -> NoContent {
    NoContent
}

async fn ready() -> NoContent {
    NoContent
}

async fn drain() -> NoContent {
    NoContent
}

fn main() {
    let router = Router::<()>::new()
        .info(Info::new("cost fixture", "0.0.0"))
        .mount(
            EndpointBuilder::new(
                Method::Get,
                PathTemplate::parse("/health").expect("valid path"),
                health,
            )
            .intercept(BodySize::new(1_024)),
        )
        .mount(EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/ready").expect("valid path"),
            ready,
        ))
        .mount(EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/drain").expect("valid path"),
            drain,
        ));

    let service = router.build(()).expect("the fixture describes");
    let document = service.openapi().to_json().expect("the document emits");

    // `black_box` so the linker cannot delete the work: without it the whole
    // router construction is dead and every delta would be a delta of nothing.
    println!("{}", std::hint::black_box(document).len());
}
