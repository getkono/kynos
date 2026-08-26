use kynos::extract::{
    body::json_lines::{JsonLines, JsonSeq, records::Records},
    describe::RequestContent,
};

#[derive(kynos::Schema, serde::Deserialize)]
struct Reading {
    value: f64,
}

fn is_content<T: RequestContent>() {}

fn main() {
    is_content::<JsonLines<Records<Reading>>>();
    is_content::<JsonSeq<Records<Reading>>>();
}
