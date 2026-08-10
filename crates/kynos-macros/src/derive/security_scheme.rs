//! `#[derive(SecurityScheme)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, Type, parse_macro_input, parse_quote};

use crate::derive::common::unit_struct;

/// What the attribute says, before it is turned into a description.
#[derive(Default)]
struct SchemeArgs {
    /// The component name the scheme is registered under.
    name: Option<LitStr>,
    /// What a verified credential yields to the handler.
    credential: Option<Type>,
}

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // A scheme is a marker: it names a way of authenticating, and carries no
    // data of its own. The credential is what carries data, and it is named by
    // the associated type rather than by a field.
    unit_struct(input, "SecurityScheme", "names a way of authenticating")?;

    let args = parse_args(input)?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let declared = args
        .name
        .unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));
    let credential: Type = args.credential.unwrap_or_else(|| parse_quote!(String));

    Ok(quote! {
        impl #impl_generics ::kynos::security::SecurityScheme
            for #name #ty_generics #where_clause
        {
            const NAME: &'static str = #declared;

            type Credential = #credential;

            fn describe() -> ::kynos::openapi::SecurityScheme {
                ::core::todo!()
            }
        }
    })
}

fn parse_args(input: &DeriveInput) -> syn::Result<SchemeArgs> {
    let mut args = SchemeArgs::default();
    for attr in &input.attrs {
        if !attr.path().is_ident("security") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                args.name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("credential") {
                args.credential = Some(meta.value()?.parse()?);
            } else {
                // The scheme kind and its options are read when `describe` is
                // implemented; parsing past them keeps the attribute usable.
                let _ = meta.input.parse::<proc_macro2::TokenStream>();
            }
            Ok(())
        })?;
    }
    Ok(args)
}
