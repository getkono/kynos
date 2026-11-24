//! A transparent enum displays as the error each variant holds, so one held
//! error with no `Display` is reported at its field. A refusal Kynos ships is
//! one; an enum holding it marks its variants `transparent` instead.

use kynos::ApiError;

#[derive(Debug, ApiError)]
#[problem(transparent)]
enum UploadRefused {
    TooLarge(kynos::middleware::limits::body_size::BodySizeExceeded),
}

fn main() {}
