//! A GET describes its HEAD under a derived name, or beside an explicit
//! `operation_id` under one of its own: the controls for
//! `macros/route_head_refusals.rs`.

use kynos::router::endpoint::meta::EndpointMeta;

#[kynos::get("/files", head)]
async fn get_files() {}

#[kynos::get("/archive", operation_id = "getArchive", head = "headArchive")]
async fn archive() {}

fn main() {
    assert_eq!(get_files::HEAD_OPERATION_ID, Some("head_files"));
    assert_eq!(archive::HEAD_OPERATION_ID, Some("headArchive"));
}
