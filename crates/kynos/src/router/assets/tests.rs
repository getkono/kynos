use std::sync::LazyLock;

use super::error::InvalidAsset;
use super::media::{self, EXTENSIONS, FALLBACK};
use super::{Asset, AssetSet, Encoded};

/// The table is closed, and every row is well formed.
///
/// A closed enumeration under `docs/testing.md`: the whole set is here, so a
/// row added wrong fails rather than being sampled around.
#[test]
fn every_row_is_an_extension_and_a_media_type() {
    for (extension, media_type) in EXTENSIONS {
        assert!(
            extension.starts_with('.'),
            "`{extension}` is not an extension"
        );
        assert_eq!(
            *extension,
            extension.to_ascii_lowercase(),
            "`{extension}` is not lower case, so `for_path` could never match it"
        );
        assert!(
            media_type.contains('/'),
            "`{media_type}` is not a media type"
        );
    }
}

/// No extension is listed twice.
///
/// A duplicate would make `for_path`'s answer depend on table order, which is
/// exactly the kind of thing a reader cannot see.
#[test]
fn no_extension_is_named_twice() {
    let mut seen: Vec<&str> = EXTENSIONS.iter().map(|(extension, _)| *extension).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();

    assert_eq!(seen.len(), before, "an extension is listed more than once");
}

/// The table is in the order a maintainer reads it.
#[test]
fn the_table_is_sorted() {
    let listed: Vec<&str> = EXTENSIONS.iter().map(|(extension, _)| *extension).collect();
    let mut sorted = listed.clone();
    sorted.sort_unstable();

    assert_eq!(listed, sorted, "the table is not in extension order");
}

/// Every row resolves, and resolves to itself.
#[test]
fn every_row_resolves_from_a_file_name() {
    for (extension, media_type) in EXTENSIONS {
        assert_eq!(
            media::for_path(&format!("app{extension}")),
            Some(*media_type),
            "{extension}"
        );
    }
}

/// The longest suffix wins.
///
/// `.map` and `.js` both end `app.js.map`, and a source map is JSON rather than
/// JavaScript. Without this the table's order would decide.
#[test]
fn the_longest_matching_extension_wins() {
    assert_eq!(media::for_path("app.js.map"), Some("application/json"));
    assert_eq!(
        media::for_path("app.js"),
        Some("text/javascript; charset=utf-8")
    );
}

/// A file name is not case-sensitive to the table.
#[test]
fn an_extension_resolves_whatever_case_it_is_written_in() {
    for spelling in ["LOGO.PNG", "logo.PnG", "logo.png"] {
        assert_eq!(media::for_path(spelling), Some("image/png"), "{spelling}");
    }
}

/// An extension the table does not name resolves to nothing, and the caller
/// serves the fallback.
#[test]
fn an_unnamed_extension_resolves_to_nothing() {
    assert_eq!(media::for_path("archive.tar.zst"), None);
    assert_eq!(media::for_path("LICENSE"), None);
    assert_eq!(FALLBACK, "application/octet-stream");
}

// --- What a set registers -------------------------------------------------

const INDEX: Asset = Asset::embedded("index.html", b"<!doctype html>", "\"i\"");
const STYLE: Asset = Asset::embedded("css/app.css", b"body{}", "\"s\"");
const NESTED_INDEX: Asset = Asset::embedded("docs/index.html", b"<!doctype html>", "\"d\"");

const SET: &[Asset] = &[INDEX, STYLE, NESTED_INDEX];

/// An asset's media type comes from the table, and falls back honestly.
#[test]
fn an_asset_reports_the_media_type_its_name_implies() {
    assert_eq!(INDEX.media_type(), "text/html; charset=utf-8");
    assert_eq!(STYLE.media_type(), "text/css; charset=utf-8");
    assert_eq!(
        Asset::embedded("LICENSE", b"", "\"l\"").media_type(),
        FALLBACK
    );
}

/// A directory index is served at its own path *and* at the directory's.
///
/// A set that serves `index.html` at `/index.html` and 404s at `/` surprises
/// everyone, and both URLs are real — so both are described.
#[test]
fn an_index_is_registered_at_the_directory_it_indexes() {
    let set = AssetSet::embedded(SET);

    // Three files, and two of them are indexes.
    assert_eq!(set.len(), 5);

    let directories: Vec<String> = set.indexed().map(|(_, directory)| directory).collect();
    assert_eq!(directories, ["", "docs/"]);
}

