//! Reading the attributes already on a handler: its doc comment, and whether it
//! is deprecated.

use syn::{ItemFn, Meta};

use crate::route::args::expect_str;

/// Splits a doc comment into its summary and description.
///
/// The first paragraph becomes the operation's `summary` and the remainder its
/// `description`, matching how the specification distinguishes the two.
pub(crate) fn split_doc(lines: &[String]) -> (Option<String>, Option<String>) {
    let trimmed: Vec<&str> = lines.iter().map(|line| line.trim()).collect();
    let first_blank = trimmed.iter().position(|line| line.is_empty());

    match first_blank {
        None if trimmed.is_empty() => (None, None),
        None => (Some(trimmed.join(" ")), None),
        Some(index) => {
            let summary = trimmed[..index].join(" ");
            let rest = trimmed[index + 1..].join("\n");
            let rest = rest.trim().to_owned();
            (
                (!summary.is_empty()).then_some(summary),
                (!rest.is_empty()).then_some(rest),
            )
        }
    }
}

/// Collects the text of every `#[doc]` attribute on an item.
pub(crate) fn doc_lines(function: &ItemFn) -> Vec<String> {
    function
        .attrs
        .iter()
        .filter_map(|attribute| {
            let Meta::NameValue(pair) = &attribute.meta else {
                return None;
            };
            if !pair.path.is_ident("doc") {
                return None;
            }
            expect_str(&pair.value).ok().map(|literal| literal.value())
        })
        .collect()
}

/// Whether the handler carries `#[deprecated]`, which becomes
/// `Operation.deprecated`.
///
/// Delegates to [`crate::derive::common::is_deprecated`] so an operation and a
/// schema answer the question the same way. They did not always: this read the
/// function's attributes and the `Schema` derive read nothing at all, so a
/// deprecated field reached no description.
pub(crate) fn is_deprecated(function: &ItemFn) -> bool {
    crate::derive::common::is_deprecated(&function.attrs)
}
