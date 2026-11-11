//! Expansion of `routes![a, b.intercept(x), c]`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Expr, ExprMethodCall, Token, parse::Parser, punctuated::Punctuated, spanned::Spanned};

/// Expands `routes![a, b.intercept(x), c]`.
pub(crate) fn expand_routes(input: TokenStream) -> TokenStream {
    expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The expansion, over `proc_macro2` so it runs outside a macro invocation.
pub(crate) fn expand(input: TokenStream2) -> syn::Result<TokenStream2> {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let members = parser.parse2(input)?;

    if members.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`routes!` needs at least one operation",
        ));
    }

    // A tuple rather than `Endpoints`, so each member keeps the type its
    // interceptors are checked against at the mount site.
    let members = members
        .iter()
        .map(member)
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(nest(members))
}

/// The widest tuple `IntoEndpoints` is implemented for, in
/// `kynos::router::endpoint::set`.
const WIDEST_TUPLE: usize = 16;

/// `members` as one tuple, nested so no level is wider than [`WIDEST_TUPLE`].
///
/// A tuple of tuples is itself `IntoEndpoints`, and registers its members in
/// order, so the nesting changes no mounted route and no declaration order.
fn nest(members: Vec<TokenStream2>) -> TokenStream2 {
    if members.len() <= WIDEST_TUPLE {
        return quote! { ( #(#members,)* ) };
    }
    let mut members = members.into_iter().peekable();
    let mut chunks = Vec::new();
    while members.peek().is_some() {
        let chunk = members.by_ref().take(WIDEST_TUPLE);
        chunks.push(quote! { ( #(#chunk,)* ) });
    }
    nest(chunks)
}

/// One member: an operation, then the interceptors scoped to it alone.
///
/// The path names both the endpoint type and the handler function. The context
/// and arguments are inferred, so a missing dependency fails at the mount site.
fn member(expr: &Expr) -> syn::Result<TokenStream2> {
    let mut calls: Vec<&ExprMethodCall> = Vec::new();
    let mut receiver = expr;
    while let Expr::MethodCall(call) = receiver {
        calls.push(call);
        receiver = &call.receiver;
    }

    // syn attaches a member's leading attributes to its outermost call, or to
    // the path when there is none; none of them would reach the expansion.
    let attrs = calls
        .iter()
        .flat_map(|call| &call.attrs)
        .chain(match receiver {
            Expr::Path(path) => path.attrs.as_slice(),
            _ => &[],
        });
    if let Some(attr) = attrs.into_iter().next() {
        return Err(syn::Error::new(
            attr.span(),
            "a `routes!` member takes no attributes; gate the handler or the whole `routes!` call",
        ));
    }

    let path = match receiver {
        Expr::Path(path) if path.qself.is_none() => &path.path,
        other => {
            return Err(syn::Error::new(
                other.span(),
                "expected the name of a route-attributed handler, as in `routes![get_user]`",
            ));
        }
    };

    let mut built = quote! {
        ::kynos::__private::endpoint::from_meta::<_, #path, _, _>(#path)
    };

    // Collected outermost call first; applied in the order written, so the
    // first `intercept` is the outermost, as on `EndpointBuilder`.
    for call in calls.into_iter().rev() {
        // Everything else a builder sets is the attribute's to declare.
        if call.method != "intercept" {
            return Err(syn::Error::new(
                call.method.span(),
                "`routes!` accepts only `.intercept(..)` after an operation; name its other \
                 facts on the route attribute",
            ));
        }
        // The written identifier, so a collision is reported where it was named.
        let method = &call.method;
        let turbofish = &call.turbofish;
        let args = &call.args;
        built = quote! { #built.#method #turbofish (#args) };
    }

    Ok(built)
}
