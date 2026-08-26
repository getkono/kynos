use kynos::extract::{
    body::{
        alternative::Alternative,
        json::Json,
        json_lines::{JsonLines, JsonSeq, records::Records},
    },
    describe::RequestContent,
};

fn alternatives<Rhs: RequestContent, T: Alternative<Rhs>>() {}

fn main() {
    alternatives::<Json<u64>, JsonLines<Records<String>>>();
    alternatives::<JsonSeq<Records<u64>>, JsonLines<Records<String>>>();
}
