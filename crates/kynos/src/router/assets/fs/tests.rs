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
/// than on component: `a..b` is an ordinary file.
#[test]
fn a_name_containing_dots_is_not_an_escape() {
    let directory = directory();

    for name in ["a..b.css", "app.min.css", "css/a..b/app.css"] {
        assert!(
            directory.resolve(name).is_some(),
            "`{name}` is a file name, not an escape"
        );
    }
}

/// A segment beginning with a dot is refused wherever it stands.
///
/// What `assets!` never embeds, a directory never serves: `Directory::new(".")`
/// over a checkout would otherwise answer for `.git/config` and `.env`.
#[test]
fn a_hidden_segment_is_refused() {
    let directory = directory();

    for requested in [
        ".env",
        ".git/config",
        "css/.htpasswd",
        ".well-known/security.txt",
        "...css",
        "..hidden.txt",
    ] {
        assert_eq!(
            directory.resolve(requested),
            None,
            "`{requested}` resolved to something"
        );
    }
}

/// A scratch directory of this test's own, removed when it drops.
///
/// Named by process and test, so no two tests ever share one.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("kynos-fs-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    /// Writes `contents` at `relative`, creating its parents.
    fn file(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its parents");
        std::fs::write(path, contents).expect("a file");
    }

    /// Links `relative` to `target`.
    #[cfg(unix)]
    fn link(&self, relative: &str, target: impl AsRef<std::path::Path>) {
        std::os::unix::fs::symlink(target, self.0.join(relative)).expect("a link");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A link below the root is never followed, wherever it points.
///
/// `assets!` skips every link, because following one is how a set escapes its
/// own directory; a directory refuses the same ones, including a link that
/// stays inside, since where a link points is decided by whoever last wrote it.
#[cfg(unix)]
#[tokio::test]
async fn a_link_below_the_root_is_not_followed() {
    let scratch = Scratch::new("a_link_below_the_root_is_not_followed");
    scratch.file("outside/secret.txt", "secret");
    scratch.file("root/real/file.txt", "real");
    scratch.file("root/site/placeholder.txt", "");
    scratch.link("root/escape", scratch.0.join("outside"));
    scratch.link("root/secret.txt", scratch.0.join("outside/secret.txt"));
    scratch.link("root/inside", scratch.0.join("root/real"));
    scratch.link("root/site/index.html", scratch.0.join("outside/secret.txt"));

    let directory = Directory::new(scratch.0.join("root"));

    for requested in [
        "escape/secret.txt",
        "secret.txt",
        "inside/file.txt",
        "site/",
    ] {
        assert!(
            directory.locate(requested).await.is_none(),
            "`{requested}` followed a link"
        );
    }

    // The control: the same file reached without a link is served.
    assert!(directory.locate("real/file.txt").await.is_some());
}

/// The root itself may be a link.
///
/// A deploy that swaps `current` between releases points the root at one, and
/// that link is the operator's rather than the directory's contents.
#[cfg(unix)]
#[tokio::test]
async fn a_root_that_is_a_link_is_followed() {
    let scratch = Scratch::new("a_root_that_is_a_link_is_followed");
    scratch.file("release/app.css", "body {}");
    scratch.link("current", scratch.0.join("release"));

    let directory = Directory::new(scratch.0.join("current"));

    let (path, _) = directory
        .locate("app.css")
        .await
        .expect("a file under a linked root");
    assert_eq!(path, scratch.0.join("current/app.css"));
}

/// A request for `path` under `/static/{*path}`, carrying what the router
/// would have captured: everything after the prefix, still percent-encoded.
fn routed(path: &'static str) -> crate::http::Request {
    let mut request = crate::http::Request::new(crate::http::body::Body::empty());
    *request.uri_mut() = path.parse().expect("a usable path");

    let raw = path
        .strip_prefix("/static/")
        .expect("a path under the prefix");
    let captures = crate::extract::params::path::PathCaptures::new(path, [("path", raw)]);
    let forwarded = crate::http::forwarded::Forwarded::resolve(
        request.headers(),
        None,
        &crate::http::forwarded::TrustedProxies::none(),
    );
    request
        .extensions_mut()
        .insert(crate::router::dispatch::Routed {
            matched: Some(crate::extract::connection::MatchedPath("/static/{*path}")),
            captures: Some(captures),
            forwarded,
            secured: false,
            links: None,
        });

    request
}

/// A capture that names no file is a 404, never the root's index.
///
/// `%FF` decodes to an octet that is not UTF-8, so `unchecked::captured` has
/// no text to give; reading that as an empty path would resolve to the root
/// and serve its index. `%2e%2e` is decoded before resolving, so it arrives
/// as `..` and is refused rather than climbing to the file beside the root.
/// A request the router never matched carries no capture at all.
#[tokio::test]
async fn a_capture_naming_no_file_is_not_found() {
    let scratch = Scratch::new("a_capture_naming_no_file_is_not_found");
    scratch.file("root/index.html", "index");
    scratch.file("root/app.css", "body {}");
    scratch.file("etc/passwd", "secret");

    let directory = Directory::new(scratch.0.join("root"));

    for path in ["/static/%FF", "/static/%2e%2e/etc/passwd"] {
        let response = super::serve(&directory, &routed(path)).await;
        assert_eq!(
            response.status(),
            crate::http::StatusCode::NOT_FOUND,
            "{path}"
        );
    }

    let unrouted = crate::http::Request::new(crate::http::body::Body::empty());
    let response = super::serve(&directory, &unrouted).await;
    assert_eq!(response.status(), crate::http::StatusCode::NOT_FOUND);

    // The control: the root's index and a file under it are served.
    for path in ["/static/", "/static/app.css"] {
        let response = super::serve(&directory, &routed(path)).await;
        assert_eq!(response.status(), crate::http::StatusCode::OK, "{path}");
    }
}
