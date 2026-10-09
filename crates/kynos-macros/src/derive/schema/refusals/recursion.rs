//! Types whose schema would never end.

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::{DeriveInput, Ident};

use crate::derive::schema::{
    attributes::{is_described, is_phantom},
    described_groups,
};

/// A generic type naming itself in a member its schema describes is refused.
///
/// A generic type has no component name, so the registry inlines it, and an
/// inlined body cannot be stood in for while it is being built: the
/// description would recurse without end. A concrete type is named, and
/// `$ref`s itself. Only the direct case is visible here; generic types
/// reaching each other are refused by `Registry::resolve` when the document is
/// built.
///
/// The type is named by its bare identifier or by `Self`. A path qualified to
/// it, `crate::Node`, is left alone, since it may name another type of the
/// same identifier.
pub(super) fn reject_recursive_generic(input: &DeriveInput) -> syn::Result<()> {
    if input.generics.type_params().next().is_none() {
        return Ok(());
    }

    let site = described_groups(input)
        .into_iter()
        .flatten()
        .filter(|field| is_described(field) && !is_phantom(&field.ty))
        .find_map(|field| self_reference(field.ty.to_token_stream(), &input.ident));
    let Some(span) = site else {
        return Ok(());
    };
    Err(syn::Error::new(
        span,
        format!(
            "`{name}` refers to itself, but a generic type has no component name and is \
             inlined, so its schema would never end. Make the recursive type concrete, or \
             implement `Schema` for it by hand returning a `name()`",
            name = input.ident,
        ),
    ))
}

/// Where `tokens` name `ty`: by its identifier at the head of a path, or as
/// `Self`.
fn self_reference(tokens: TokenStream, ty: &Ident) -> Option<Span> {
    let mut qualified = false;
    for token in tokens {
        match &token {
            TokenTree::Ident(ident) if ident == "Self" || (!qualified && ident == ty) => {
                return Some(ident.span());
            }
            TokenTree::Group(group) => {
                if let Some(span) = self_reference(group.stream(), ty) {
                    return Some(span);
                }
            }
            _ => {}
        }
        // An identifier after `::` is a segment of a longer path.
        qualified = matches!(&token, TokenTree::Punct(punct) if punct.as_char() == ':');
    }
    None
}
