//! `#[problem(subset_of = Wide)]`: an enum whose variants are some of a wider
//! `ApiError` enum's, answering and described as that enum's do.
//!
//! A derive sees one item, so the subset cannot read the wider enum's
//! declarations. Every derived enum therefore publishes them as hidden
//! inherent items — its [`handles`] — which a subset names through the type it
//! was given: one `const` per variant holding the statuses it answers with,
//! and a function describing only the variants a caller keeps. The subset
//! generates `From<Self> for Wide` by moving each variant's fields into the
//! wider variant of the same name, so a variant or a field the wider enum
//! lacks, or one it has and the subset lacks, is a compile error there; then
//! it answers by converting, and declares the union of its variants'
//! statuses. A subset publishes handles too, so a subset of a subset holds.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{
    Data, DeriveInput, Fields, Ident, PathArguments, Token, Type, TypePath, spanned::Spanned,
};

use super::{args::ProblemArgs, emit::union, impls, outcomes::generic_named_in};

/// The function a derived enum describes the variants a caller keeps with.
const RESPONSES: &str = "__kynos_problem_responses";

/// The `const` holding the statuses `variant` answers with, spanned to the
/// variant, so a subset naming one the wider enum lacks is told so there.
fn statuses_of(variant: &Ident) -> Ident {
    format_ident!(
        "__KYNOS_PROBLEM_STATUSES_{}",
        variant,
        span = variant.span()
    )
}

/// The hidden inherent items a subset reads: each variant's statuses, and the
/// responses function over `body`, which reads `registry` and `keep`.
pub(super) fn handles(
    input: &DeriveInput,
    variants: impl Iterator<Item = (Ident, TokenStream2)>,
    body: &TokenStream2,
) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let consts = variants.map(|(variant, statuses)| {
        let item = statuses_of(&variant);
        quote! {
            #[doc(hidden)]
            pub const #item: &'static [u16] = #statuses;
        }
    });
    let responses = Ident::new(RESPONSES, Span::call_site());

    quote! {
        #[automatically_derived]
        #[allow(dead_code, non_upper_case_globals, unreachable_pub)]
        impl #impl_generics #name #ty_generics #where_clause {
            #(#consts)*

            #[doc(hidden)]
            pub fn #responses(
                registry: &mut ::kynos::schema::registry::Registry,
                keep: ::core::option::Option<&[&str]>,
            ) -> ::kynos::openapi::Responses {
                #body
            }
        }
    }
}

