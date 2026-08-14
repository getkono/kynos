//! `#[derive(Provider)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Field, Type, parse_macro_input, spanned::Spanned};

use crate::derive::common::{named_fields, skip_value};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Provider")?;

    let provided: Vec<&Field> = fields
        .named
        .iter()
        .filter(|field| !is_skipped(field))
        .collect();

    reject_duplicate_types(&provided)?;
    reject_type_parameter_fields(input, &provided)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // One implementation per field. A handler asking for something no field
    // supplies fails to typecheck at the mount site, which is where the
    // context type first becomes concrete.
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
fn is_skipped(field: &Field) -> bool {
    field.attrs.iter().any(|attr| {
        if !attr.path().is_ident("provide") {
            return false;
        }
        let mut skipped = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                skipped = true;
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        });
        skipped
    })
}

/// Two fields of the same type would emit the same implementation twice.
///
/// Reported here rather than left to coherence, which would blame the derive
/// output for a mistake in the input and name neither field.
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
///
/// Coherence catches it, but blames the derive's own output and names neither
/// field — the exact diagnostic `reject_duplicate_types` exists to prevent.
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

/// A type as written, normalized enough to compare two spellings.
///
/// Textual rather than semantic: two different spellings of one type slip
/// through and are caught by coherence instead, which is a worse diagnostic but
/// not a wrong one.
fn render(ty: &Type) -> String {
    quote!(#ty).to_string().replace(' ', "")
}
