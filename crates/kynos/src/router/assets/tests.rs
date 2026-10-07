use super::media::{self, EXTENSIONS, FALLBACK};
use super::{Asset, AssetSet};

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

/// An index is a whole file name, so a file whose name only ends in it indexes
/// nothing.
///
/// Every place the index name can sit against a segment boundary, closed:
/// alone, after a `/`, after part of a name, and after part of a
/// percent-escape, where the cut would leave a directory that is not a
/// servable path.
#[test]
fn an_index_is_cut_only_at_a_segment_boundary() {
    const CASES: &[(Asset, &str, Option<&str>)] = &[
        (
            Asset::embedded("index.html", b"", "\"t\""),
            "index.html",
            Some(""),
        ),
        (
            Asset::embedded("docs/index.html", b"", "\"t\""),
            "index.html",
            Some("docs/"),
        ),
        (
            Asset::embedded("xindex.html", b"", "\"t\""),
            "index.html",
            None,
        ),
        (
            Asset::embedded("docs/xindex.html", b"", "\"t\""),
            "index.html",
            None,
        ),
        (
            Asset::embedded("a/default.htm", b"", "\"t\""),
            "default.htm",
            Some("a/"),
        ),
        (
            Asset::embedded("a%default.htm", b"", "\"t\""),
            "default.htm",
            None,
        ),
    ];

    for (asset, index, expected) in CASES {
        let set = AssetSet::embedded(std::slice::from_ref(asset)).index(index);
        let directories: Vec<String> = set.indexed().map(|(_, directory)| directory).collect();
        let path = asset.path;

        assert_eq!(
            directories,
            expected.iter().map(|d| (*d).to_owned()).collect::<Vec<_>>(),
            "`{path}` under the index `{index}`"
        );
        assert_eq!(set.len(), 1 + directories.len(), "`{path}` under `{index}`");
    }
}

/// A custom index beside a name that ends in it after a `%` mounts rather than
/// registering the unservable directory `/a%`.
#[test]
fn a_custom_index_beside_a_percent_escape_mounts() {
    const ESCAPED: &[Asset] = &[Asset::embedded("a%default.htm", b"", "\"t\"")];

    let _router =
        crate::Router::<()>::new().mount(AssetSet::embedded(ESCAPED).index("default.htm"));
}
