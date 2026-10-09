//! The pages this module ships, and the two values substituted into them.
//!
//! Escaping is per token, since no one escaping fits both script and markup
//! (a path may hold `'` and `&`; a title is arbitrary prose).

/// Where the page fetches the description. Substituted as a JSON string,
/// quotes included.
pub(super) const DESCRIPTION_URL: &str = "{{description_url}}";

/// The document's title. Substituted as HTML text.
pub(super) const TITLE: &str = "{{title}}";

/// A page Kynos ships, and the policy it is served under.
///
/// The policy admits the pinned, integrity-checked bundle and the boot script
/// by hash. The boot script holds no token (it reads the URL from a JSON data
/// block), so its hash, and the policy, stay constant.
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
/// Booted from a script, so the URL needs only JSON escaping. From jsDelivr's
/// immutable npm copy, one origin for both pages. `worker-src blob:` admits the
/// search worker the already-allowed bundle builds.
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

/// Every page this module ships, for the sweeps in `tests.rs`; a new page
/// belongs here.
#[cfg(test)]
pub(super) const SHIPPED: &[(&str, &Shipped)] = &[("scalar", &SCALAR), ("redoc", &REDOC)];

/// One page, with both values substituted.
pub(super) fn render(template: &str, description_url: &str, title: &str) -> String {
    // Serialized, not hand-quoted, so it holds whatever the path grammar admits.
    let url = serde_json::to_string(description_url).expect("a `str` serializes as a JSON string");

    template
        .replace(DESCRIPTION_URL, &url)
        .replace(TITLE, &text(title))
}

/// `value` as HTML text: `&` and `<` for `<title>`'s RCDATA, and `>` for a
/// custom page placing it elsewhere.
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