/// Turning the index off registers one operation per file and no more.
#[test]
fn a_set_without_an_index_registers_one_operation_per_file() {
    assert_eq!(AssetSet::embedded(SET).no_index().len(), 3);
}

/// An index named something else is the one that is indexed.
#[test]
fn the_index_is_whichever_file_the_set_named() {
    let set = AssetSet::embedded(SET).index("app.css");
    let directories: Vec<String> = set.indexed().map(|(_, directory)| directory).collect();

    assert_eq!(directories, ["css/"]);
}

// --- A hand-built asset, checked ------------------------------------------

/// The variant's name, by an exhaustive `match`: a variant added to
/// `InvalidAsset` fails to compile here until it is witnessed below.
fn variant(error: InvalidAsset) -> &'static str {
    match error {
        InvalidAsset::Path { .. } => "Path",
        InvalidAsset::ETag { .. } => "ETag",
        InvalidAsset::Coding { .. } => "Coding",
    }
}

const BR: &[Encoded] = &[Encoded::stored("br", b"\x0b", "\"b\"")];

/// One stored coding, for the coding checks.
fn coded(coding: &'static str, etag: &'static str) -> Result<Asset, InvalidAsset> {
    let encodings: &'static [Encoded] = Box::leak(Box::new([Encoded::stored(coding, b"", etag)]));
    Asset::try_embedded_with_codings("app.css", b"", "\"t\"", encodings)
}

/// Every refusal, each naming the input it refused.
#[test]
#[expect(clippy::too_many_lines)]
fn every_refusal_names_what_it_refused() {
    let cases = [
        (
            Asset::try_embedded("", b"", "\"t\""),
            InvalidAsset::Path { path: "" },
        ),
        (
            Asset::try_embedded("/app.css", b"", "\"t\""),
            InvalidAsset::Path { path: "/app.css" },
        ),
        (
            Asset::try_embedded("css//app.css", b"", "\"t\""),
            InvalidAsset::Path {
                path: "css//app.css",
            },
        ),
        (
            Asset::try_embedded("../app.css", b"", "\"t\""),
            InvalidAsset::Path { path: "../app.css" },
        ),
        (
            Asset::try_embedded("css/./app.css", b"", "\"t\""),
            InvalidAsset::Path {
                path: "css/./app.css",
            },
        ),
        (
            Asset::try_embedded("%2e%2E/app.css", b"", "\"t\""),
            InvalidAsset::Path {
                path: "%2e%2E/app.css",
            },
        ),
        (
            Asset::try_embedded("css/.%2e/app.css", b"", "\"t\""),
            InvalidAsset::Path {
                path: "css/.%2e/app.css",
            },
        ),
        (
            Asset::try_embedded("{id}", b"", "\"t\""),
            InvalidAsset::Path { path: "{id}" },
        ),
        (
            Asset::try_embedded("a}b", b"", "\"t\""),
            InvalidAsset::Path { path: "a}b" },
        ),
        (
            Asset::try_embedded("app css", b"", "\"t\""),
            InvalidAsset::Path { path: "app css" },
        ),
        (
            Asset::try_embedded("a€", b"", "\"t\""),
            InvalidAsset::Path { path: "a€" },
        ),
        (
            Asset::try_embedded("app.css?v=1", b"", "\"t\""),
            InvalidAsset::Path {
                path: "app.css?v=1",
            },
        ),
        (
            Asset::try_embedded("app%zz.css", b"", "\"t\""),
            InvalidAsset::Path { path: "app%zz.css" },
        ),
        (
            Asset::try_embedded("app.css", b"", "t"),
            InvalidAsset::ETag { etag: "t" },
        ),
        (
            Asset::try_embedded("app.css", b"", "W/\"t\""),
            InvalidAsset::ETag { etag: "W/\"t\"" },
        ),
        (
            Asset::try_embedded("app.css", b"", "\""),
            InvalidAsset::ETag { etag: "\"" },
        ),
        (
            Asset::try_embedded("app.css", b"", "\"caf\u{e9}\""),
            InvalidAsset::ETag {
                etag: "\"caf\u{e9}\"",
            },
        ),
        (coded("", "\"b\""), InvalidAsset::Coding { coding: "" }),
        (
            coded("IDENTITY", "\"b\""),
            InvalidAsset::Coding { coding: "IDENTITY" },
        ),
        (
            coded("br", "W/\"b\""),
            InvalidAsset::ETag { etag: "W/\"b\"" },
        ),
    ];

    let mut witnessed = Vec::new();
    for (result, expected) in cases {
        assert_eq!(result, Err(expected));
        witnessed.push(variant(expected));
    }
    witnessed.sort_unstable();
    witnessed.dedup();
    assert_eq!(
        witnessed,
        ["Coding", "ETag", "Path"],
        "a variant has no case"
    );
}

