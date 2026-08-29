//! An offer naming no languages does not compile.
//!
//! The other half of the same `const`: a set with no first entry has no default
//! to serve, and the whole design rests on there always being one — a request
//! whose preferences match nothing is answered rather than refused.

use kynos::response::language::{AcceptLanguage, offer::Languages};

struct Silent;

impl Languages for Silent {
    const TAGS: &'static [&'static str] = &[];
}

fn main() {
    let _ = AcceptLanguage::<Silent>::parse("en").choose();
}
