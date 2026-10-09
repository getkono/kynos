//! The pages this module ships, and the two values substituted into them.
//!
//! # Two tokens, two contexts
//!
//! `{{description_url}}` is substituted as a *JSON string literal, quotes
//! included*, and belongs where a script expects a string expression.
//! `{{title}}` is substituted as HTML text and belongs in element content.
//!
//! The rule is per token rather than per page, because no single escaping is
//! correct in both places: `&` must become `&amp;` in markup and must stay a
//! bare `&` inside a script, and a page that got either backwards would be
//! wrong about the URL it fetches. Neither is a hypothetical. A path template
//! admits every RFC 3986 `pchar`, which includes the sub-delimiter `'` -- so
//! `description_at("/it's")` is a legal path that closes a single-quoted
//! JavaScript string, and a title is arbitrary developer prose.

/// Where the page fetches the description. Substituted as a JSON string,
/// quotes included.
pub(super) const DESCRIPTION_URL: &str = "{{description_url}}";

/// The document's title. Substituted as HTML text.
pub(super) const TITLE: &str = "{{title}}";

/// A page Kynos ships, and the policy it is served under.
///
/// # Why the policy is a constant
///
/// The bundle is pinned to one exact version and carries a Subresource
/// Integrity hash, so a compromised or breaking upstream publish is refused by
/// the browser rather than run on the API's own origin. The response's
/// `Content-Security-Policy` then allows that one bundle and the one inline
/// script that boots it, named by its hash, and nothing else.
///
/// A hash only names a script that never changes, so the shipped boot scripts
/// hold no token. The description URL is substituted into a JSON data block
/// instead, which the browser parses and never runs, and the boot script reads
/// it from there. That keeps `{{description_url}}` in the one context it is
/// escaped for, and keeps the policy a constant rather than a value derived
/// per mount.
///
/// Custom pages get neither header: Kynos cannot know what a page it did not
/// write loads, and a policy guessed for it would break it.
#[derive(Debug)]
pub(super) struct Shipped {
    /// The page, with both tokens still in it.
    pub(super) template: &'static str,
    /// The `Content-Security-Policy` the page is served under.
    pub(super) policy: &'static str,
}

/// The Scalar playground: a reference with a client built into it.
pub(super) const SCALAR: Shipped = Shipped {
    template: r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{title}}</title>
  </head>
  <body>
    <div id="app"></div>
    <script id="description-url" type="application/json">{{description_url}}</script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.73.1/dist/browser/standalone.js" integrity="sha384-kYDGzV91Jnn3TbHINV3nt54riK2uMJDfN5Al8dAkz4FssELTBWbD8rgw32sTKfOi" crossorigin="anonymous"></script>
    <script>Scalar.createApiReference('#app', { url: JSON.parse(document.getElementById('description-url').textContent) })</script>
  </body>
</html>
"#,
    policy: "script-src 'sha256-J/fJKAZX9bnXNfWmo/83p2nvJPUNFTingwNtRW2cTh0=' \
             https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.73.1/dist/browser/standalone.js; \
             object-src 'none'; base-uri 'none'",
};

/// Redoc: the same description, read-only, in three panels.
///
/// Booted from a script rather than from `<redoc spec-url="...">`, so the URL
/// lands in the one context this module escapes for. The element form would
/// need markup escaping and nothing else here would, which is a second rule
/// for one value.
///
/// Loaded from jsDelivr's copy of the npm package rather than from
/// `cdn.redoc.ly`, which serves the same bytes: one CDN origin for both pages,
/// and an npm version is immutable. `worker-src blob:` admits the search
/// worker the bundle builds from its own source; a blob is created by script
/// the policy already allows, so it admits nothing a page could inject.
pub(super) const REDOC: Shipped = Shipped {
    template: r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{title}}</title>
  </head>
  <body>
    <div id="redoc"></div>
    <script id="description-url" type="application/json">{{description_url}}</script>
    <script src="https://cdn.jsdelivr.net/npm/redoc@2.5.4/bundles/redoc.standalone.js" integrity="sha384-w447zOpYfw/1Tv/5AK9NfHTlQIqE3RVR6KY62jCyy9zNDgO64cMwGGP1Fj0zJVf5" crossorigin="anonymous"></script>
    <script>Redoc.init(JSON.parse(document.getElementById('description-url').textContent), {}, document.getElementById('redoc'))</script>
  </body>
</html>
"#,
    policy: "script-src 'sha256-PBZ5Sp8gfwtQxdNCcMjRlhBC7XcdtM/rWrspQE5kTKM=' \
             https://cdn.jsdelivr.net/npm/redoc@2.5.4/bundles/redoc.standalone.js; \
             worker-src blob:; object-src 'none'; base-uri 'none'",
};

/// Every page this module ships, for the sweeps in `tests.rs`.
///
/// A table rather than two constants named separately: a page added without a
/// case is what the sweeps exist to fail on, and they can only be total over a
/// set that is written down once.
#[cfg(test)]
pub(super) const SHIPPED: &[(&str, &Shipped)] = &[("scalar", &SCALAR), ("redoc", &REDOC)];

/// One page, with both values substituted.
pub(super) fn render(template: &str, description_url: &str, title: &str) -> String {
    // Written rather than hand-quoted. Hand-quoting would be correct only
    // because the path grammar happens to exclude `"` and `\` today, which is a
    // fact about another crate rather than about this one.
    let url = serde_json::to_string(description_url).expect("a `str` serializes as a JSON string");

    template
        .replace(DESCRIPTION_URL, &url)
        .replace(TITLE, &text(title))
}

/// `value` as HTML text.
///
/// `<title>` is RCDATA: `&` starts a character reference and `</title` ends the
/// element, so those are the whole contract. `>` is escaped too because a
/// custom page may place the title somewhere ordinary text is parsed.
fn text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }

    escaped
}
