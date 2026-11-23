//! `#[provider(for = Outer, via = path)]`: a provider struct's
//! implementations, emitted again for the context holding it.

use proc_macro2::{TokenStream, TokenTree};
use quote::{quote, quote_spanned};
use syn::{DeriveInput, Expr, Field, Token, spanned::Spanned};

use crate::derive::attribute::{Keys, compact, unknown};

/// One `#[provider(for = Outer, via = path)]`: this struct's implementations,
/// emitted again for the context `Outer`, reaching this struct at
/// `self.path`.
///
/// It is written on the inner struct because only there are the field types
/// visible; a blanket forward on `Outer` would overlap the reflexive
/// `Provides<T> for T` and `Outer`'s own fields.
pub(super) struct Forward {
    /// The context the implementations are emitted for.
    target: syn::Type,
    /// `self.path`, the field path from `target` to this struct.
    via: Expr,
}

impl Forward {
    /// Every `#[provider(...)]` on the struct, one forward each.
    pub(super) fn read_all(input: &DeriveInput) -> syn::Result<Vec<Self>> {
        let mut forwards: Vec<Self> = Vec::new();
        for attr in &input.attrs {
            if !attr.path().is_ident("provider") {
                continue;
            }
            let forward = Self::read(attr)?;
            let rendered = compact(&forward.target);
            if forwards
                .iter()
                .any(|seen| compact(&seen.target) == rendered)
            {
                return Err(syn::Error::new(
                    forward.target.span(),
                    format!(
                        "this struct already forwards to `{rendered}`, and a second forward \
                         would provide every type twice. Keep one"
                    ),
                ));
            }
            forwards.push(forward);
        }
        Ok(forwards)
    }

    /// One attribute, which names its target and its path once each.
    fn read(attr: &syn::Attribute) -> syn::Result<Self> {
        let mut target: Option<syn::Type> = None;
        let mut via: Option<Expr> = None;
        let mut keys = Keys::new(
            "is already set, and each `#[provider(...)]` forwards to one context, so it takes \
             each key once",
        );
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            match key.as_str() {
                "for" => target = Some(meta.value()?.parse()?),
                "via" => via = Some(field_path(meta.value()?)?),
                _ => return Err(unknown(&meta.path, &key, "the `#[provider(...)]` grammar")),
            }
            Ok(())
        })?;
        match (target, via) {
            (Some(target), Some(via)) => Ok(Self { target, via }),
            (Some(target), None) => Err(syn::Error::new(
                attr.path().span(),
                format!(
                    "forwarding to `{}` needs `via = path`, the fields leading from it to this \
                     struct, such as `via = inner.services`",
                    compact(&target)
                ),
            )),
            (None, _) => Err(syn::Error::new(
                attr.path().span(),
                "`#[provider(...)]` forwards this struct's types to the context holding it, so \
                 it needs `for = Context`, the context to provide them on",
            )),
        }
    }

    /// This struct's `Provides` and `Authenticates` implementations, each
    /// forwarded through its own implementation, so the two cannot disagree.
    ///
    /// Each is spanned at the field it forwards, so a type the target already
    /// provides is reported as a conflict between the target's source and
    /// this field.
    pub(super) fn expand(
        &self,
        input: &DeriveInput,
        provided: &[&Field],
        authenticators: &[(syn::Type, &Field)],
    ) -> TokenStream {
        let Self { target, via } = self;
        let name = &input.ident;
        let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
        let inner = quote_spanned!(via.span()=> &#via);
        // The field the conflicting implementation forwards, by name.
        let at = |field: &Field| field.ident.as_ref().expect("named fields").span();

        let provides = provided.iter().map(|field| {
            let ty = &field.ty;
            quote_spanned! {at(field)=>
                impl #impl_generics ::kynos::di::Provides<#ty> for #target #where_clause {
                    fn provide(&self) -> #ty {
                        <#name #ty_generics as ::kynos::di::Provides<#ty>>::provide(#inner)
                    }
                }
            }
        });

        let authenticates = authenticators.iter().map(|(scheme, field)| {
            let ty = &field.ty;
            quote_spanned! {at(field)=>
                impl #impl_generics ::kynos::security::Authenticates<#scheme>
                    for #target #where_clause
                {
                    type Authenticator = #ty;

                    fn authenticator(&self) -> &Self::Authenticator {
                        <#name #ty_generics as ::kynos::security::Authenticates<#scheme>>
                            ::authenticator(#inner)
                    }
                }
            }
        });

        quote!(#(#provides)* #(#authenticates)*)
    }
}

/// Reads `via`'s value, a field path such as `inner.services` or `0.services`,
/// up to the next key, into the expression `self.inner.services`.
///
/// Written without `self`, as the `for` context's fields; a method call, an
/// index, or anything else that is not a field is refused, since a provider
/// is expected to be a clone of a handle and nothing more.
fn field_path(input: syn::parse::ParseStream<'_>) -> syn::Result<Expr> {
    if input.peek(Token![self]) {
        return Err(syn::Error::new(
            input.span(),
            "`via` starts at the `for` context's own fields, so it is written without `self`. \
             Drop `self.`",
        ));
    }
    let span = input.span();
    let mut tokens = TokenStream::new();
    while !input.is_empty() && !input.peek(Token![,]) {
        tokens.extend([input.parse::<TokenTree>()?]);
    }
    // `self` spanned at the path, so a path reaching some other type is
    // reported where it is written.
    let receiver = quote_spanned!(span=> self);
    syn::parse2::<Expr>(quote!(#receiver.#tokens))
        .ok()
        .filter(is_field_path)
        .ok_or_else(|| {
            syn::Error::new(
                if tokens.is_empty() {
                    span
                } else {
                    tokens.span()
                },
                format!(
                    "`via` is the fields leading from the `for` context to this struct, such as \
                     `inner.services`, and `{}` is not a field path",
                    compact(&tokens)
                ),
            )
        })
}

/// Whether `expr` is `self` followed by one field access or more.
fn is_field_path(expr: &Expr) -> bool {
    let Expr::Field(field) = expr else {
        return false;
    };
    match &*field.base {
        Expr::Path(path) => {
            path.attrs.is_empty() && path.qself.is_none() && path.path.is_ident("self")
        }
        base => is_field_path(base),
    }
}
