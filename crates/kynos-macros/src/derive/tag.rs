//! `#[derive(Tag)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, parse_macro_input};

use crate::derive::common::unit_struct;

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    unit_struct(input, "Tag", "names a group of operations")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Defaulting to the type's own identifier is what makes a typo a compile
    // error: there is no string to get wrong unless one is written on purpose.
    let declared = tag_name(input)?.unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));

    Ok(quote! {
        impl #impl_generics ::kynos::router::operation::Tag for #name #ty_generics #where_clause {
            const NAME: &'static str = #declared;

            fn metadata() -> ::kynos::openapi::Tag {
                ::core::todo!()
            }
        }
    })
}

/// The `#[tag(name = "...")]` override, if one is written.
fn tag_name(input: &DeriveInput) -> syn::Result<Option<LitStr>> {
    let mut declared = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("tag") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                declared = Some(meta.value()?.parse::<LitStr>()?);
            } else {
                // `description`, `parent` and the rest are read when
                // `metadata` is implemented; parsing past them keeps the
                // attribute usable now.
                let _ = meta.input.parse::<proc_macro2::TokenStream>();
            }
            Ok(())
        })?;
    }
    Ok(declared)
}
