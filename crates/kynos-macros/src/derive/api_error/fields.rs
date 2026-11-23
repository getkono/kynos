//! The fields a failure sends as response fields rather than in its problem:
//! its `#[problem(headers)]` group and its `#[problem(retry_after)]` delay,
//! read off its markers and bound for `into_response`.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use syn::{Fields, Ident, Index, Member, Type};

use super::{Failure, args::Markers};

/// A `#[problem(headers)]` or `#[problem(retry_after)]` field: where it sits,
/// and its type.
pub(super) struct HeaderField {
    pub(super) member: Member,
    pub(super) ty: Type,
}

/// What `into_response` binds a failure's header group to.
pub(super) const HEADERS_BINDING: &str = "__kynos_problem_headers";

/// What `into_response` binds a failure's delay to.
pub(super) const RETRY_AFTER_BINDING: &str = "__kynos_problem_retry_after";

impl Failure {
    /// Whether the failure sends a response field, from a group or a delay.
    pub(super) fn sends_headers(&self) -> bool {
        self.headers.is_some() || self.retry_after.is_some()
    }

    /// [`pattern`](Self::pattern), binding the header group and the delay too.
    pub(super) fn response_pattern(&self) -> TokenStream2 {
        let path = &self.path;
        let bindings = self.extensions.iter().map(|extension| &extension.name);
        let bind = |field: Option<&HeaderField>, binding: &str| {
            field.map(|field| {
                let member = &field.member;
                let binding = Ident::new(binding, Span::call_site());
                quote!(#member: #binding,)
            })
        };
        let headers = bind(self.headers.as_deref(), HEADERS_BINDING);
        let retry_after = bind(self.retry_after.as_deref(), RETRY_AFTER_BINDING);
        quote!(#path { #(#bindings,)* #headers #retry_after .. })
    }
}

/// The one field marked `#[problem(headers)]`, by its `markers`, refusing a
/// second and one also published as an extension member.
pub(super) fn header_field(
    fields: &Fields,
    markers: &[Markers],
) -> syn::Result<Option<Box<HeaderField>>> {
    let mut found: Option<(HeaderField, String)> = None;

    for (index, (field, marked)) in fields.iter().zip(markers).enumerate() {
        let Some(span) = marked.headers else {
            continue;
        };

        if marked.extension.is_some() {
            return Err(syn::Error::new(
                span,
                "a header group is sent as response fields, not published in the body. Mark this \
                 field `extension` or `headers`, not both",
            ));
        }

        let member = field
            .ident
            .clone()
            .map_or_else(|| Member::Unnamed(Index::from(index)), Member::Named);
        if let Some((_, first)) = &found {
            return Err(syn::Error::new(
                span,
                format!(
                    "this failure already sends the header group in `{first}`; one group holds \
                     every header it sends"
                ),
            ));
        }

        let name = member.to_token_stream().to_string();
        found = Some((
            HeaderField {
                member,
                ty: field.ty.clone(),
            },
            name,
        ));
    }

    Ok(found.map(|(field, _)| Box::new(field)))
}

/// The one field marked `#[problem(retry_after)]`, by its `markers`, refusing
/// a second and one also marked otherwise.
pub(super) fn retry_after_field(
    fields: &Fields,
    markers: &[Markers],
) -> syn::Result<Option<Box<HeaderField>>> {
    let mut found: Option<(HeaderField, String)> = None;

    for (index, (field, marked)) in fields.iter().zip(markers).enumerate() {
        let Some(span) = marked.retry_after else {
            continue;
        };

        if marked.extension.is_some() || marked.headers.is_some() {
            return Err(syn::Error::new(
                span,
                "a `retry_after` field holds the delay `Retry-After` sends, so it is neither an \
                 extension member nor a header group. Mark it `retry_after` alone",
            ));
        }

        let member = field
            .ident
            .clone()
            .map_or_else(|| Member::Unnamed(Index::from(index)), Member::Named);
        if let Some((_, first)) = &found {
            return Err(syn::Error::new(
                span,
                format!(
                    "this failure already sends `Retry-After` from `{first}`; one field holds the \
                     delay"
                ),
            ));
        }

        let name = member.to_token_stream().to_string();
        found = Some((
            HeaderField {
                member,
                ty: field.ty.clone(),
            },
            name,
        ));
    }

    Ok(found.map(|(field, _)| Box::new(field)))
}
