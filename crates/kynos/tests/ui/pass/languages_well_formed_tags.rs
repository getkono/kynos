//! The control for `traits/languages_malformed_tag.rs`: the same offer and the
//! same call, differing only in that the tag is spelled the way RFC 5646 spells
//! one.

use kynos::response::language::{AcceptLanguage, Languages};

struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en-GB"];
}

fn main() {
    assert_eq!(AcceptLanguage::<Supported>::parse("en").choose(), "en-GB");
}
