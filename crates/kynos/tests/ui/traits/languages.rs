//! A type that names no offered languages cannot be negotiated against. The
//! set has to be written down: it is what the emitted `Content-Language`
//! enumeration is built from, so a type with no `TAGS` leaves the description
//! with nothing to say.

use kynos::response::language::{AcceptLanguage, Languages};

struct Untranslated;

fn offers<L: Languages>() {}

fn main() {
    offers::<Untranslated>();
    let _ = AcceptLanguage::<Untranslated>::parse("en");
}
