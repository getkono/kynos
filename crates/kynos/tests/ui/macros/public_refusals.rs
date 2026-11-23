//! `#[kynos::public]` takes no arguments, goes beside a route attribute, and
//! is written once. Its pass sibling,
//! `ui/pass/public_beside_a_route_attribute.rs`, writes it once, bare, beside
//! a route attribute, above and below.

#[kynos::public(anonymous)]
#[kynos::post("/sessions")]
async fn create_session() {}

#[kynos::public]
async fn not_an_operation() {}

#[kynos::public]
#[kynos::get("/status")]
#[kynos::public]
async fn status() {}

fn main() {}
