fn from_request<C, T: kynos::extract::FromRequest<C>>() {}

fn main() {
    from_request::<(), kynos::extract::body::text::Text>();
}
