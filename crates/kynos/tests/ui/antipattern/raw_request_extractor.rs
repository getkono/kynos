//! README anti-pattern 2: there is no `Request` extractor.

fn from_request<C, T: kynos::extract::FromRequest<C>>() {}

fn main() {
    from_request::<(), kynos::http::Request>();
}
