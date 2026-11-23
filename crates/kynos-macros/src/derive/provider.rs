//! `#[derive(Provider)]`.

mod forward;

use proc_macro2::Span;
use quote::{quote, quote_spanned};
use syn::{DeriveInput, Field, spanned::Spanned};

use crate::derive::{
    attribute::{Keys, compact, unknown},
    common::named_fields,
};
use forward::Forward;

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Provider")?;

    let mut provided: Vec<&Field> = Vec::new();
    // Each scheme with the field that authenticates it, in declaration order.
    let mut authenticators: Vec<(syn::Type, &Field)> = Vec::new();
    for field in &fields.named {
        let args = FieldArgs::read(field)?;
        if args.is_provided() {
            provided.push(field);
        }
        for scheme in args.authenticates {
            reject_second_authenticator(&authenticators, &scheme, field)?;
            authenticators.push((scheme, field));
        }
    }

    reject_duplicate_types(&provided)?;
    reject_type_parameter_fields(input, &provided)?;
    let forwards = Forward::read_all(input)?;

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

    // Spanned at the field's type, so a type that cannot authenticate the
    // scheme is reported where it is written.
    let authenticates = authenticators.iter().map(|(scheme, field)| {
        let ident = field.ident.as_ref().expect("named fields");
        let ty = &field.ty;
        let authenticator = quote_spanned!(ty.span()=> type Authenticator = #ty;);
        quote! {
            impl #impl_generics ::kynos::security::Authenticates<#scheme>
                for #name #ty_generics #where_clause
            {
                #authenticator

                fn authenticator(&self) -> &Self::Authenticator {
                    &self.#ident
                }
            }
        }
    });

    let forwarded = forwards
        .iter()
        .map(|forward| forward.expand(input, &provided, &authenticators));

    Ok(quote!(#(#implementations)* #(#authenticates)* #(#forwarded)*))
}

/// What a field's `#[provide(...)]` lists said.
///
/// `skip` and `provided` are bare and read once; `authenticates = Scheme`
/// repeats, once per scheme. A key outside the grammar is refused rather
/// than leaving the field provided.
struct FieldArgs {
    /// `skip`: the field is not handed out.
    skip: Option<Span>,
    /// `provided`: an authenticator that is handed out as well.
    provided: Option<Span>,
    /// The schemes this field authenticates, each `authenticates = Scheme`.
    authenticates: Vec<syn::Type>,
}

impl FieldArgs {
    fn read(field: &Field) -> syn::Result<Self> {
        let mut args = Self {
            skip: None,
            provided: None,
            authenticates: Vec::new(),
        };
        let mut keys = Keys::new("is already set, and `#[provide(...)]` takes each key once")
            .except(|key| key == "authenticates");
        for attr in &field.attrs {
            if !attr.path().is_ident("provide") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                let key = keys.read(&meta.path)?;
                match key.as_str() {
                    "skip" => args.skip = Some(meta.path.span()),
                    "provided" => args.provided = Some(meta.path.span()),
                    "authenticates" => args.authenticates.push(meta.value()?.parse()?),
                    _ => return Err(unknown(&meta.path, &key, "the `#[provide(...)]` grammar")),
                }
                Ok(())
            })?;
        }
        args.reject_inconsistent()?;
        Ok(args)
    }

    /// Each combination that says two things at once, refused so the field
    /// does what one reading of its attribute expects.
    fn reject_inconsistent(&self) -> syn::Result<()> {
        let authenticates = !self.authenticates.is_empty();
        match (self.skip, self.provided, authenticates) {
            (Some(_), Some(provided), _) => Err(syn::Error::new(
                provided,
                "`skip` and `provided` contradict each other: the field is either handed out or \
                 not. Write one",
            )),
            (Some(skip), None, true) => Err(syn::Error::new(
                skip,
                "`authenticates` already keeps the field from being handed out, so `skip` adds \
                 nothing. Drop it",
            )),
            (None, Some(provided), false) => Err(syn::Error::new(
                provided,
                "`provided` hands out a field `authenticates` would keep back, and this field \
                 authenticates no scheme, so it is handed out already. Drop it",
            )),
            _ => Ok(()),
        }
    }

    /// Whether the field gets a `Provides` implementation: unless skipped, or
    /// an authenticator not opted back in.
    fn is_provided(&self) -> bool {
        self.skip.is_none() && (self.authenticates.is_empty() || self.provided.is_some())
    }
}

/// A scheme has one authenticator; a second `authenticates` for it, on this
/// field or another, would emit two `Authenticates` implementations.
fn reject_second_authenticator(
    seen: &[(syn::Type, &Field)],
    scheme: &syn::Type,
    field: &Field,
) -> syn::Result<()> {
    let rendered = compact(scheme);
    let Some((_, first)) = seen.iter().find(|(seen, _)| compact(seen) == rendered) else {
        return Ok(());
    };
    let named = |field: &Field| {
        field
            .ident
            .as_ref()
            .map_or_else(String::new, ToString::to_string)
    };
    let (first, second) = (named(first), named(field));
    if first == second {
        return Err(syn::Error::new(
            scheme.span(),
            format!("`{second}` already authenticates `{rendered}`; name each scheme once"),
        ));
    }
    Err(syn::Error::new(
        scheme.span(),
        format!(
            "`{first}` and `{second}` both authenticate `{rendered}`, and a scheme has one \
             authenticator. Keep one"
        ),
    ))
}

/// Two fields of the same type would emit the same implementation twice;
/// refused here so the error names both fields.
fn reject_duplicate_types(provided: &[&Field]) -> syn::Result<()> {
    for (index, field) in provided.iter().enumerate() {
        // Compared as written; differing spellings of one type fall through
        // to coherence.
        let rendered = compact(&field.ty);
        if let Some(earlier) = provided[..index]
            .iter()
            .position(|seen| compact(&seen.ty) == rendered)
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
        let rendered = compact(&field.ty);
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
