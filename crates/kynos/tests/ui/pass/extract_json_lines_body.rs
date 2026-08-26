use kynos::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};

fn from_request<C, T: kynos::extract::FromRequest<C>>() {}

fn main() {
    from_request::<(), JsonLines<Records<String>>>();
    from_request::<(), JsonSeq<Records<String>>>();
}
