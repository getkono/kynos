use super::Directory;

/// A resolver over a fixed root.
fn directory() -> Directory {
    Directory::new("/srv/assets")
}

/// Every way out of the root, refused.
///
/// Structural rather than canonicalizing: `..` is never accepted, so it cannot
/// climb; an absolute segment is never accepted, so it cannot replace the base.
/// A check that canonicalized and compared afterwards is one that has to be
/// remembered, and this one has no branch that admits the bad input.
#[test]
fn every_escape_is_refused() {
    let directory = directory();

    let escapes = [
        "..",
        "../etc/passwd",
        "css/../../etc/passwd",
        "a/b/../../../etc/passwd",
        "css/..",
    ];

    for requested in escapes {
        assert_eq!(
            directory.resolve(requested),
            None,
            "`{requested}` resolved to something"
        );
    }
}

/// The control: a path that stays inside resolves under the root.
///
/// Without it, "every escape is refused" would pass for a resolver that refused
/// everything.
#[test]
fn a_path_inside_the_root_resolves_under_it() {
    let directory = directory();

    for (requested, expected) in [
        ("app.css", "/srv/assets/app.css"),
        ("css/app.css", "/srv/assets/css/app.css"),
        ("a/b/c.png", "/srv/assets/a/b/c.png"),
        // A `.` segment and an empty one are noise rather than an escape.
        ("./css/./app.css", "/srv/assets/css/app.css"),
        ("css//app.css", "/srv/assets/css/app.css"),
        ("", "/srv/assets"),
        // A capture that looks absolute is not: an empty leading segment is
        // skipped rather than replacing the base, so it stays inside the root.
        // Worth pinning, because the obvious implementation --
        // `root.join(requested)` -- would have `/etc/passwd` *replace* the
        // root entirely, which is `Path::join`'s documented behaviour and the
        // classic way this goes wrong.
        ("/etc/passwd", "/srv/assets/etc/passwd"),
        ("//etc/passwd", "/srv/assets/etc/passwd"),
    ] {
        assert_eq!(
            directory.resolve(requested).as_deref(),
            Some(std::path::Path::new(expected)),
            "{requested}"
        );
    }
}

/// A name that merely *contains* dots is a name.
///
/// The failure this rules out is a resolver that refused on substring rather
/// than on component: `..hidden` and `a..b` are ordinary files.
#[test]
fn a_name_containing_dots_is_not_an_escape() {
    let directory = directory();

    for name in ["a..b.css", "...css", "..hidden.txt"] {
        assert!(
            directory.resolve(name).is_some(),
            "`{name}` is a file name, not an escape"
        );
    }
}

/// A percent-encoded escape never reaches the resolver as one.
///
/// `unchecked::captured` decodes before this sees it, so `%2e%2e` arrives as
/// `..` and is refused by the same branch. Recorded because the alternative --
/// resolving the *encoded* text -- is the classic traversal bug.
#[test]
fn a_decoded_escape_is_refused_the_same_way() {
    assert_eq!(directory().resolve("../etc/passwd"), None);
}
