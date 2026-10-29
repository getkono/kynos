//! `#[derive(Reply)]`.
//!
//! ```text
//! #[reply( <member> [, <member>]* )]            on each variant, required
//!
//! member := status = <200..=599>                required, exactly once
//!         | description = "<what it means>"     at most once
//!         | response                            at most once, not on a unit variant
//! ```
//!
//! No member goes on the enum, and any other key is refused.
//!
//! `status` is required on every variant, names a final response, and is
//! unique. A variant at 204, 205 or 304 is a unit variant, since those
//! responses carry no content. `description` falls back to the variant's doc comment, then the
//! reason phrase, then a sentence of the status's class.
//!
//! A bodied variant is described and written as `application/json`, so its
//! payload must be `serde::Serialize` (not `IntoResponse`, which `u32` or
//! `String` lack). A unit variant writes an empty body.
//!
//! A `response` variant holds a response type instead, `IntoResponse` and
//! `Responses` both, so `Created<Json<T>>` or `WithHeaders<Json<T>, H>` brings
//! its headers. It is described by that type's entry at `status`, else its 200
//! re-keyed, and the 4xx and 5xx it passes through are described under their
//! own statuses, joining the entry of any variant declaring one. A description
//! it carries stands unless the variant gives one.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{Attribute, Data, DeriveInput, Fields, LitStr, Variant, spanned::Spanned};

use crate::derive::attribute::{key_name, status};
use crate::derive::common::{doc_string, optional};

/// The range a reply's status may fall in: final responses only, never 1xx.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 200..=599;

/// The final statuses whose response carries no content, each with the RFC
/// 9110 section saying so.
const WITHOUT_CONTENT: [(u16, &str); 3] = [(204, "15.3.5"), (205, "15.3.6"), (304, "15.4.5")];

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // A closed set of responses needs an enum; one status is a status wrapper.
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

    nothing_on_the_enum(&input.attrs)?;

    let mut seen: Vec<(u16, Span)> = Vec::new();
    let mut declared: Vec<TokenStream2> = Vec::new();
    let mut written: Vec<TokenStream2> = Vec::new();
    let mut holds_a_response = false;
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

        // Unlike an `ApiError`, a reply's variants are keyed by status alone.
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

        // A body at a status without content would be described and written
        // anyway, so the variant answering with one is refused instead.
        if let (Fields::Unnamed(fields), Some((_, section))) = (
            &variant.fields,
            WITHOUT_CONTENT.iter().find(|(code, _)| *code == status),
        ) {
            return Err(syn::Error::new(
                fields.span(),
                format!(
                    "a `{status}` response carries no content (RFC 9110 §{section}), so variant \
                     `{}` cannot hold a body. Make it a unit variant, or answer with a status \
                     whose response has content",
                    variant.ident
                ),
            ));
        }

        let held = holds_a_response_type(variant, args.response)?;
        holds_a_response |= held;

        let description = args.description.map(|(text, _)| text);
        declared.push(response(variant, status, description.as_deref(), held));
        written.push(write(variant, status, held));
    }

    // Statuses a held response passes through join after every variant's
    // own, so a status both answer is described with what each sends.
    let (collect, join) = if holds_a_response {
        (
            quote!(let mut failures = ::kynos::openapi::Responses::new();),
            quote!(::kynos::__private::reply::join(&mut responses, &failures);),
        )
    } else {
        (TokenStream2::new(), TokenStream2::new())
    };

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
                #collect
                #(#declared)*
                #join
                responses
            }
        }
    })
}

/// Every member belongs on a variant, since each describes the one response
/// that variant answers with.
fn nothing_on_the_enum(attrs: &[Attribute]) -> syn::Result<()> {
    let on_enum = parse_reply(attrs)?;
    if let Some((_, span)) = on_enum.status {
        return Err(syn::Error::new(
            span,
            "a status belongs on each variant, because the point of a `Reply` enum is that its \
             variants answer differently. Move it to the variants",
        ));
    }
    if let Some((_, span)) = on_enum.description {
        return Err(syn::Error::new(
            span,
            "a description belongs on each variant, since each describes the one response it \
             answers with, and no response is described by the enum's. Move it to the variants",
        ));
    }
    if let Some(span) = on_enum.response {
        return Err(syn::Error::new(
            span,
            "`response` marks the variant holding a response type, so it belongs on that \
             variant rather than the enum",
        ));
    }
    Ok(())
}

