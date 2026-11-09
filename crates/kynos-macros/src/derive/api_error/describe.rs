//! The `Responses` body, from the failures read: what the description says
//! each status may carry, apart from what the wire carries.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::{Ident, spanned::Spanned};

use super::{Extension, Failure, Transparent};
use crate::derive::common::optional;

/// The `responses` body: one response per distinct status, over the `Problem`
/// component, shaped by `kynos::__private::problem::response`, then each
/// transparent variant's responses in declaration order, a status met twice
/// unioned rather than replaced.
///
/// Where `filtered`, the body reads `keep`, an `Option<&[&str]>` naming the
/// variants it describes, `None` naming every one, so a `subset_of` enum is
/// described by this enum's own declarations of the variants it holds. A
/// status no kept failure answers with is left out, and so is the `Problem`
/// component until one does.
pub(super) fn responses(
    failures: &[&Failure],
    statuses: &[u16],
    delegates: &[&Transparent],
    filtered: bool,
) -> TokenStream2 {
    // Whether the variant is described, as an expression of the body.
    let kept = |variant: Option<&Ident>| match variant {
        Some(variant) if filtered => {
            let name = variant.to_string();
            quote!(__kynos_keep(#name))
        }
        _ => quote!(true),
    };

    let entries = statuses.iter().map(|status| {
        // Every failure declaring this status, in declaration order.
        let declaring: Vec<&&Failure> = failures
            .iter()
            .filter(|failure| failure.status == *status)
            .collect();
        let kept: Vec<TokenStream2> = declaring
            .iter()
            .map(|failure| kept(failure.variant.as_ref()))
            .collect();
        entry(*status, &declaring, &kept)
    });

    let handed_over = delegates.iter().map(|delegate| {
        let ty = &delegate.ty;
        let kept = kept(Some(&delegate.variant));
        // As for a headers group: the type's path is spanned, the bindings
        // are not.
        let inner = quote_spanned!(ty.span()=> <#ty as ::kynos::response::Responses>::responses);
        quote! {
            if #kept {
                ::kynos::__private::problem::union_responses(&mut responses, &#inner(registry));
            }
        }
    });

    let keep = filtered.then(|| {
        quote! {
            let __kynos_keep = |variant: &str| keep.is_none_or(|kept| kept.contains(&variant));
        }
    });
    // Resolved only once a described status reads it.
    let schema = if !statuses.is_empty() {
        quote!(let mut __kynos_problem = ::core::option::Option::None;)
    } else if delegates.is_empty() {
        quote!(let _ = registry;)
    } else {
        TokenStream2::new()
    };

    quote! {
        #keep
        #schema
        let mut responses = ::kynos::openapi::Responses::new();
        #(#entries)*
        #(#handed_over)*
        responses
    }
}

/// The statement describing `status`, answered by `declaring`, each described
/// where its expression in `kept` holds, and the status only where one does.
fn entry(status: u16, declaring: &[&&Failure], kept: &[TokenStream2]) -> TokenStream2 {
    // Each kept failure's members are resolved into a list of their own,
    // which its branch borrows.
    let lists = declaring
        .iter()
        .zip(kept)
        .enumerate()
        .filter_map(|(index, (failure, kept))| {
            if failure.extensions.is_empty() {
                return None;
            }
            let list = format_ident!("__kynos_members_{index}");
            let members = failure.extensions.iter().map(member);
            Some(quote! {
                let #list: ::std::vec::Vec<(&'static str, ::kynos::openapi::Schema, bool)> =
                    if #kept {
                        ::std::vec![#(#members),*]
                    } else {
                        ::std::vec::Vec::new()
                    };
            })
        });
    let branches = declaring
        .iter()
        .zip(kept)
        .enumerate()
        .map(|(index, (failure, kept))| {
            let uri = optional(failure.type_uri.as_ref());
            let summary = optional(failure.title.as_deref().or(failure.doc.as_deref()));
            let members = if failure.extensions.is_empty() {
                quote!(&[])
            } else {
                let list = format_ident!("__kynos_members_{index}");
                quote!(&#list)
            };
            quote! {
                if #kept {
                    __kynos_branches.push((#uri, #summary, #members));
                }
            }
        });
    let response = quote! {
        ::kynos::__private::problem::response(__kynos_schema, #status, &__kynos_branches)
    };

    let describe = if declaring.iter().all(|failure| failure.headers.is_none()) {
        quote! {
            responses = responses.with(#status, #response);
        }
    } else {
        // One group per kept failure, `()` where it sends none, so a
        // header only some of them send is declared without being
        // required.
        let groups = declaring.iter().zip(kept).map(|(failure, kept)| {
            let group = failure.headers.as_ref().map_or_else(
                || quote!(::kynos::__private::problem::headers::<()>(registry)),
                |field| {
                    let ty = &field.ty;
                    // Only the path is spanned to the field: `registry`
                    // keeps the derive's hygiene context, wherever the
                    // type's tokens came from.
                    let headers = quote_spanned!(ty.span()=>
                        ::kynos::__private::problem::headers::<#ty>
                    );
                    quote!(#headers(registry))
                },
            );
            quote! {
                if #kept {
                    __kynos_groups.push(#group);
                }
            }
        });

        quote! {
            responses = responses.with(#status, {
                let mut response = #response;
                let mut __kynos_groups = ::std::vec::Vec::new();
                #(#groups)*
                ::kynos::__private::problem::declare_headers(&mut response, &__kynos_groups);
                response
            });
        }
    };

    quote! {
        if #(#kept)||* {
            let __kynos_schema = &*__kynos_problem
                .get_or_insert_with(|| registry.resolve::<::kynos::Problem>());
            #(#lists)*
            let mut __kynos_branches: ::std::vec::Vec<(
                ::core::option::Option<&'static str>,
                ::core::option::Option<&'static str>,
                &[(&'static str, ::kynos::openapi::Schema, bool)],
            )> = ::std::vec::Vec::new();
            #(#branches)*
            #describe
        }
    }
}

/// One extension member as the helper declares it: its name, its type's
/// schema, and whether every occurrence sends it. An `Option<T>` is described
/// by `T`, since `None` is not sent rather than sent as `null`.
fn member(extension: &Extension) -> TokenStream2 {
    let name = extension.name.to_string();
    let (ty, sent) = extension
        .optional()
        .map_or((&extension.ty, true), |inner| (inner, false));
    // The type is spanned to the field, so one with no schema is reported
    // where it is named; `registry` is not, so it keeps the derive's hygiene
    // context when a `macro_rules!` hands the type over.
    let resolve = quote_spanned!(ty.span()=> resolve::<#ty>);
    quote!((#name, registry.#resolve(), #sent))
}
