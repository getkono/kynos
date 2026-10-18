use super::Directory;

/// A resolver over a fixed root.
fn directory() -> Directory {
    Directory::new("/srv/assets")
}

/// A `Cache-Control` that is no field value is refused where the directory is
/// configured, rather than left off every file it serves.
#[test]
#[should_panic(expected = "is not a Cache-Control field value")]
fn a_cache_control_that_is_no_field_value_is_refused_where_it_is_set() {
    let _ = directory().cache_control("max-age=60\n");
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
/// everything. Expectations are relative to the resolved root, since
/// `Directory::new` makes `/srv/assets` absolute against the current drive on
/// Windows.
#[test]
fn a_path_inside_the_root_resolves_under_it() {
    let directory = directory();

    for (requested, expected) in [
        ("app.css", "app.css"),
        ("css/app.css", "css/app.css"),
        ("a/b/c.png", "a/b/c.png"),
        // A `.` segment and an empty one are noise rather than an escape.
        ("./css/./app.css", "css/app.css"),
        ("css//app.css", "css/app.css"),
        ("", ""),
        // A capture that looks absolute is not: an empty leading segment is
        // skipped rather than replacing the base, so it stays inside the root.
        // Worth pinning, because the obvious implementation --
        // `root.join(requested)` -- would have `/etc/passwd` *replace* the
        // root entirely, which is `Path::join`'s documented behaviour and the
        // classic way this goes wrong.
        ("/etc/passwd", "etc/passwd"),
        ("//etc/passwd", "etc/passwd"),
    ] {
        assert_eq!(
            directory.resolve(requested),
            Some(directory.root.join(expected)),
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

/// A file of `length` octets that differ from one offset to the next, so a
/// span read from the wrong place does not pass for the right one.
fn patterned(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|offset| u8::try_from(offset % 251).expect("under 256") ^ seed)
        .collect()
}

/// The file is read only once the body is polled, so a HEAD, whose body is
/// dropped unpolled (RFC 9110 section 9.3.2), reads none of it.
///
/// Witnessed by rewriting the file between the response and its first poll:
/// a body read while the response was being built still holds what was there
/// before.
#[tokio::test]
async fn a_file_is_read_only_once_its_body_is_polled() {
    use http_body_util::BodyExt;

    let scratch = Scratch::new("a_file_is_read_only_once_its_body_is_polled");
    let path = scratch.0.join("root/large.bin");
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("its parents");
    let length = 3 * 64 * 1024 + 1;
    std::fs::write(&path, patterned(length, 0)).expect("a file");

    let directory = Directory::new(scratch.0.join("root"));

    for range in [None, Some("bytes=0-"), Some("bytes=10-70000")] {
        let mut request = routed("/static/large.bin");
        if let Some(range) = range {
            request.headers_mut().insert(
                crate::http::header::RANGE,
                crate::http::HeaderValue::from_static(range),
            );
        }
        std::fs::write(&path, patterned(length, 0)).expect("the original");

        let response = super::serve(&directory, &request).await;
        assert!(response.status().is_success(), "{range:?}");

        let rewritten = patterned(length, 0x5a);
        std::fs::write(&path, &rewritten).expect("the same length, rewritten");

        let expected = match range {
            Some("bytes=10-70000") => &rewritten[10..=70000],
            _ => &rewritten[..],
        };
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a readable body")
            .to_bytes();
        assert!(
            body == expected,
            "{range:?}: the body was read before it was polled"
        );
    }
}

/// A whole file, asked for with or without `bytes=0-`, arrives in spans no
/// larger than [`SPAN`](crate::response::range::source::SPAN) rather than as
/// one buffer the size of the file, and still announces its exact length.
#[tokio::test]
async fn a_whole_file_is_streamed_in_spans() {
    use http_body_util::BodyExt;

    let scratch = Scratch::new("a_whole_file_is_streamed_in_spans");
    let path = scratch.0.join("root/large.bin");
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("its parents");
    let contents = patterned(3 * 64 * 1024 + 1, 0);
    std::fs::write(&path, &contents).expect("a file");

    let directory = Directory::new(scratch.0.join("root"));
    let span = usize::try_from(crate::response::range::source::SPAN).expect("a usize span");

    for range in [None, Some("bytes=0-")] {
        let mut request = routed("/static/large.bin");
        if let Some(range) = range {
            request.headers_mut().insert(
                crate::http::header::RANGE,
                crate::http::HeaderValue::from_static(range),
            );
        }

        let mut body = super::serve(&directory, &request).await.into_body();
        assert_eq!(
            http_body::Body::size_hint(&body).exact(),
            u64::try_from(contents.len()).ok(),
            "{range:?}"
        );

        let mut read = Vec::with_capacity(contents.len());
        while let Some(frame) = body.frame().await {
            let data = frame
                .expect("a readable frame")
                .into_data()
                .expect("a data frame");
            assert!(
                data.len() <= span,
                "{range:?}: a frame of {} octets, past the {span}-octet span",
                data.len()
            );
            read.extend_from_slice(&data);
        }
        assert!(read == contents, "{range:?}: the octets differ");
    }
}

/// An empty file is a 200 with an empty body, not a one-octet read that the
/// file cannot fill.
#[tokio::test]
async fn an_empty_file_is_served_empty() {
    use http_body_util::BodyExt;

    let scratch = Scratch::new("an_empty_file_is_served_empty");
    scratch.file("root/empty.txt", "");

    let directory = Directory::new(scratch.0.join("root"));
    let response = super::serve(&directory, &routed("/static/empty.txt")).await;
    assert_eq!(response.status(), crate::http::StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("an empty body")
        .to_bytes();
    assert!(body.is_empty());
}
