//! README anti-pattern 2: there is no `Body` extractor.

fn from_request<C, T: kynos::extract::FromRequest<C>>() {}

fn main() {
    from_request::<(), kynos::http::body::Body>();
}
