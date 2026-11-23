//! `#[problem(variant_of = Wide::Variant)]`: one variant that is a wider
//! `ApiError` enum's variant, in an enum that is otherwise its own.
//!
//! What `subset_of` does for a whole enum, for one variant: it answers by
//! building the wider variant from its fields, by name, and is described as
//! the wider enum describes that variant, through the [`handles`] every derived
//! enum publishes. A field the wider variant lacks, or lacks and the variant
//! has, is a compile error where it is named. Every variant taken this way
//! gives `TryFrom<Wide> for Self`, whose `Error` is the wider value it did
//! not convert, so a value moves across without being rebuilt by hand.
//!
//! [`handles`]: super::subset::handles

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields, Generics, Ident, Path, Type, Variant, spanned::Spanned};

use super::{
    Transparent,
    args::{Markers, ProblemArgs},
    outcomes::generic_named_in,
    subset::{RESPONSES, expression_path},
};

/// Reads a variant declaring `variant_of = wide`, refusing what it cannot
/// also say.
pub(super) fn variant_of(
    variant: &Variant,
    args: &ProblemArgs,
    wide: &Path,
    span: Span,
    markers: &[Markers],
    generics: &Generics,
) -> syn::Result<Transparent> {
    if args.status.is_some()
        || args.title.is_some()
        || args.type_uri.is_some()
        || args.transparent.is_some()
        || !args.constants.is_empty()
    {
        return Err(syn::Error::new(
            span,
            "a variant `variant_of` names answers with the wider variant's declarations, so it \
             takes no `status`, `title`, `type`, `transparent` or `extension` of its own",
        ));
    }

    if let Some(span) = markers
        .iter()
        .find_map(|marked| marked.extension.or(marked.headers).or(marked.retry_after))
    {
        return Err(syn::Error::new(
            span,
            format!(
                "variant `{}` answers as the wider variant, whose field markers it takes; drop \
                 this `#[problem(...)]`",
                variant.ident
            ),
        ));
    }

    let segments: Vec<_> = wide.segments.iter().collect();
    let Some((last, enum_segments)) = segments.split_last().filter(|(_, rest)| !rest.is_empty())
    else {
        return Err(syn::Error::new(
            wide.span(),
            "`variant_of` names a variant through its enum, as `Wide::Variant`",
        ));
    };
    let of = last.ident.clone();
    let leading = wide.leading_colon;
    let ty: Type = syn::parse2(quote!(#leading #(#enum_segments)::*))?;

    if let Some(param) = generic_named_in(&ty, generics) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "a wider variant's statuses are a `const`, which cannot depend on the type's \
                 generic parameter `{param}`; name a concrete wider enum"
            ),
        ));
    }

    let bindings = bindings(&variant.fields);
    let path = expression_path(&ty);
    // Spanned to the wider variant, so one the wider enum lacks, or a field
    // it lacks, is reported where `variant_of` names it.
    let held = quote::quote_spanned!(of.span()=> #path::#of #bindings);

    Ok(Transparent {
        variant: variant.ident.clone(),
        bindings,
        held,
        ty,
        of: Some(of),
    })
}

/// The braced bindings of `fields`, by name or position, so the same tokens
/// are a pattern and an expression on either side of a conversion.
fn bindings(fields: &Fields) -> TokenStream2 {
    let bound = fields.iter().enumerate().map(|(index, field)| {
        let binding = format_ident!("__kynos_field_{index}", span = field.ty.span());
        let member = field.ident.as_ref().map_or_else(
            || {
                let index = syn::Index::from(index);
                quote!(#index)
            },
            |ident| quote!(#ident),
        );
        quote!(#member: #binding)
    });
    quote!({ #(#bound),* })
}

/// The responses the wider enum declares for `of` alone, reading `registry`.
pub(super) fn described_as(wide: &Type, of: &Ident) -> TokenStream2 {
    let responses = Ident::new(RESPONSES, Span::call_site());
    let name = of.to_string();
    quote!(<#wide>::#responses(registry, ::core::option::Option::Some(&[#name])))
}

/// `TryFrom<Wide> for Self` for each wider enum `delegates` take variants of,
/// moving each such variant's fields back, and handing any other wider value
/// back as the error; refusing two variants taking one wider variant, which
/// could not both convert back.
pub(super) fn conversions_back(
    input: &DeriveInput,
    delegates: &[&Transparent],
) -> syn::Result<TokenStream2> {
    // Grouped by the wider enum as written, in order of first appearance.
    let mut groups: Vec<(String, &Type, Vec<&Transparent>)> = Vec::new();
    for delegate in delegates {
        let Some(of) = &delegate.of else {
            continue;
        };
        let key = crate::derive::attribute::compact(&delegate.ty);
        let index = groups
            .iter()
            .position(|(seen, ..)| *seen == key)
            .unwrap_or_else(|| {
                groups.push((key, &delegate.ty, Vec::new()));
                groups.len() - 1
            });
        let group = &mut groups[index].2;
        if let Some(first) = group.iter().find(|taken| taken.of.as_ref() == Some(of)) {
            return Err(syn::Error::new(
                of.span(),
                format!(
                    "variant `{}` already is the wider variant `{of}`, and a wider value \
                     converts back into one variant; take it once",
                    first.variant
                ),
            ));
        }
        group.push(delegate);
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let impls = groups.iter().map(|(_, wide, taken)| {
        let arms = taken.iter().map(|delegate| {
            let variant = &delegate.variant;
            let held = &delegate.held;
            let bindings = &delegate.bindings;
            quote! {
                #held => ::core::result::Result::Ok(Self::#variant #bindings),
            }
        });
        quote! {
            #[automatically_derived]
            impl #impl_generics ::core::convert::TryFrom<#wide>
                for #name #ty_generics #where_clause
            {
                type Error = #wide;

                fn try_from(wide: #wide) -> ::core::result::Result<Self, #wide> {
                    // Unreachable where every wider variant is taken.
                    #[allow(unreachable_patterns)]
                    match wide {
                        #(#arms)*
                        other => ::core::result::Result::Err(other),
                    }
                }
            }
        }
    });

    Ok(quote!(#(#impls)*))
}
