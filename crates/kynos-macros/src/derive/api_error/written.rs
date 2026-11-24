//! `#[problem(describe(status = "text", ...))]`: the descriptions an enum
//! writes for its statuses in place of the ones its failures' summaries
//! compose, and the checks that each names a status the enum answers with.
//!
//! The text reaches the response through `kynos::__private::problem::response`,
//! where a lone branch then keeps its summary as its title, and is written
//! again over a transparent variant's union by [`rewritten`].

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{DeriveInput, LitInt, LitStr, Token, meta::ParseNestedMeta, spanned::Spanned};

use super::{Transparent, args::Position};

/// One `describe(status = "text")` entry: the description written for
/// `status` in place of the one its failures' titles compose.
pub(super) struct Described {
    pub(super) status: u16,
    /// Where `status` is written, for a refusal naming it.
    pub(super) span: Span,
    pub(super) text: String,
}

/// Reads the `status = "text"` entries of an enum's `describe(...)` list,
/// refusing one on a variant, an empty list, a status written twice, and an
/// empty text.
pub(super) fn parse(meta: &ParseNestedMeta<'_>, position: Position) -> syn::Result<Vec<Described>> {
    if position == Position::Variant {
        return Err(syn::Error::new(
            meta.path.span(),
            "`describe` writes what the whole enum says of a status, whichever variants answer \
             with it, so it belongs on the enum rather than on one variant; a variant's own \
             summary is its `title`",
        ));
    }

    let list;
    syn::parenthesized!(list in meta.input);
    let entries = list.parse_terminated(
        |entry| {
            let status: LitInt = entry.parse()?;
            entry.parse::<Token![=]>()?;
            let text: LitStr = entry.parse()?;
            Ok((status, text))
        },
        Token![,],
    )?;
    if entries.is_empty() {
        return Err(syn::Error::new(
            meta.path.span(),
            "`describe(...)` names no status; write `describe(403 = \"...\")`",
        ));
    }

    let mut described: Vec<Described> = Vec::with_capacity(entries.len());
    for (literal, text) in entries {
        let status: u16 = literal.base10_parse()?;
        if described.iter().any(|seen| seen.status == status) {
            return Err(syn::Error::new(
                literal.span(),
                format!(
                    "`describe` already writes the description of {status}, and a response has \
                     one"
                ),
            ));
        }
        if text.value().trim().is_empty() {
            return Err(syn::Error::new(
                text.span(),
                format!(
                    "the description of {status} is empty; write what the status means, or drop \
                     the entry to keep the one its variants' titles compose"
                ),
            ));
        }
        described.push(Described {
            status,
            span: literal.span(),
            text: text.value(),
        });
    }

    Ok(described)
}

/// Checks that `describe` is on an enum, and that each status it writes a
/// description of is one the enum answers with, since a description of a
/// status nothing sends would describe nothing.
///
/// A status a variant declares is checked here. Any other is refused here
/// where no variant is transparent, and otherwise checked against `codes` in
/// a `const` the returned tokens declare, since only the held error knows its
/// statuses.
pub(super) fn answered(
    input: &DeriveInput,
    described: Option<&(Vec<Described>, Span)>,
    statuses: &[u16],
    delegates: &[&Transparent],
    codes: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let Some((described, span)) = described else {
        return Ok(TokenStream2::new());
    };
    if matches!(input.data, syn::Data::Struct(_)) {
        return Err(syn::Error::new(
            *span,
            "a struct fails one way, and its `title` is already what its status's description \
             says; `describe` writes one in place of an enum's variants' titles, so it belongs \
             on an enum",
        ));
    }

    let name = &input.ident;
    let mut checks = TokenStream2::new();
    for Described { status, span, .. } in described {
        if statuses.contains(status) {
            continue;
        }
        let refusal = format!(
            "no variant of `{name}` answers with {status}, so `describe` has no response of it to \
             write the description of"
        );
        if delegates.is_empty() {
            return Err(syn::Error::new(*span, refusal));
        }
        // Spanned to the status, so the evaluation failure points at it. The
        // status is answered where adding it leaves the union as long.
        checks.extend(quote_spanned! {*span=>
            const _: () = {
                const __KYNOS_PROBLEM_CODES: &[u16] = #codes;
                ::core::assert!(
                    ::kynos::__private::problem::union_len(&[__KYNOS_PROBLEM_CODES, &[#status]])
                        == ::kynos::__private::problem::union_len(&[__KYNOS_PROBLEM_CODES]),
                    #refusal,
                );
            };
        });
    }
    Ok(checks)
}

/// The statements writing each description over `responses` once the
/// transparent variants' responses have joined theirs to it, so it is what
/// the whole enum says of the status; none where no variant is transparent.
pub(super) fn rewritten(described: &[Described], delegates: &[&Transparent]) -> TokenStream2 {
    if delegates.is_empty() {
        return TokenStream2::new();
    }
    let rewrites = described.iter().map(|Described { status, text, .. }| {
        let key = status.to_string();
        quote! {
            if let ::core::option::Option::Some(::kynos::openapi::RefOr::Item(response)) =
                responses.responses.get_mut(#key)
            {
                response.description =
                    ::core::option::Option::Some(::std::string::String::from(#text));
            }
        }
    });
    quote!(#(#rewrites)*)
}