/// Every ASCII octet as a one-octet opaque tag, against RFC 9110 section
/// 8.8.3's `etagc` transcribed by range, narrowed to visible ASCII as the
/// constructor documents. A lone octet above 0x7f is not UTF-8 and cannot be
/// passed at all; the refusal of `obs-text` is the `café` case above.
#[test]
fn an_ascii_opaque_tag_holds_exactly_the_visible_octets_but_dquote() {
    for octet in 0..0x80_u8 {
        let expected = octet == 0x21 || (0x23..=0x7e).contains(&octet);
        let tag: &'static str = Box::leak(format!("\"{}\"", char::from(octet)).into_boxed_str());

        assert_eq!(
            Asset::try_embedded("app.css", b"", tag).is_ok(),
            expected,
            "{octet:#04x}"
        );
    }
}

/// Every ASCII octet as a one-octet coding, against section 5.6.2's `tchar`
/// transcribed as its list.
#[test]
fn a_coding_holds_exactly_the_tchars() {
    const TCHAR: &[u8] = b"!#$%&'*+-.^_`|~0123456789\
        ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    for octet in 0..0x80_u8 {
        let coding: &'static str = Box::leak(char::from(octet).to_string().into_boxed_str());

        assert_eq!(
            coded(coding, "\"b\"").is_ok(),
            TCHAR.contains(&octet),
            "{octet:#04x}"
        );
    }
}

/// What the grammar admits at its edges: an empty opaque tag, a
/// percent-encoded octet, a trailing `/`, and segments made of dots that are
/// not dot segments.
#[test]
fn the_edges_of_each_grammar_are_accepted() {
    assert!(Asset::try_embedded("app.css", b"", "\"\"").is_ok());
    assert!(Asset::try_embedded("caf%C3%A9.css", b"", "\"t\"").is_ok());
    assert!(Asset::try_embedded("docs/", b"", "\"t\"").is_ok());
    assert!(Asset::try_embedded(".well-known/security.txt", b"", "\"t\"").is_ok());
    assert!(Asset::try_embedded("css/...", b"", "\"t\"").is_ok());
    assert!(Asset::try_embedded("css/%2e.css", b"", "\"t\"").is_ok());
}

/// What the checked constructor returns is what the unchecked one would have.
#[test]
fn an_accepted_asset_is_the_unchecked_one() {
    assert_eq!(
        Asset::try_embedded_with_codings("app.css", b"body{}", "\"s\"", BR),
        Ok(Asset::embedded_with_codings(
            "app.css", b"body{}", "\"s\"", BR
        )),
    );
}

/// The property the constructor exists for: an accepted asset mounts.
///
/// Mounting is where the unchecked constructor's bad path panics, so a mount
/// that completes for every accepted shape is the whole claim — through the
/// `LazyLock` the constructor's documentation recommends.
#[test]
fn an_accepted_asset_mounts() {
    static CHECKED: LazyLock<Vec<Asset>> = LazyLock::new(|| {
        [
            Asset::try_embedded("index.html", b"<!doctype html>", "\"i\""),
            Asset::try_embedded("caf%C3%A9.css", b"", "\"c\""),
            Asset::try_embedded("docs/", b"", "\"d\""),
            Asset::try_embedded(".well-known/security.txt", b"", "\"w\""),
            Asset::try_embedded_with_codings("app.js", b"0", "\"j\"", BR),
        ]
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("every asset is accepted")
    });

    let router = crate::Router::<()>::new().mount(AssetSet::embedded(&CHECKED));
    let _ = router;
}
