//! A route attribute's arguments are a path string and `name = value` pairs,
//! plus the bare `catch_panics`: a route with no path, a path that is not a
//! string, another bare word, a list where a value belongs, and a tag that is
//! not a type's name are each refused.
//!
//! The controls are `pass/route_arguments_once.rs` and
//! `pass/route_tag_is_a_tag.rs`.

#[derive(kynos::Tag)]
struct Users;

#[kynos::operation(method = "GET", operation_id = "getHealth")]
async fn pathless() {}

#[kynos::operation(method = "GET", path = 7)]
async fn numbered() {}

#[kynos::get("/health", nonsense)]
async fn worded() {}

#[kynos::get("/health", tag(Users))]
async fn listed() {}

#[kynos::get("/health", tag = "Users")]
async fn quoted() {}

fn main() {}
