//! `#[derive(ParamValue)]`.
//!
//! A field-less enum's text form is the names the `Schema` derive gives its
//! variants: `Display` writes the one serde writes, or for an identifier enum,
//! which serde never writes, the one serde reads it by; `FromStr` reads every
//! one serde reads, aliases included, which is the schema's `enum`. The
//! `Schema` derive's refusals run first, so the names it would refuse never
//! reach a decoder.
//!
//! A newtype's text form is its field's, as serde writes it: a one-field tuple
//! struct, or a one-field struct under `#[serde(transparent)]`. Both impls
//! delegate to the field, whose type is bounded by `ParamValue`, so the
//! wrapper carries one value only where what it wraps does.

use quote::quote;
use syn::{
    Data, DataStruct, DeriveInput, Fields, Index, LitStr, Member, parse_quote_spanned,
    spanned::Spanned,
};

use crate::derive::schema::{attributes::serde_key_span, members::variant_wire_names};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(data) => return newtype(input, data),
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`ParamValue` is derived for a field-less enum, whose variant names are its text \
                 form, or for a newtype, whose field's is; a union is neither",
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

    let variants: Vec<_> = data.variants.iter().collect();
    let names = variant_wire_names(input, &variants)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

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

/// The impls for a newtype, each delegating to its one field: a tuple
/// struct's, or a named one's that `#[serde(transparent)]` writes as itself.
fn newtype(input: &DeriveInput, data: &DataStruct) -> syn::Result<proc_macro2::TokenStream> {
    let field = match &data.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0],
        Fields::Named(fields)
            if fields.named.len() == 1
                && serde_key_span(&input.attrs, &["transparent"]).is_some() =>
        {
            &fields.named[0]
        }
        Fields::Named(fields) => {
            return Err(syn::Error::new(
                fields.span(),
                "serde writes a struct with named fields as an object keyed by them, which no \
                 parameter text holds. Make it a newtype, `struct Name(Inner);`, whose text form \
                 is its field's, or write one field under `#[serde(transparent)]`",
            ));
        }
        Fields::Unnamed(fields) => {
            return Err(syn::Error::new(
                fields.span(),
                "a parameter is one value, and this tuple struct holds several or none. A \
                 newtype's text form is its one field's: hold exactly one",
            ));
        }
        Fields::Unit => {
            return Err(syn::Error::new(
                input.ident.span(),
                "a unit struct holds no value a parameter could carry. Hold the value in one \
                 field, `struct Name(Inner);`",
            ));
        }
    };

    let name = &input.ident;
    let inner = &field.ty;
    let member = field.ident.as_ref().map_or_else(
        || Member::Unnamed(Index::from(0)),
        |ident| Member::Named(ident.clone()),
    );
    let construct = match &member {
        Member::Unnamed(_) => quote!(Self),
        Member::Named(ident) => quote!(|value| Self { #ident: value }),
    };
    let mut generics = input.generics.clone();
    // Spanned to the field, so an inner type that is not one value is
    // reported where it is named.
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote_spanned!(inner.span()=> #inner: ::kynos::schema::ParamValue));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let from_str = quote!(<#inner as ::core::str::FromStr>);

    Ok(quote! {
        // As for an enum: the impls name the type, which may be deprecated.
        #[allow(deprecated)]
        impl #impl_generics ::core::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.#member, f)
            }
        }

        #[allow(deprecated)]
        impl #impl_generics ::core::str::FromStr for #name #ty_generics #where_clause {
            type Err = #from_str::Err;

            fn from_str(raw: &str) -> ::core::result::Result<Self, Self::Err> {
                ::core::result::Result::map(#from_str::from_str(raw), #construct)
            }
        }

        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::ParamValue for #name #ty_generics #where_clause {}
    })
}
