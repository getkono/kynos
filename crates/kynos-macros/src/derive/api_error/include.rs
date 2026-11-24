//! `#[problem(include)]`: a transparent variant holding a shared `ApiError`
//! enum, which the enum is also built from wherever an error converts into
//! that shared enum, so `?` reaches it from the shared enum's own sources.
//!
//! The `From` is generic over that source, so it overlaps any other `From`
//! whose source could convert into the shared enum. Two `include`s always
//! overlap, and an `include` beside a `from` holding its type always does,
//! so both are refused here by name; whether a `From` of another type
//! overlaps is the trait solver's to say, at the enum.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Generics, Token, Variant, meta::ParseNestedMeta, spanned::Spanned};

use super::{
    Transparent,
    args::{Markers, Position, ProblemArgs},
    transparent,
};
use crate::derive::attribute::compact;

/// Reads `include` at `position`: bare, and on a variant, since a derive
/// cannot add the variant the shared enum would be spliced into.
pub(super) fn parse(meta: &ParseNestedMeta<'_>, position: Position) -> syn::Result<Span> {
    let span = meta.path.span();
    if position == Position::Type {
        return Err(syn::Error::new(
            span,
            "a derive cannot add a variant to the enum it reads, so `include` marks the variant \
             holding the shared enum: write `#[problem(include)] Shared(SharedError)` among the \
             variants",
        ));
    }
    if meta.input.peek(Token![=]) || meta.input.peek(syn::token::Paren) {
        return Err(syn::Error::new(
            span,
            "`include` is a bare marker, the shared enum being the type of the variant's one \
             field; write `#[problem(include)]`",
        ));
    }
    Ok(span)
}

/// Reads an `include` variant, `whole` the span of its enum's `from` where
/// the enum is transparent whole and says it: transparent, and built from
/// every error converting into what it holds, so neither its enum nor itself
/// says `from` beside it, nor its field `thiserror`'s `#[from]`, and it takes
/// no `variant_of`.
pub(super) fn read(
    variant: &Variant,
    args: &ProblemArgs,
    whole: Option<Span>,
    markers: &[Markers],
    generics: &Generics,
) -> syn::Result<Transparent> {
    let span = args
        .include
        .expect("read only where the variant says `include`");
    if let Some(from) = whole {
        return Err(syn::Error::new(
            from,
            format!(
                "`from` on the enum builds variant `{}` from the shared enum it holds, which its \
                 `include` already does; drop `from` from the enum and mark each other variant \
                 `#[problem(from)]`",
                variant.ident
            ),
        ));
    }
    if let Some(from) = args.from {
        return Err(syn::Error::new(
            from,
            format!(
                "`include` already builds variant `{}` from the shared enum it holds and from \
                 every error converting into it; drop `from`",
                variant.ident
            ),
        ));
    }
    if let Some((_, of)) = &args.variant_of {
        return Err(syn::Error::new(
            *of,
            "`include` holds a shared enum whole, and `variant_of` takes one variant of a wider \
             enum; drop one of them",
        ));
    }

    let mut delegate = transparent::read(variant, args, span, None, markers, generics)?;
    if let Some(attr) = variant
        .fields
        .iter()
        .flat_map(|field| &field.attrs)
        .find(|attr| attr.path().is_ident("from"))
    {
        return Err(syn::Error::new(
            attr.path().span(),
            "`thiserror`'s `#[from]` implements a `From` the `include` beside it already does; \
             drop `#[from]`",
        ));
    }
    delegate.include = Some(span);
    Ok(delegate)
}

/// Refuses `delegate`, an `include`, where one of the `earlier` converting
/// delegates is one too: both `From`s build the enum from an error
/// converting into both shared enums.
pub(super) fn unique(delegate: &Transparent, earlier: &[&Transparent]) -> syn::Result<()> {
    let (Some(span), Some(first)) = (
        delegate.include,
        earlier.iter().find(|earlier| earlier.include.is_some()),
    ) else {
        return Ok(());
    };
    Err(syn::Error::new(
        span,
        format!(
            "variant `{}` already includes `{}`, building the enum from every error converting \
             into it, and a second `include` would build it from an error converting into both \
             as well; gather the shared failures into one enum, or mark this variant \
             `#[problem(transparent, from)]`",
            first.variant,
            compact(&first.ty)
        ),
    ))
}

/// `From<Source> for Self` wherever `Source: Into<Held>`, building the
/// variant from what the source converts into; the held error itself is one
/// such source.
pub(super) fn conversion(input: &syn::DeriveInput, delegate: &Transparent) -> TokenStream2 {
    let name = &input.ident;
    let ty = &delegate.ty;
    let variant = &delegate.variant;
    let bindings = &delegate.bindings;
    let held = &delegate.held;

    // The held type is concrete, as `transparent::read` requires, so the
    // parameter added here is the only one its bound names.
    let mut generics = input.generics.clone();
    generics
        .params
        .push(syn::parse_quote!(__KynosSource: ::core::convert::Into<#ty>));
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = input.generics.split_for_impl();
    quote! {
        #[automatically_derived]
        impl #impl_generics ::core::convert::From<__KynosSource> for #name #ty_generics
            #where_clause
        {
            fn from(source: __KynosSource) -> Self {
                let #held: #ty = ::core::convert::Into::into(source);
                Self::#variant #bindings
            }
        }
    }
}
