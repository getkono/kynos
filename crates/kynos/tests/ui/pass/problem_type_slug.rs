//! The control for `macros/problem_type_slug_refusals`: the same slugs, each
//! spelled in URI path characters as non-empty segments with no dot segment.

kynos::problem_base!("https://errors.example.com/");

const CHARACTERS: &str = kynos::problem_type!("insufficient%20scope");
const SEGMENTS: &str = kynos::problem_type!("auth/insufficient-scope");
const DOTTED: &str = kynos::problem_type!("auth/.well-known/insufficient-scope");

fn main() {
    assert_eq!(CHARACTERS, "https://errors.example.com/insufficient%20scope");
    assert_eq!(SEGMENTS, "https://errors.example.com/auth/insufficient-scope");
    assert_eq!(DOTTED, "https://errors.example.com/auth/.well-known/insufficient-scope");
}
