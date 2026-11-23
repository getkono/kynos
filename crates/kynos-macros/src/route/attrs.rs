//! Reading what a handler's other attributes say about its operation: its doc
//! comment as the summary and description, and `#[kynos::public]`.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::ToTokens;
use syn::{Attribute, ItemFn, Meta, spanned::Spanned};

use crate::doc::doc_markdown;

/// The route attributes, by the last segment of the path naming them, which is
/// all a `use` leaves of it.
const ROUTE_ATTRIBUTES: &[&str] = &[
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "head",
    "options",
    "trace",
    "query",
    "operation",
];

/// Whether `attribute` is `#[kynos::public]`, by path or after a `use`.
fn is_public(attribute: &Attribute) -> bool {
    let segments: Vec<String> = attribute
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments == ["public"] || segments == ["kynos", "public"]
}

/// The refusal of anything written after `#[kynos::public]`.
fn no_arguments(span: Span) -> syn::Error {
    syn::Error::new(
        span,
        "`#[kynos::public]` takes no arguments; it only declares that the operation takes no \
         guard on purpose",
    )
}

/// Removes `#[kynos::public]` from a route handler's attributes, answering
/// whether it was there.
pub(crate) fn take_public(attributes: &mut Vec<Attribute>) -> syn::Result<bool> {
    let mut found = false;
    let mut kept = Vec::with_capacity(attributes.len());
    for attribute in attributes.drain(..) {
        if !is_public(&attribute) {
            kept.push(attribute);
            continue;
        }
        if !matches!(attribute.meta, Meta::Path(_)) {
            return Err(no_arguments(attribute.meta.span()));
        }
        if found {
            return Err(syn::Error::new(
                attribute.meta.span(),
                "`#[kynos::public]` is already given; an operation is declared public once",
            ));
        }
        found = true;
    }
    *attributes = kept;
    Ok(found)
}

/// Expands `#[kynos::public]` where it is read before the route attribute,
/// written above it: moves it below that attribute, which reads and removes
/// it, refusing it on a function carrying none.
pub(crate) fn expand_public(
    attribute: &TokenStream2,
    item: TokenStream2,
) -> syn::Result<TokenStream2> {
    if !attribute.is_empty() {
        return Err(no_arguments(attribute.span()));
    }

    let mut function: ItemFn = syn::parse2(item)?;
    let Some(route) =
        function.attrs.iter().position(|attribute| {
            attribute.path().segments.last().is_some_and(|segment| {
                ROUTE_ATTRIBUTES.contains(&segment.ident.to_string().as_str())
            })
        })
    else {
        return Err(syn::Error::new(
            function.sig.ident.span(),
            "`#[kynos::public]` declares an operation public, and this function is none; put it \
             beside a route attribute such as `#[kynos::get(\"/path\")]`",
        ));
    };

    function
        .attrs
        .insert(route + 1, syn::parse_quote!(#[::kynos::public]));
    // Refused now, in the order written, rather than by the route attribute.
    take_public(&mut function.attrs.clone())?;
    Ok(function.into_token_stream())
}

/// Splits a doc comment into its summary and description.
///
/// The first paragraph becomes the operation's `summary` and the remainder its
/// `description`, matching how the specification distinguishes the two. The
/// summary is plain text, so its lines join into one; the description is
/// CommonMark, so it keeps the indentation its Markdown depends on, less what
/// every line of the comment shares (see [`doc_markdown`]).
pub(crate) fn split_doc(texts: &[String]) -> (Option<String>, Option<String>) {
    let lines = doc_markdown(texts);
    let first_blank = lines.iter().position(|line| line.is_empty());
    let (summary, rest) = match first_blank {
        Some(index) => (&lines[..index], &lines[index + 1..]),
        None => (&lines[..], &[][..]),
    };

    let summary = summary
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let rest = rest.join("\n");
    let rest = rest.trim_start_matches('\n').trim_end();
    (
        (!summary.is_empty()).then_some(summary),
        (!rest.is_empty()).then(|| rest.to_owned()),
    )
}
