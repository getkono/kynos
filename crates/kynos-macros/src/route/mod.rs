//! Expansion of the route attributes and the `routes!` / `path!` macros.

pub(crate) mod args;
pub(crate) mod attrs;
pub(crate) mod emit;
pub(crate) mod path;
pub(crate) mod routes;
pub(crate) mod uri;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{ItemFn, Meta, Token, parse::Parser, parse_macro_input, punctuated::Punctuated};

use crate::route::{
    args::{RouteArgs, expect_str, prepend_path_name},
    emit::emit,
};

/// The methods that have a dedicated Path Item field in OpenAPI 3.1.
const STANDARD_METHODS: &[&str] = &[
    "GET", "PUT", "POST", "DELETE", "OPTIONS", "HEAD", "PATCH", "TRACE",
];

/// Expands a route attribute for a method with a dedicated Path Item field.
pub(crate) fn expand(method: &str, attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = match RouteArgs::parse(prepend_path_name(attribute.into())) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(method, &args, &function).into()
}

/// Expands `#[kynos::operation(method = "...", path = "...")]`.
pub(crate) fn expand_generic(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let tokens: TokenStream2 = attribute.into();

    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let items = match parser.parse2(tokens.clone()) {
        Ok(items) => items,
        Err(error) => return error.to_compile_error().into(),
    };

    let mut method = None;
    for entry in &items {
        if let Meta::NameValue(pair) = entry {
            if pair.path.is_ident("method") {
                match expect_str(&pair.value) {
                    Ok(value) => method = Some(value),
                    Err(error) => return error.to_compile_error().into(),
                }
            }
        }
    }

    let Some(method) = method else {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[kynos::operation]` needs `method = \"...\"`; the per-method attributes such as \
             `#[kynos::get]` are the usual way to declare an operation",
        )
        .to_compile_error()
        .into();
    };

    let method_value = method.value();
    if !STANDARD_METHODS.contains(&method_value.as_str()) && !cfg!(feature = "openapi32") {
        return syn::Error::new(
            method.span(),
            format!(
                "`{method_value}` has no Path Item field in OpenAPI 3.1, so it can only be \
                 described through `additionalOperations`; enable the `openapi32` feature"
            ),
        )
        .to_compile_error()
        .into();
    }

    // `method` belongs to this attribute and to no other, so it is removed
    // before delegating rather than tolerated by the shared parser -- which
    // would silently accept `#[kynos::get("/x", method = "POST")]` and serve a
    // route the description does not match.
    let remaining: Punctuated<Meta, Token![,]> = items
        .into_iter()
        .filter(|entry| !matches!(entry, Meta::NameValue(pair) if pair.path.is_ident("method")))
        .collect();

    let args = match RouteArgs::parse(quote::quote!(#remaining)) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(&method_value, &args, &function).into()
}

#[cfg(test)]
mod tests;