/// Whether a variant marked `response` holds a type to answer with; a unit
/// variant holds none.
fn holds_a_response_type(variant: &Variant, response: Option<Span>) -> syn::Result<bool> {
    match (response, &variant.fields) {
        (Some(span), Fields::Unit) => Err(syn::Error::new(
            span,
            format!(
                "`response` says variant `{}` holds the response type it answers with, and a \
                 unit variant holds none. Give it the one field, or drop `response` for an \
                 empty body",
                variant.ident
            ),
        )),
        (response, _) => Ok(response.is_some()),
    }
}

/// One variant, as the response it declares: JSON content for a body, none for
/// a unit variant, and the held type's own entry for a `response` variant.
fn response(variant: &Variant, status: u16, description: Option<&str>, held: bool) -> TokenStream2 {
    let description = description
        .map(ToOwned::to_owned)
        .or_else(|| doc_string(&variant.attrs));
    let class = class_sentence(status);
    let fallback = quote! {
        ::kynos::http::StatusCode::from_u16(#status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or(#class)
    };

    if let (true, Fields::Unnamed(fields)) = (held, &variant.fields) {
        let ty = &fields.unnamed[0].ty;
        let description = optional(description);
        // Spanned on the held type, so one lacking `Responses` is reported
        // against the variant a user wrote.
        return quote_spanned! {ty.span()=>
            ::kynos::__private::reply::describe::<#ty>(
                registry,
                &mut responses,
                &mut failures,
                #status,
                #description,
                #fallback,
            );
        };
    }

    let description = description.map_or(fallback, |text| quote!(#text));

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

/// What a status with no reason phrase means, by its class (RFC 9110 §15).
fn class_sentence(status: u16) -> &'static str {
    match status {
        200..=299 => "the request succeeded",
        300..=399 => "further action is needed to complete the request",
        _ => "the request failed",
    }
}

/// One variant, as the match arm that writes it; the mirror of [`response`].
fn write(variant: &Variant, status: u16, held: bool) -> TokenStream2 {
    let ident = &variant.ident;

    match &variant.fields {
        Fields::Unnamed(fields) if held => {
            let span = fields.unnamed[0].ty.span();
            quote_spanned! {span=>
                Self::#ident(held) => ::kynos::__private::reply::held(#status, held),
            }
        }
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
    description: Option<(String, Span)>,
    response: Option<Span>,
}

/// Reads one item's `#[reply(...)]` lists, validating every member.
fn parse_reply(attrs: &[Attribute]) -> syn::Result<ReplyArgs> {
    let mut args = ReplyArgs::default();

    for attr in attrs {
        if !attr.path().is_ident("reply") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            match key.as_str() {
                "status" => status(
                    &meta,
                    STATUS_RANGE,
                    "a handler returns the final response",
                    &mut args.status,
                )?,
                "description" => {
                    let literal = meta.value()?.parse::<LitStr>()?;
                    if args.description.is_some() {
                        return Err(syn::Error::new(
                            literal.span(),
                            "this already declares a description, and a response has one",
                        ));
                    }
                    args.description = Some((literal.value(), literal.span()));
                }
                "response" => {
                    let span = meta.path.span();
                    if args.response.is_some() {
                        return Err(syn::Error::new(
                            span,
                            "this already says it holds a response, and saying it twice says \
                             nothing more",
                        ));
                    }
                    args.response = Some(span);
                }
                other => {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        format!("`{other}` is not part of the `#[reply(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(args)
}

/// A variant's fields are its response body, and a body is one described type
/// (or none, for a unit variant).
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
