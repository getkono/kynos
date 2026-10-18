//! `#[derive(Provider)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Field, Type, parse_macro_input, spanned::Spanned};

use crate::derive::{attribute::key_name, common::named_fields};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Provider")?;

    let mut provided: Vec<&Field> = Vec::new();
    for field in &fields.named {
        if !is_skipped(field)? {
            provided.push(field);
        }
    }

    reject_duplicate_types(&provided)?;
    reject_type_parameter_fields(input, &provided)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // One implementation per field; a missing one fails at the mount site.
    let implementations = provided.iter().map(|field| {
        let ident = field.ident.as_ref().expect("named fields");
        let ty = &field.ty;
        quote! {
            impl #impl_generics ::kynos::di::Provides<#ty> for #name #ty_generics #where_clause {
                fn provide(&self) -> #ty {
                    ::core::clone::Clone::clone(&self.#ident)
                }
            }
        }
    });

    Ok(quote!(#(#implementations)*))
}

/// Whether the field opted out with `#[provide(skip)]`.
///
/// `skip` is the grammar's one key, and it is bare: anything else, and a
/// second `skip`, is refused rather than leaving the field provided.
fn is_skipped(field: &Field) -> syn::Result<bool> {
    let mut skipped = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("provide") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            if key != "skip" {
                return Err(syn::Error::new(
                    meta.path.span(),
                    format!("`{key}` is not part of the `#[provide(...)]` grammar"),
                ));
            }
            if skipped {
                return Err(syn::Error::new(
                    meta.path.span(),
                    "`skip` is already set, and `#[provide(...)]` takes each key once",
                ));
            }
            skipped = true;
            Ok(())
        })?;
    }
    Ok(skipped)
}

/// Two fields of the same type would emit the same implementation twice;
/// refused here so the error names both fields.
fn reject_duplicate_types(provided: &[&Field]) -> syn::Result<()> {
    for (index, field) in provided.iter().enumerate() {
        let rendered = render(&field.ty);
        if let Some(earlier) = provided[..index]
            .iter()
            .position(|seen| render(&seen.ty) == rendered)
        {
            let first = provided[earlier]
                .ident
                .as_ref()
                .map_or_else(String::new, ToString::to_string);
            let second = field
                .ident
                .as_ref()
                .map_or_else(String::new, ToString::to_string);
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "`{first}` and `{second}` are both `{rendered}`, so a handler asking for one \
                     could not say which. Give one a newtype, or opt it out with \
                     `#[provide(skip)]`"
                ),
            ));
        }
    }
    Ok(())
}

/// A field whose type is one of the context's own type parameters would emit
/// `impl<T> Provides<T> for Ctx<T>`, which overlaps every other field's
/// implementation at that instantiation.
fn reject_type_parameter_fields(input: &DeriveInput, provided: &[&Field]) -> syn::Result<()> {
    if provided.len() < 2 {
        return Ok(());
    }

    let parameters: Vec<String> = input
        .generics
        .type_params()
        .map(|parameter| parameter.ident.to_string())
        .collect();

    for field in provided {
        let rendered = render(&field.ty);
        if !parameters.contains(&rendered) {
            continue;
        }
        let named = field
            .ident
            .as_ref()
            .map_or_else(String::new, ToString::to_string);
        return Err(syn::Error::new(
            field.span(),
            format!(
                "`{named}` is `{rendered}`, one of this context's own type parameters, so it \
                 would supply every type at once and collide with the other fields. Wrap it in a \
                 newtype, or opt it out with `#[provide(skip)]`"
            ),
        ));
    }
    Ok(())
}

/// A type as written, normalized enough to compare two spellings; differing
/// spellings of one type fall through to coherence.
fn render(ty: &Type) -> String {
    quote!(#ty).to_string().replace(' ', "")
}
