use kynos::extract::{
    body::{alternative::Alternative, binary::Binary, text::Text},
    describe::RequestContent,
    media::Pdf,
};

fn alternatives<Rhs: RequestContent, T: Alternative<Rhs>>() {}

fn main() {
    alternatives::<Binary<Pdf>, Text>();
}
