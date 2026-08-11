//! `#[derive(Reply)]`.
//!
//! ```text
//! #[reply( <member> [, <member>]* )]            on each variant, required
//!
//! member := status = <200..=599>                required, exactly once
//!         | description = "<what it means>"
//! ```
//!
//! `status` is what makes the set closed, and it is checked three ways: it must
//! be present on every variant, it must name a final response, and no two
//! variants may share one. `description` is parsed and checked so the grammar
//! is settled and a typo is still an error; it is read when `responses` stops
//! being a placeholder, falling back to the variant's own doc comment — the
//! same treatment `#[derive(ApiError)]` gives the members its `into_problem`
//! will need.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Fields, LitInt, LitStr, parse_macro_input, spanned::Spanned,
};

/// The range a reply's status may fall in.
///
/// A 1xx is an interim response that precedes the final one, so a handler that
/// returns a `Reply` never produces it. 6xx and above are not status codes at
/// all.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 200..=599;

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // The point of the derive is a *closed set* of responses, one variant per
    // status. A struct has one shape and therefore one status, which the
    // status types in `response::status` already express.
    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(data) => {
            return Err(syn::Error::new(
                data.struct_token.span(),
                "`Reply` declares a closed set of responses, one variant per status, so it needs \
                 an enum. A single response is already expressible: return the body type, or wrap \
                 it in `Created`, `Accepted` or `NoContent`",
            ));
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`Reply` needs an enum",
            ));
        }
    };

    // A status on the enum itself would apply to every variant, which is the
    // opposite of what a closed set of responses is for.
    if let Some((_, span)) = parse_reply(&input.attrs)? {
        return Err(syn::Error::new(
            span,
            "a status belongs on each variant, because the point of a `Reply` enum is that its \
             variants answer differently. Move it to the variants",
        ));
    }

    let mut seen: Vec<(u16, Span)> = Vec::new();
    for variant in &data.variants {
        body_is_one_named_type(variant)?;

        let Some((status, span)) = parse_reply(&variant.attrs)? else {
            return Err(syn::Error::new(
                variant.ident.span(),
                format!(
                    "variant `{}` does not say what status it produces; add \
                     `#[reply(status = ...)]`",
                    variant.ident
                ),
            ));
        };

        // Unlike an `ApiError`, whose variants carry a `detail` that tells two
        // occurrences of one status apart, a reply's variants are keyed by
        // status alone: two under the same code are two bodies the description
        // would have to file under one key.
        if seen.iter().any(|(earlier, _)| *earlier == status) {
            return Err(syn::Error::new(
                span,
                format!(
                    "another variant already answers with `{status}`, and a `Reply` carries one \
                     variant per status. Give this one its own status, or fold the two bodies into \
                     a single type"
                ),
            ));
        }
        seen.push((status, span));
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                ::core::todo!()
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

/// Reads one item's `#[reply(...)]` lists, validating every member.
///
/// Returns the status and its span when one was given. `description` is
/// checked for shape and discarded; see the module documentation.
fn parse_reply(attrs: &[Attribute]) -> syn::Result<Option<(u16, Span)>> {
    let mut status = None;

    for attr in attrs {
        if !attr.path().is_ident("reply") {
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
                                "a handler returns the final response, so its status is between {} \
                                 and {}; `{code}` is not",
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
                // Checked for shape now, read when `responses` lands.
                "description" => {
                    let _: LitStr = meta.value()?.parse()?;
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("`{other}` is not part of the `#[reply(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(status)
}

/// A variant's fields are its response body, and a body is one described type.
///
/// An anonymous record has no name to register a component under and no
/// `Schema` implementation to build one from, so the remedy is to give it a
/// name. A unit variant is the empty body and needs no type at all.
fn body_is_one_named_type(variant: &syn::Variant) -> syn::Result<()> {
    match &variant.fields {
        Fields::Unit => Ok(()),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(()),
        fields => Err(syn::Error::new(
            fields.span(),
            format!(
                "variant `{}` carries its response body, and a body is one described type. Give \
                 these fields a struct deriving `Schema` and hold it in a single-field variant, or \
                 drop them for an empty body",
                variant.ident
            ),
        )),
    }
}
