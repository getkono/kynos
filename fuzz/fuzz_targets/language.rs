//! `Accept-Language` negotiation and the language-tag reader.
//!
//! The input is one `Accept-Language` field. Whatever it holds, the tag
//! chosen is one the offer declares, so it is a member of the published
//! `Content-Language` enumeration. Each of its list elements, without its
//! weight, is also read as a language tag: a tag read is the same tag in any
//! ASCII case, reading it back from its normalized text gives it again, and
//! the `const` well-formedness check gives the reader's answer.

#![no_main]

use kynos::response::language::{AcceptLanguage, offer::Languages, tag::LanguageTag};
use libfuzzer_sys::fuzz_target;

/// An offer spanning each shape of tag: a bare language, a region, a script
/// and a region, a private-use tag, and an irregular grandfathered one.
struct Offered;

impl Languages for Offered {
    const TAGS: &'static [&'static str] = &["en", "fr-CA", "zh-Hant-TW", "x-private", "i-klingon"];
}

fuzz_target!(|data: &[u8]| {
    let Ok(field) = std::str::from_utf8(data) else {
        return;
    };

    let chosen = AcceptLanguage::<Offered>::parse(field).choose();
    assert!(
        Offered::TAGS.contains(&chosen),
        "{field:?} chose {chosen:?}"
    );

    for element in field.split(',') {
        let written = element.split(';').next().unwrap_or_default().trim();
        let read = LanguageTag::parse(written);
        assert_eq!(
            LanguageTag::is_well_formed(written),
            read.is_ok(),
            "{written:?}"
        );
        let Ok(tag) = read else {
            continue;
        };
        assert!(
            tag.as_str().eq_ignore_ascii_case(written),
            "{written:?} read as {tag}"
        );
        assert_eq!(
            LanguageTag::parse(tag.as_str()).as_ref(),
            Ok(&tag),
            "{written:?}"
        );
        assert_eq!(
            LanguageTag::parse(&written.to_ascii_uppercase()).as_ref(),
            Ok(&tag),
            "{written:?}"
        );
    }
});
