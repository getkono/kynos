//! `problem_type!` joins its slug onto the crate's base by the rule
//! `#[problem(slug = "...")]` is held to, so each slug the derive refuses is
//! refused here at compile time, saying which rule it broke: a character no
//! URI path admits, a leading, trailing or doubled `/`, and a dot segment.
//!
//! The control is `pass/problem_type_slug.rs`.

kynos::problem_base!("https://errors.example.com/");

const CHARACTERS: &str = kynos::problem_type!("insufficient scope");
const LEADING_SLASH: &str = kynos::problem_type!("/insufficient-scope");
const TRAILING_SLASH: &str = kynos::problem_type!("insufficient-scope/");
const EMPTY_SEGMENT: &str = kynos::problem_type!("auth//insufficient-scope");
const DOT_SEGMENT: &str = kynos::problem_type!("auth/%2e./insufficient-scope");

fn main() {
    let _ = (CHARACTERS, LEADING_SLASH, TRAILING_SLASH, EMPTY_SEGMENT, DOT_SEGMENT);
}
