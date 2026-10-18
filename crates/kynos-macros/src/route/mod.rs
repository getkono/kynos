//! Expansion of the route attributes and the `routes!` / `path!` macros.

pub(crate) mod args;
pub(crate) mod attrs;
pub(crate) mod emit;
pub(crate) mod path;
pub(crate) mod routes;
pub(crate) mod uri;

use kynos_openapi::Method;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{
    ItemFn, LitStr, Meta, Token, parse::Parser, parse_macro_input, punctuated::Punctuated,
    spanned::Spanned,
};

use crate::route::{
    args::{RouteArgs, expect_str, prepend_path_name},
    emit::emit,
};

/// Why `#[kynos::operation]` refuses `method`, or `None` when the router can
/// mount it.
///
/// Mountable means [`Method::from_wire_str`] returns it, since that is what
/// `routes!` builds the endpoint from: reading the same function keeps the
/// macro and the router from disagreeing, which is what let a method past this
/// check and into a panic at router construction.
fn unmountable(method: &str) -> Option<String> {
    // Checked before the model, whose own `openapi32` could be on through
    // another host dependency while this crate's, which `kynos` sets, is off.
    if method == "QUERY" && !cfg!(feature = "openapi32") {
        return Some(
            "`QUERY` has no Path Item field in OpenAPI 3.1; enable the `openapi32` feature"
                .to_owned(),
        );
    }
    if Method::from_wire_str(method).is_some() {
        return None;
    }
    let upper = method.to_ascii_uppercase();
    if Method::from_wire_str(&upper).is_some() {
        return Some(format!(
            "method names are case-sensitive, so `{method}` is not `{upper}`; write \
             `method = \"{upper}\"`"
        ));
    }
    let mountable = Method::all()
        .iter()
        .map(|method| format!("`{}`", method.as_wire_str()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "the router cannot mount `{method}`; `#[kynos::operation]` accepts {mountable}"
    ))
}

/// Expands a route attribute for a method with a dedicated Path Item field.
pub(crate) fn expand(method: &str, attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = match RouteArgs::parse(prepend_path_name(attribute.into())) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(method, &args, &function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Expands `#[kynos::operation(method = "...", path = "...")]`.
pub(crate) fn expand_generic(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let (method, remaining) = match split_method(attribute.into()) {
        Ok(split) => split,
        Err(error) => return error.to_compile_error().into(),
    };

    let args = match RouteArgs::parse(remaining) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(&method.value(), &args, &function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Splits `#[kynos::operation]`'s arguments into its `method` and the rest,
/// which the shared route parser reads, refusing a method the router cannot
/// mount.
///
/// `method` is removed here so the shared parser refuses it on
/// `#[kynos::get("/x", method = "POST")]`.
fn split_method(tokens: TokenStream2) -> syn::Result<(LitStr, TokenStream2)> {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let items = parser.parse2(tokens)?;

    let mut method = None;
    for entry in &items {
        if let Meta::NameValue(pair) = entry {
            if pair.path.is_ident("method") {
                if method.is_some() {
                    return Err(syn::Error::new(
                        pair.path.span(),
                        "`method` is already given, and an operation answers one method",
                    ));
                }
                method = Some(expect_str(&pair.value)?);
            }
        }
    }

    let Some(method) = method else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[kynos::operation]` needs `method = \"...\"`; the per-method attributes such as \
             `#[kynos::get]` are the usual way to declare an operation",
        ));
    };
    if let Some(refusal) = unmountable(&method.value()) {
        return Err(syn::Error::new(method.span(), refusal));
    }

    let remaining: Punctuated<Meta, Token![,]> = items
        .into_iter()
        .filter(|entry| !matches!(entry, Meta::NameValue(pair) if pair.path.is_ident("method")))
        .collect();
    Ok((method, quote::quote!(#remaining)))
}

#[cfg(test)]
mod tests;
