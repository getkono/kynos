//! `#[derive(Schema)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, LitStr, parse_macro_input, spanned::Spanned};

use crate::derive::common::skip_value;

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`Schema` cannot describe a union: no JSON value corresponds to one",
        ));
    }
    reject_untagged(input)?;

    let name = &input.ident;
    let generics = schema_bounded_generics(input);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Only a concrete type claims a component name. A generic one would give
    // every instantiation the same name, so `Page<User>` and `Page<Order>`
    // would collide in `components`; mangling the arguments into a legal
    // component key is the eventual answer, and inlining is the honest
    // placeholder rather than a name that is wrong.
    let component = LitStr::new(&name.to_string(), name.span());
    let named = if input.generics.type_params().next().is_some() {
        quote!(::core::option::Option::None)
    } else {
        quote!(::kynos::openapi::ComponentName::sanitized(#component).ok())
    };

    Ok(quote! {
        impl #impl_generics ::kynos::schema::Schema for #name #ty_generics #where_clause {
            fn schema(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Schema {
                let _ = registry;
                ::core::todo!()
            }

            fn name() -> ::core::option::Option<::kynos::openapi::ComponentName> {
                #named
            }
        }
    })
}

/// The input's generics, with `Schema` required of each type parameter.
///
/// serde's own default shape, and for the same reason: bounding the
/// *parameters* rather than the field types is both sufficient and narrower.
/// `Vec<T>: Schema` follows from `T: Schema` through the blanket
/// implementation, while a field-type bound would demand `PhantomData<T>:
/// Schema` — a bound nothing satisfies, on a field no schema describes, failing
/// at the handler rather than here.
///
/// Emitted now because the implementation will need it, and adding a bound
/// after the freeze breaks exactly the code this milestone invites people to
/// write.
fn schema_bounded_generics(input: &DeriveInput) -> syn::Generics {
    let mut generics = input.generics.clone();
    let parameters: Vec<syn::Ident> = generics
        .type_params()
        .map(|parameter| parameter.ident.clone())
        .collect();
    if parameters.is_empty() {
        return generics;
    }

    let clause = generics.make_where_clause();
    for parameter in parameters {
        clause
            .predicates
            .push(syn::parse_quote!(#parameter: ::kynos::schema::Schema));
    }
    generics
}

/// `#[serde(untagged)]` has no describable decoding rule.
///
/// `anyOf` with no discriminator leaves a consumer to guess which branch a
/// payload is, and serde's first-match tie-break is not expressible in JSON
/// Schema. An internally or adjacently tagged enum becomes a `discriminator`,
/// which is.
fn reject_untagged(input: &DeriveInput) -> syn::Result<()> {
    for attr in &input.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("untagged") {
                found = Some(meta.path.span());
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        });
        if let Some(span) = found {
            return Err(syn::Error::new(
                span,
                "an untagged enum has no describable decoding rule: `anyOf` without a \
                 discriminator is ambiguous, and serde's first-match tie-break cannot be \
                 expressed. Use `#[serde(tag = \"...\")]`, which becomes a `discriminator`",
            ));
        }
    }
    Ok(())
}
