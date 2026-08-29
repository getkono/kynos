//! The control for `traits/languages_empty_offer.rs`: the same offer and the
//! same call, differing only in that the offer names a language.

use kynos::response::language::{AcceptLanguage, offer::Languages};

struct Spoken;

impl Languages for Spoken {
    const TAGS: &'static [&'static str] = &["en"];
}

fn main() {
    assert_eq!(AcceptLanguage::<Spoken>::parse("ja").choose(), "en");
}
