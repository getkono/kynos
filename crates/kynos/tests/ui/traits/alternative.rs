//! Two bodies that read the same media type cannot be alternatives.

use kynos::extract::{
    body::{alternative::Alternative, text::Text},
    describe::RequestContent,
};

fn alternatives<Rhs: RequestContent, T: Alternative<Rhs>>() {}

fn main() {
    alternatives::<Text, Text>();
}
