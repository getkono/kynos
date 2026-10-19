//! A route reads its path variables through the type `Path<T>` names, so a
//! `Path` with no type argument is refused rather than read as describing
//! nothing.
//!
//! The control is `pass/route_with_path_extractor.rs`.

// `Path` is spelled out rather than imported, for the reason
// `route_path_extractor_without_variables.rs` gives.
#[kynos::get("/users/{id}")]
async fn get_user(kynos::extract::params::path::Path(path): kynos::extract::params::path::Path) {
    let _ = path;
}

fn main() {}
