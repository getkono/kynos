//! README anti-pattern 2: there is no `HeaderMap` extractor.

fn from_request_parts<C, T: kynos::extract::FromRequestParts<C>>() {}

fn main() {
    from_request_parts::<(), kynos::http::HeaderMap>();
}
