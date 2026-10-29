//! Reading what an item's own Rust attributes say about it: its doc comment,
//! and whether it is deprecated. The route attributes and the derives read
//! both the same way.

use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// The text of every `#[doc = "..."]` on an item, in order.
pub(crate) fn doc_texts(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter_map(|attribute| match &attribute.meta {
            Meta::NameValue(pair) if pair.path.is_ident("doc") => match &pair.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The text of an item's doc comment as the CommonMark it was written in; the
/// fallback for an unwritten description.
pub(crate) fn doc_string(attrs: &[Attribute]) -> Option<String> {
    let text = doc_markdown(&doc_texts(attrs)).join("\n");
    let text = text.trim_start_matches('\n').trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The lines of a doc comment's `#[doc]` texts, with the indentation every
/// non-blank line shares removed and each blank line emptied.
///
/// Only the shared indentation goes - rustdoc's one space after `///` - so an
/// indented code block, a nested list item and a trailing two-space hard break
/// reach a CommonMark description as written. One text spanning several lines
/// contributes each of them. Indentation is counted in ASCII spaces and tabs,
/// so the shared prefix removed is a whole number of characters on every line.
pub(crate) fn doc_markdown(texts: &[String]) -> Vec<&str> {
    // `split` rather than `lines`, which yields nothing for `#[doc = ""]`.
    let lines: Vec<&str> = texts
        .iter()
        .flat_map(|text| text.split('\n'))
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let indent = |line: &str| {
        line.bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count()
    };
    let shared = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent(line))
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                ""
            } else {
                &line[shared..]
            }
        })
        .collect()
}

/// Whether the item carries Rust's own `#[deprecated]`.
///
/// The one source of `deprecated` for operations and schemas alike; Kynos has
/// no key of its own. The note is not read, since it addresses Rust callers.
pub(crate) fn is_deprecated(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("deprecated"))
}

#[cfg(test)]
mod tests;
