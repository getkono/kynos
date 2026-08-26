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
//! variants may share one. `description` is what the response says it means,
//! falling back to the variant's own doc comment and then to the status code's
//! reason phrase, so a variant is described without being described twice.
//!
//! # What carries a variant's body onto the wire
//!
//! `serde::Serialize`, required of a bodied variant's payload by the emitted
//! `into_response` rather than by a bound anyone writes.
//!
//! It follows from the description this derive already emits: a bodied variant
//! is described as `application/json`, and a payload that cannot be serialized
//! as JSON would make that description a claim the handler cannot honour. The
//! alternative bound, `IntoResponse`, is the wrong one — Kynos deliberately has
//! no `IntoResponse for u32` or `for String`, and a reply variant legitimately
//! holds either.
//!
//! A unit variant writes its status and an empty body, and needs no bound at
//! all.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, Data, DeriveInput, Fields, LitInt, LitStr, Variant, parse_macro_input,
    spanned::Spanned,
};

use crate::derive::common::doc_string;

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

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
    if let Some((_, span)) = parse_reply(&input.attrs)?.status {
        return Err(syn::Error::new(
            span,
            "a status belongs on each variant, because the point of a `Reply` enum is that its \
             variants answer differently. Move it to the variants",
        ));
    }

    let mut seen: Vec<(u16, Span)> = Vec::new();
    let mut declared: Vec<TokenStream2> = Vec::new();
    let mut written: Vec<TokenStream2> = Vec::new();
    for variant in &data.variants {
        body_is_one_named_type(variant)?;

        let args = parse_reply(&variant.attrs)?;
        let Some((status, span)) = args.status else {
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
        declared.push(response(variant, status, args.description.as_deref()));
        written.push(write(variant, status));
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                match self {
                    #(#written)*
                }
            }
        }

        impl #impl_generics ::kynos::response::Responses for #name #ty_generics #where_clause {
            fn responses(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Responses {
                let mut responses = ::kynos::openapi::Responses::new();
                #(#declared)*
                responses
            }
        }
    })
}

/// One variant, as the response it declares.
///
/// A variant carrying a body describes it as JSON, which is the representation
/// a described type reaches a consumer as unless a body wrapper says otherwise;
/// a unit variant describes a response with no content at all.
fn response(variant: &Variant, status: u16, description: Option<&str>) -> TokenStream2 {
    let description = description
        .map(ToOwned::to_owned)
        .or_else(|| doc_string(&variant.attrs));
    let description = description.map_or_else(
        || {
            quote! {
                ::kynos::http::StatusCode::from_u16(#status)
                    .ok()
                    .and_then(|status| status.canonical_reason())
                    .unwrap_or("the request succeeded")
            }
        },
        |text| quote!(#text),
    );

    let body = match &variant.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed[0].ty;
            Some(quote!(registry.resolve::<#ty>()))
        }
        _ => None,
    };

    let built = body.map_or_else(
        || quote!(::kynos::openapi::Response::new(#description)),
        |schema| {
            quote! {
                ::kynos::openapi::Response::with_content(
                    #description,
                    ::kynos::openapi::model::body::mime_names::APPLICATION_JSON,
                    ::kynos::openapi::MediaType::new(#schema),
                )
            }
        },
    );

    quote!(responses = responses.with(#status, #built);)
}

/// One variant, as the match arm that writes it.
///
/// The mirror of [`response`]: a bodied variant writes the `application/json`
/// that function described it as, and a unit variant the empty body. Both carry
/// the status the variant declared, so what a consumer receives and what the
/// description promised come from one attribute.
fn write(variant: &Variant, status: u16) -> TokenStream2 {
    let ident = &variant.ident;

    match &variant.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            // Spanned on the payload's type, so a payload without `Serialize`
            // is reported against the variant a user wrote.
            let span = fields.unnamed[0].ty.span();
            quote_spanned! {span=>
                Self::#ident(body) => ::kynos::__private::reply::json(#status, &body),
            }
        }
        _ => quote! {
            Self::#ident => ::kynos::__private::reply::empty(#status),
        },
    }
}

/// What one item's `#[reply(...)]` lists said.
#[derive(Default)]
struct ReplyArgs {
    status: Option<(u16, Span)>,
    description: Option<String>,
}

/// Reads one item's `#[reply(...)]` lists, validating every member.
fn parse_reply(attrs: &[Attribute]) -> syn::Result<ReplyArgs> {
    let mut args = ReplyArgs::default();

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
                    if args.status.is_some() {
                        return Err(syn::Error::new(
                            literal.span(),
                            "this already declares a status, and a response has one",
                        ));
                    }
                    args.status = Some((code, literal.span()));
                }
                "description" => {
                    args.description = Some(meta.value()?.parse::<LitStr>()?.value());
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

    Ok(args)
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
