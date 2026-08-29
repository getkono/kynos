//! The control for `traits/languages.rs`: the same two calls, differing only in
//! that the offer names its languages.

use kynos::response::language::{AcceptLanguage, Languages};

struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en", "fr"];
}

fn offers<L: Languages>() {}

fn main() {
    offers::<Supported>();
    assert_eq!(AcceptLanguage::<Supported>::parse("fr-CA").choose(), "fr");
}
