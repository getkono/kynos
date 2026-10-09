//! `#[derive(Tag)]`.
//!
//! ```text
//! #[tag( <member> [, <member>]* )]           on the type, optional
//!
//! member := name = "<tag name>"
//!         | summary = "<one line>"           OpenAPI 3.2
//!         | description = "<CommonMark>"
//!         | parent = <Type>                  OpenAPI 3.2
//!         | kind = "<nav | badge | audience | ...>"   OpenAPI 3.2
//! ```
//!
//! The 3.2 members are refused under `openapi31` rather than dropped, as
//! [`security_scheme`](super::security_scheme) refuses its own: a failed build
//! names its remedy, a description that silently says less does not.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, Type, parse_macro_input};

use crate::derive::common::{doc_string, skip_value, unit_struct};

/// What the attribute said, before it becomes a tag.
#[derive(Default)]
struct TagArgs {
    name: Option<LitStr>,
    summary: Option<LitStr>,
    description: Option<LitStr>,
    parent: Option<Type>,
    kind: Option<LitStr>,
}

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    unit_struct(input, "Tag", "names a group of operations")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let args = parse_args(input)?;

    // Defaults to the type's identifier, so a typo is a compile error.
    let declared = args
        .name
        .unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));

    // Falls back to the doc comment.
    let described = args
        .description
        .map(|text| text.value())
        .or_else(|| doc_string(&input.attrs))
        .map(|text| {
            quote!(tag.description = ::core::option::Option::Some(
                ::std::string::String::from(#text)
            );)
        });

    // `summary`, `kind` and `parent` are OpenAPI 3.2 Tag Object fields; a 3.1
    // build refuses them (see the module docs).
    if !cfg!(feature = "openapi32") {
        for (key, span) in [
            (
                "summary",
                args.summary.as_ref().map(syn::spanned::Spanned::span),
            ),
            ("kind", args.kind.as_ref().map(syn::spanned::Spanned::span)),
            (
                "parent",
                args.parent.as_ref().map(syn::spanned::Spanned::span),
            ),
        ] {
            if let Some(span) = span {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "`{key}` writes a Tag Object field that OpenAPI 3.2 introduced, and this \
                         build describes 3.1; enable the `openapi32` feature, or drop it"
                    ),
                ));
            }
        }
    }

    let summary = args.summary.map(|text| {
        quote!(tag.summary = ::core::option::Option::Some(
            ::std::string::String::from(#text)
        );)
    });
    let kind = args.kind.map(|text| {
        quote!(tag.kind = ::core::option::Option::Some(
            ::std::string::String::from(#text)
        );)
    });
    let parent = args.parent.map(|parent| {
        quote! {
            tag.parent = ::core::option::Option::Some(
                ::std::string::String::from(
                    <#parent as ::kynos::router::operation::Tag>::NAME,
                ),
            );
        }
    });

    Ok(quote! {
        impl #impl_generics ::kynos::router::operation::Tag for #name #ty_generics #where_clause {
            const NAME: &'static str = #declared;

            fn metadata() -> ::kynos::openapi::Tag {
                let mut tag = ::kynos::openapi::Tag::new(Self::NAME);
                #described
                #summary
                #kind
                #parent
                tag
            }
        }
    })
}

/// Reads the type's `#[tag(...)]` lists, skipping keys it does not model.
fn parse_args(input: &DeriveInput) -> syn::Result<TagArgs> {
    let mut args = TagArgs::default();

    for attr in &input.attrs {
        if !attr.path().is_ident("tag") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return Ok(());
            };

            match key.to_string().as_str() {
                "name" => args.name = Some(meta.value()?.parse()?),
                "summary" => args.summary = Some(meta.value()?.parse()?),
                "description" => args.description = Some(meta.value()?.parse()?),
                "parent" => args.parent = Some(meta.value()?.parse()?),
                "kind" => args.kind = Some(meta.value()?.parse()?),
                _ => skip_value(&meta)?,
            }
            Ok(())
        })?;
    }

    Ok(args)
}