/// The expansion of an enum declaring `subset_of = wide`.
pub(super) fn expand(
    input: &DeriveInput,
    args: &ProblemArgs,
    wide: &Type,
    span: Span,
) -> syn::Result<TokenStream2> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            span,
            "`subset_of` takes some of a wider enum's variants, so it belongs on an enum",
        ));
    };

    if args.status.is_some()
        || args.title.is_some()
        || args.type_uri.is_some()
        || args.base.is_some()
    {
        return Err(syn::Error::new(
            span,
            "a subset answers with the wider enum's declarations, so `subset_of` takes no \
             `status`, `title`, `type` or `base` of its own",
        ));
    }

    if let Some(param) = generic_named_in(wide, &input.generics) {
        return Err(syn::Error::new(
            wide.span(),
            format!(
                "a subset's statuses are a `const`, which cannot depend on the type's generic \
                 parameter `{param}`; name a concrete wider error"
            ),
        ));
    }

    // Every `#[problem(...)]` below the enum would say what the wider
    // variant already says.
    for variant in &data.variants {
        let fields = variant.fields.iter().flat_map(|field| &field.attrs);
        if let Some(attr) = variant
            .attrs
            .iter()
            .chain(fields)
            .find(|attr| attr.path().is_ident("problem"))
        {
            return Err(syn::Error::new(
                attr.span(),
                format!(
                    "a subset's variant answers as the wider enum's variant `{}`, whose status, \
                     title, type and field markers it takes; drop this `#[problem(...)]`",
                    variant.ident
                ),
            ));
        }
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let idents: Vec<&Ident> = data.variants.iter().map(|variant| &variant.ident).collect();
    let consts: Vec<Ident> = idents.iter().map(|variant| statuses_of(variant)).collect();
    let responses = Ident::new(RESPONSES, Span::call_site());

    let from = conversion(input, data.variants.iter(), wide);

    let names = idents.iter().map(ToString::to_string);
    let body = quote! {
        const __KYNOS_PROBLEM_VARIANTS: &[&str] = &[#(#names),*];
        let kept: ::std::vec::Vec<&str> = ::core::iter::Iterator::collect(
            ::core::iter::Iterator::filter(
                ::core::iter::Iterator::copied(__KYNOS_PROBLEM_VARIANTS.iter()),
                |variant| keep.is_none_or(|kept| kept.contains(variant)),
            ),
        );
        <#wide>::#responses(registry, ::core::option::Option::Some(&kept))
    };
    let handles = handles(
        input,
        idents
            .iter()
            .zip(&consts)
            .map(|(variant, item)| ((*variant).clone(), quote!(<#wide>::#item))),
        &body,
    );

    let codes = union(consts.iter().map(|item| quote!(<#wide>::#item)));
    let convert = quote!(<#wide as ::core::convert::From<Self>>::from(self));
    let impls = impls(
        input,
        &quote!(::kynos::error::problem::IntoProblem::into_problem(#convert)),
        &quote!(::kynos::response::IntoResponse::into_response(#convert)),
        &quote!(Self::#responses(registry, ::core::option::Option::None)),
        &codes,
    );

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::core::convert::From<#name #ty_generics> for #wide #where_clause {
            fn from(subset: #name #ty_generics) -> Self {
                #from
            }
        }

        #handles
        #impls
    })
}

/// The `from` body: each variant's fields moved into the wider variant of
/// its name, so the expression building the wider variant checks every
/// name, field and type.
fn conversion<'a>(
    input: &DeriveInput,
    variants: impl Iterator<Item = &'a syn::Variant>,
    wide: &Type,
) -> TokenStream2 {
    let name = &input.ident;
    let wide = expression_path(wide);
    let arms = variants.map(|variant| {
        let ident = &variant.ident;
        // Spanned to each field's type, so one of another type is reported
        // there.
        let bindings: Vec<Ident> = variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| format_ident!("__kynos_field_{index}", span = field.ty.span()))
            .collect();
        // In the variant's own form, the pattern and the expression alike; a
        // unit variant braced, so a wider variant with fields is told it is
        // missing them.
        let fields = match &variant.fields {
            Fields::Named(named) => {
                let members = named.named.iter().map(|field| &field.ident);
                quote!({ #(#members: #bindings),* })
            }
            Fields::Unnamed(_) => quote!((#(#bindings),*)),
            Fields::Unit => quote!({}),
        };
        // Spanned to the variant, so a variant or a field the wider enum has
        // and this one lacks is reported there.
        let into = quote_spanned!(ident.span()=> #wide::#ident #fields);
        quote! {
            #name::#ident #fields => #into,
        }
    });

    quote! {
        match subset {
            #(#arms)*
        }
    }
}

/// `wide` as the path a struct expression names its variants through, its
/// generic arguments in turbofish form, so a variant it lacks is reported as
/// one; `Self`, the wider enum in the conversion, where it is not a plain
/// path, since a struct expression cannot name a variant through a qualified
/// one.
fn expression_path(wide: &Type) -> TokenStream2 {
    let Type::Path(TypePath { qself: None, path }) = wide else {
        return quote!(Self);
    };
    let mut path = path.clone();
    for segment in &mut path.segments {
        if let PathArguments::AngleBracketed(arguments) = &mut segment.arguments {
            arguments.colon2_token = Some(Token![::](arguments.lt_token.span));
        }
    }
    path.to_token_stream()
}
