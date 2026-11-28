//! A route's `head` describes the HEAD its GET answers: a bare `head` beside
//! an explicit `operation_id`, which leaves no name to derive the HEAD's from,
//! a `head` on a method answering no HEAD, and one naming the GET's own
//! `operationId` are each refused.
//!
//! The control is `pass/route_head.rs`.

#[kynos::get("/files", operation_id = "listFiles", head)]
async fn renamed() {}

#[kynos::post("/files", head = "headFiles")]
async fn posted() {}

#[kynos::get("/files", head = "files")]
async fn files() {}

fn main() {}
