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
//! The 3.2 members are read whatever the build, and emitted only where the
//! document model has a field for them: a member that silently did nothing
//! under `openapi31` would be worse than one that is simply not carried, and a
//! diagnostic here would make enabling a feature elsewhere in the dependency
//! graph change whether an application compiles.

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

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    unit_struct(input, "Tag", "names a group of operations")?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let args = parse_args(input)?;

    // Defaulting to the type's own identifier is what makes a typo a compile
    // error: there is no string to get wrong unless one is written on purpose.
    let declared = args
        .name
        .unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));

    // The doc comment a Rust reader already sees, when no description was
    // written: a tag's prose is the same prose either way, and asking for it
    // twice is how the two come to differ.
    let described = args
        .description
        .map(|text| text.value())
        .or_else(|| doc_string(&input.attrs))
        .map(|text| {
            quote!(tag.description = ::core::option::Option::Some(
                ::std::string::String::from(#text)
            );)
        });

    let extended = cfg!(feature = "openapi32");
    let summary = args.summary.filter(|_| extended).map(|text| {
        quote!(tag.summary = ::core::option::Option::Some(
            ::std::string::String::from(#text)
        );)
    });
    let kind = args.kind.filter(|_| extended).map(|text| {
        quote!(tag.kind = ::core::option::Option::Some(
            ::std::string::String::from(#text)
        );)
    });
    let parent = args.parent.filter(|_| extended).map(|parent| {
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

/// Reads the type's `#[tag(...)]` lists.
///
/// Silent about a key it does not model, as every Kynos attribute is: the
/// grammar grows, and a key this derive has not learned yet is not a mistake.
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
