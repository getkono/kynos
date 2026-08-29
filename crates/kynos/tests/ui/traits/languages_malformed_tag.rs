//! An offer holding something that is not a language tag does not compile.
//!
//! The check is a `const` on a private trait, forced by `choose`, so the build
//! fails rather than a `Content-Language` no client can read reaching the wire.
//! `en_GB` is the readable mistake: an underscore is the POSIX locale spelling,
//! and RFC 5646 separates subtags with a hyphen.

use kynos::response::language::{AcceptLanguage, Languages};

struct Supported;

impl Languages for Supported {
    const TAGS: &'static [&'static str] = &["en_GB"];
}

fn main() {
    let _ = AcceptLanguage::<Supported>::parse("en").choose();
}
