//! `#[derive(ApiError)]`.
//!
//! ```text
//! #[problem( base = "<URI prefix>" )]              on the type, optional
//! #[problem( <member> [, <member>]* )]             on each variant, or on a struct
//! #[problem(extension)]                            on a named field, optional
//!
//! member := status = <400..=599>                   required, exactly once
//!         | title = "<human-readable summary>"
//!         | type = "<absolute URI>"
//! ```
//!
//! `status` is the only member read today: it becomes the `statuses()` const,
//! which is what the description is built from. The rest are parsed and checked
//! so the grammar is settled and a typo is still an error, and are read when
//! `into_problem` stops being a placeholder — the same treatment
//! `#[derive(SecurityScheme)]` gives the members its `describe` will need.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Fields, LitInt, LitStr, parse_macro_input, spanned::Spanned,
};

use crate::derive::common::skip_value;

/// The range a problem detail's status may fall in.
///
/// RFC 9457 defines the format for 4xx and 5xx; a problem describing a success
/// is a contradiction, and one describing a redirect has no consumer.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 400..=599;

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
            "`ApiError` cannot describe a union",
        ));
    }

    let statuses = statuses(input)?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // `detail` is the occurrence-specific half of a problem detail and comes
    // from `Display`, so a type without one would describe every occurrence
    // identically. Asserted here rather than bounded on the implementation so
    // the diagnostic lands on the error type instead of on the handler that
    // returns it.
    let display = quote! {
        const _: () = {
            #[allow(dead_code)]
            fn detail_comes_from_display #impl_generics () #where_clause {
                fn is_display<T: ::core::fmt::Display + ?Sized>() {}
                is_display::<#name #ty_generics>();
            }
        };
    };

    // `Responses` comes from the same declaration as `into_problem`, so a
    // status the error can return and a status the description advertises
    // cannot drift apart.
    Ok(quote! {
        #display

        impl #impl_generics ::kynos::error::problem::IntoProblem
            for #name #ty_generics #where_clause
        {
            fn into_problem(self) -> ::kynos::Problem {
                ::core::todo!()
            }

            fn statuses() -> &'static [::kynos::http::StatusCode] {
                // `StatusCode` has no const constructor, so the codes the
                // derive already validated are built once on first use rather
                // than on every call. This runs while the router is built, not
                // while a request is served.
                static STATUSES: ::std::sync::LazyLock<
                    ::std::vec::Vec<::kynos::http::StatusCode>
                > = ::std::sync::LazyLock::new(|| {
                    ::std::vec![
                        #(
                            ::kynos::http::StatusCode::from_u16(#statuses)
                                .expect("the derive checked this code")
                        ),*
                    ]
                });
                &STATUSES
            }
        }

        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                ::kynos::response::IntoResponse::into_response(
                    ::kynos::error::problem::IntoProblem::into_problem(self),
                )
            }
        }

        impl #impl_generics ::kynos::response::Responses for #name #ty_generics #where_clause {
            fn responses(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Responses {
                let _ = registry;
                ::core::todo!()
            }
        }
    })
}

/// Every status the type can produce, in declaration order and without repeats.
///
/// A repeated code is not an error — two variants may well be different 404s —
/// but the description carries one response per status, so the list is deduped
/// before it becomes one.
fn statuses(input: &DeriveInput) -> syn::Result<Vec<u16>> {
    let mut collected = Vec::new();

    match &input.data {
        Data::Enum(data) => {
            // A status on the enum itself would apply to every variant, which
            // is the opposite of what a closed set of failures is for.
            if let Some(status) = parse_problem(&input.attrs, Position::Type)? {
                return Err(syn::Error::new(
                    status.1,
                    "a status belongs on each variant, because the point of an `ApiError` enum is \
                     that its variants fail differently. Move it to the variants",
                ));
            }

            for variant in &data.variants {
                reject_unnamed_extensions(&variant.fields)?;

                let Some((status, _)) = parse_problem(&variant.attrs, Position::Variant)? else {
                    return Err(syn::Error::new(
                        variant.ident.span(),
                        format!(
                            "variant `{}` does not say what status it produces; add \
                             `#[problem(status = ...)]`",
                            variant.ident
                        ),
                    ));
                };
                collected.push(status);
            }
        }
        Data::Struct(data) => {
            reject_unnamed_extensions(&data.fields)?;

            let Some((status, _)) = parse_problem(&input.attrs, Position::Type)? else {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "this error does not say what status it produces; add \
                     `#[problem(status = ...)]`, or use an enum when it can fail several ways",
                ));
            };
            collected.push(status);
        }
        Data::Union(_) => unreachable!("rejected above"),
    }

    let mut seen = Vec::new();
    collected.retain(|status| {
        let fresh = !seen.contains(status);
        seen.push(*status);
        fresh
    });

    Ok(collected)
}

/// Where a `#[problem(...)]` list is written, which decides its legal members.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Type,
    Variant,
}

/// Reads one item's `#[problem(...)]` lists, validating every member.
///
/// Returns the status and its span when one was given. The other members are
/// checked for shape and discarded; see the module documentation.
fn parse_problem(attrs: &[Attribute], position: Position) -> syn::Result<Option<(u16, Span)>> {
    let mut status = None;

    for attr in attrs {
        if !attr.path().is_ident("problem") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return Ok(());
            };

            match key.to_string().as_str() {
                "status" => {
                    let literal: LitInt = meta.value()?.parse()?;
                    let code: u16 = literal.base10_parse()?;
                    if !STATUS_RANGE.contains(&code) {
                        return Err(syn::Error::new(
                            literal.span(),
                            format!(
                                "a problem detail describes a failure, so its status is between \
                                 {} and {}; `{code}` is not",
                                STATUS_RANGE.start(),
                                STATUS_RANGE.end()
                            ),
                        ));
                    }
                    if status.is_some() {
                        return Err(syn::Error::new(
                            literal.span(),
                            "this already declares a status, and a response has one",
                        ));
                    }
                    status = Some((code, literal.span()));
                }
                // Checked for shape now, read when `into_problem` lands.
                "title" | "type" => {
                    let _: LitStr = meta.value()?.parse()?;
                }
                "base" if position == Position::Type => {
                    let _: LitStr = meta.value()?.parse()?;
                }
                "base" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`base` is the prefix every type URI shares, so it belongs on the type \
                         rather than on one variant",
                    ));
                }
                "extension" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`extension` marks a field to publish, so it belongs on a field",
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("`{other}` is not part of the `#[problem(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(status)
}

/// `#[problem(extension)]` names a member by the field's own name, so a field
/// without one has nothing to be published as.
fn reject_unnamed_extensions(fields: &Fields) -> syn::Result<()> {
    let Fields::Unnamed(unnamed) = fields else {
        return Ok(());
    };

    for field in &unnamed.unnamed {
        for attr in &field.attrs {
            if !attr.path().is_ident("problem") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("extension") {
                    return Err(syn::Error::new(
                        attr.span(),
                        "an extension member is published under its field's name, and this field \
                         has none. Give the variant named fields",
                    ));
                }
                skip_value(&meta)
            })?;
        }
    }

    Ok(())
}
