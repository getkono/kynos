//! README anti-pattern 4: a bare `StatusCode` is not a response.

fn responds<T: kynos::response::IntoResponse>() {}

fn main() {
    responds::<kynos::http::StatusCode>();
}
