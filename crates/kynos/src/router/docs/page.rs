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

/// The Scalar playground: a reference with a client built into it.
pub(super) const SCALAR: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{title}}</title>
  </head>
  <body>
    <div id="app"></div>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
    <script>
      Scalar.createApiReference('#app', { url: {{description_url}} })
    </script>
  </body>
</html>
"#;

/// Redoc: the same description, read-only, in three panels.
///
/// Booted from a script rather than from `<redoc spec-url="...">`, so the URL
/// lands in the one context this module escapes for. The element form would
/// need markup escaping and nothing else here would, which is a second rule
/// for one value.
pub(super) const REDOC: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{title}}</title>
  </head>
  <body>
    <div id="redoc"></div>
    <script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
    <script>
      Redoc.init({{description_url}}, {}, document.getElementById('redoc'))
    </script>
  </body>
</html>
"#;

/// Every page this module ships, for the sweeps in `tests.rs`.
///
/// A table rather than two constants named separately: a page added without a
/// case is what the sweeps exist to fail on, and they can only be total over a
/// set that is written down once.
#[cfg(test)]
pub(super) const SHIPPED: &[(&str, &str)] = &[("scalar", SCALAR), ("redoc", REDOC)];

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
