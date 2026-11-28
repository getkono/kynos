//! A transparent enum displays as the error each variant holds, and is an
//! `Error` whose source is its, so one held error with no `Display` and no
//! `Error` is reported at its field for each. A refusal Kynos ships is one; an
//! enum holding it marks its variants `transparent` instead.

use kynos::ApiError;

#[derive(Debug, ApiError)]
#[problem(transparent)]
enum UploadRefused {
    TooLarge(kynos::middleware::limits::body_size::BodySizeExceeded),
}

fn main() {}
