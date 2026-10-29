//! `#[derive(ParamValue)]`.
//!
//! A field-less enum's text form is the names the `Schema` derive gives its
//! variants: `Display` writes the one serde writes, or for an identifier enum,
//! which serde never writes, the one serde reads it by; `FromStr` reads every
//! one serde reads, aliases included, which is the schema's `enum`. The
//! `Schema` derive's refusals run first, so the names it would refuse never
//! reach a decoder.

use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, spanned::Spanned};

use crate::derive::schema::{
    attributes::{serde_key_span, variant_wire_names},
    refusals,
};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) | Data::Union(_) => {
            return Err(syn::Error::new(
                input.ident.span(),
                "`ParamValue` is derived for a field-less enum, whose variant names are its text \
                 form; implement `FromStr`, `Display` and `ParamValue` by hand for a newtype",
            ));
        }
    };

    if let Some(variant) = data
        .variants
        .iter()
        .find(|variant| !matches!(variant.fields, Fields::Unit))
    {
        return Err(syn::Error::new(
            variant.fields.span(),
            "a parameter is one bare name, and serde writes a variant declaring fields, even an \
             empty list of them, as an object keyed by its name, which no parameter text holds. \
             Make every variant a unit, or implement `FromStr`, `Display` and `ParamValue` by \
             hand",
        ));
    }

    if data.variants.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            "an enum with no variant has no value a parameter could carry, so every request \
             naming it would be refused. Declare the variants it accepts",
        ));
    }

    if let Some((_, span)) = serde_key_span(&input.attrs, &["tag"]) {
        return Err(syn::Error::new(
            span,
            "serde writes an internally or adjacently tagged variant as an object holding the \
             tag, which no parameter text holds. Drop `tag`, so each variant is its bare name",
        ));
    }

    if let Some((key, span)) = data
        .variants
        .iter()
        .find_map(|variant| serde_key_span(&variant.attrs, &["skip", "skip_serializing"]))
    {
        return Err(syn::Error::new(
            span,
            format!(
                "`{key}` leaves this variant without a name serde writes, so `Display` would have \
                 no text for it. Drop the variant, or drop `{key}`"
            ),
        ));
    }

    refusals::check(input)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let variants: Vec<_> = data.variants.iter().collect();
    let names = variant_wire_names(input, &variants);

    let written = names.iter().map(|(variant, wire, _)| {
        let ident = &variant.ident;
        let wire = LitStr::new(wire, ident.span());
        quote!(Self::#ident => #wire)
    });
    let read = names.iter().map(|(variant, _, read)| {
        let ident = &variant.ident;
        quote!(#(#read)|* => ::core::result::Result::Ok(Self::#ident))
    });
    let expected = names.iter().flat_map(|(_, _, read)| read);

    Ok(quote! {
        // The impls name the type, so a `#[deprecated]` type would otherwise
        // warn at its own definition, as the `Schema` derive also allows.
        #[allow(deprecated)]
        impl #impl_generics ::core::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.pad(match self {
                    #(#written,)*
                })
            }
        }

        #[allow(deprecated)]
        impl #impl_generics ::core::str::FromStr for #name #ty_generics #where_clause {
            type Err = ::kynos::schema::UnknownVariant;

            fn from_str(raw: &str) -> ::core::result::Result<Self, Self::Err> {
                match raw {
                    #(#read,)*
                    _ => ::core::result::Result::Err(::kynos::schema::UnknownVariant::new(&[
                        #(#expected),*
                    ])),
                }
            }
        }

        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::ParamValue for #name #ty_generics #where_clause {}
    })
}
