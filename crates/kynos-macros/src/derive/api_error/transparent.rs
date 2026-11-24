//! Transparent variants: what makes one — `#[problem(transparent)]` on it or
//! on its enum, or `#[error(transparent)]` — what each refuses, and what the
//! error it holds gives the enum beyond its answer: the enum's `Display`,
//! where the enum is transparent whole.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{DataEnum, DeriveInput, Generics, Index, Member, Variant, spanned::Spanned};

use super::{
    Failure, Transparent,
    args::{Markers, ProblemArgs, error_attribute},
    outcomes::generic_named_in,
};

/// Refuses an `#[error(...)]` on a transparent enum or its variants:
/// `thiserror`'s, which would implement the `Display` [`display`] does.
pub(super) fn display_unclaimed(input: &DeriveInput, data: &DataEnum) -> syn::Result<()> {
    let claimed = error_attribute(&input.attrs).or_else(|| {
        data.variants
            .iter()
            .find_map(|variant| error_attribute(&variant.attrs))
    });
    match claimed {
        Some(span) => Err(syn::Error::new(
            span,
            "a transparent enum displays as the error each variant holds, and `thiserror`'s \
             `#[error(...)]` would implement `Display` a second time. Drop the `#[error(...)]` \
             lists and `thiserror::Error`, or drop `transparent` from the enum",
        )),
        None => Ok(()),
    }
}

/// The `Display` the type has or needs: delegated to each held error where
/// the enum is `whole`ly transparent; asserted where a failure of its own
/// reads it for `detail`; neither where no failure does, since a held error's
/// problem carries its own.
pub(super) fn display(
    input: &DeriveInput,
    whole: bool,
    failures: &[&Failure],
    delegates: &[&Transparent],
) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    if whole {
        let arms = delegates.iter().map(|delegate| {
            let variant = &delegate.variant;
            let bindings = &delegate.bindings;
            // Spanned to the field's type, so one with no `Display` is
            // reported where it is named: the bound is the argument's.
            let span = delegate.ty.span();
            let held: TokenStream2 = delegate
                .held
                .clone()
                .into_iter()
                .map(|mut token| {
                    token.set_span(span);
                    token
                })
                .collect();
            let fmt = quote_spanned!(span=> ::core::fmt::Display::fmt);
            quote!(Self::#variant #bindings => #fmt(#held, formatter),)
        });
        // An enum with no variants has no value to display, and `match self`
        // over a reference to one is not exhaustive.
        let matched = if delegates.is_empty() {
            quote!(*self)
        } else {
            quote!(self)
        };
        return quote! {
            #[automatically_derived]
            impl #impl_generics ::core::fmt::Display for #name #ty_generics #where_clause {
                fn fmt(
                    &self,
                    formatter: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    match #matched {
                        #(#arms)*
                    }
                }
            }
        };
    }

    if failures.is_empty() {
        return TokenStream2::new();
    }

    // Asserted rather than bounded so the diagnostic lands on the error type,
    // not the handler returning it.
    quote! {
        const _: () = {
            #[allow(dead_code)]
            fn detail_comes_from_display #impl_generics () #where_clause {
                fn is_display<T: ::core::fmt::Display + ?::core::marker::Sized>() {}
                is_display::<#name #ty_generics>();
            }
        };
    }
}

/// Reads a `#[problem(transparent)]` variant, refusing what it cannot also
/// say.
pub(super) fn read(
    variant: &Variant,
    args: &ProblemArgs,
    span: Span,
    markers: &[Markers],
    generics: &Generics,
) -> syn::Result<Transparent> {
    if args.status.is_some() || args.title.is_some() || args.type_uri.is_some() {
        return Err(syn::Error::new(
            span,
            "a transparent variant answers as the error it holds, so its status, title and type \
             are that error's. Drop them, or drop `transparent`",
        ));
    }

    if let Some(constant) = args.constants.first() {
        return Err(syn::Error::new(
            constant.span,
            "a transparent variant publishes the extension members of the error it holds, so it \
             sends no constant of its own. Declare it on that error, or drop `transparent`",
        ));
    }

    let mut fields = variant.fields.iter();
    let (Some(field), None) = (fields.next(), fields.next()) else {
        return Err(syn::Error::new(
            variant.ident.span(),
            format!(
                "transparent variant `{}` holds exactly one field, the error it answers as",
                variant.ident
            ),
        ));
    };

    // The one field's, since `variant` has exactly one.
    let marked = markers.first().copied().unwrap_or_default();

    if let Some(span) = marked.extension {
        return Err(syn::Error::new(
            span,
            "a transparent variant publishes the extension members of the error it holds, not \
             the field holding it",
        ));
    }

    if let Some(span) = marked.headers {
        return Err(syn::Error::new(
            span,
            "a transparent variant sends the headers of the error it holds, which is not a \
             header group itself",
        ));
    }

    if let Some(span) = marked.retry_after {
        return Err(syn::Error::new(
            span,
            "a transparent variant sends the `Retry-After` of the error it holds, which is not a \
             delay itself",
        ));
    }

    if let Some(param) = generic_named_in(&field.ty, generics) {
        return Err(syn::Error::new(
            field.ty.span(),
            format!(
                "a transparent variant's statuses are a `const`, which cannot depend on the \
                 type's generic parameter `{param}`; name a concrete error type"
            ),
        ));
    }

    let member = field
        .ident
        .clone()
        .map_or_else(|| Member::Unnamed(Index::from(0)), Member::Named);
    Ok(Transparent::field(
        variant.ident.clone(),
        &member,
        field.ty.clone(),
    ))
}

/// Reads a variant of an enum `transparent` marks whole, refusing a variant
/// that says so again or takes a wider enum's variant instead.
pub(super) fn whole(
    variant: &Variant,
    args: &ProblemArgs,
    whole: Span,
    markers: &[Markers],
    generics: &Generics,
) -> syn::Result<Transparent> {
    if let Some(span) = args
        .transparent
        .or_else(|| args.variant_of.as_ref().map(|(_, span)| *span))
    {
        return Err(syn::Error::new(
            span,
            format!(
                "the enum is transparent, so variant `{}` already answers as the one error it \
                 holds; drop this key, or drop `transparent` from the enum",
                variant.ident
            ),
        ));
    }
    read(variant, args, whole, markers, generics)
}

/// Reads a variant `#[error(transparent)]` makes transparent, having declared
/// no `status`: a `title`, `type` or constant member beside it would be
/// dropped as the held error's, so it is refused naming the opt-out.
pub(super) fn implied(
    variant: &Variant,
    args: &ProblemArgs,
    span: Span,
    markers: &[Markers],
    generics: &Generics,
) -> syn::Result<Transparent> {
    if args.title.is_some() || args.type_uri.is_some() || !args.constants.is_empty() {
        return Err(syn::Error::new(
            span,
            format!(
                "`#[error(transparent)]` makes variant `{}` answer as the error it holds, whose \
                 title, type and extension members are that error's. Declare \
                 `#[problem(status = ...)]` beside them for a failure of its own, or drop them",
                variant.ident
            ),
        ));
    }
    read(variant, args, span, markers, generics)
}
