//! Emitting the endpoint type that accompanies a handler.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr};

use crate::{
    derive::common::optional,
    doc::{doc_texts, is_deprecated},
    route::{
        args::{Head, RouteArgs},
        attrs::{split_doc, take_public},
        uri::endpoint_uri_impl,
    },
};

/// Emits the endpoint type alongside the original handler.
pub(crate) fn emit(method: &str, args: &RouteArgs, function: &ItemFn) -> syn::Result<TokenStream2> {
    let raw_path = args.path.value();

    // The document model's parser, so there is one notion of a valid template.
    let variables = kynos_openapi::PathTemplate::parse(raw_path.clone())
        .map_err(|error| syn::Error::new(args.path.span(), error.to_string()))?
        .variables()
        .to_vec();

    // Read and removed: `#[kynos::public]` written below the route attribute
    // would otherwise expand on its own once this one has.
    let mut function = function.clone();
    let public = take_public(&mut function.attrs)?;
    let function = &function;

    let name = &function.sig.ident;
    let visibility = &function.vis;
    let (summary, description) = split_doc(&doc_texts(&function.attrs));
    // `Operation.deprecated`, read as a schema reads it.
    let deprecated = is_deprecated(&function.attrs);

    let operation_id = args
        .operation_id
        .as_ref()
        .map_or_else(|| name.to_string(), LitStr::value);

    // Only a defaulted identifier is one a router's case policy recases.
    let operation_id_defaulted = args.operation_id.is_none();
    let head_defaulted = matches!(args.head, Some(Head::Derived(_)));

    let head = head_operation_id(method, args, &operation_id)?;
    let head = optional(head.as_deref());
    let summary = optional(summary.as_deref());
    let description = optional(description.as_deref());
    let uri_impl = endpoint_uri_impl(function, &raw_path, &variables)?;
    let variables = variables.iter().map(String::as_str);

    // A braced struct occupies only the type namespace, so it shares the
    // handler function's name without shadowing it.
    let endpoint = format_ident!("{name}");
    let panic_policy = if args.catch_panics {
        quote!(::kynos::middleware::catch_panic::Catch)
    } else {
        quote!(::kynos::middleware::catch_panic::Propagate)
    };
    let panic_strategy_check = args.catch_panics.then(|| {
        quote! {
            #[cfg(panic = "abort")]
            ::core::compile_error!(
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
    });
    // A `DeclaredTag` carries the name and the thunk documenting it, so
    // `from_meta` registers both from this constant; `of` carries the `Tag` bound.
    let tags = args.tag.as_ref().map_or_else(
        || quote!(&[]),
        |tag| quote!(&[::kynos::router::operation::DeclaredTag::of::<#tag>()]),
    );

    let chrono = crate::chrono::signature_markers(&function.sig);

    Ok(quote! {
        #function

        #chrono

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug, ::core::default::Default)]
        #visibility struct #endpoint {}

        impl ::kynos::router::endpoint::meta::EndpointMeta for #endpoint {
            type PanicPolicy = #panic_policy;

            const METHOD: &'static str = #method;
            const PATH: &'static str = #raw_path;
            const PATH_VARIABLES: &'static [&'static str] = &[#(#variables),*];
            const OPERATION_ID: &'static str = #operation_id;
            const OPERATION_ID_DEFAULTED: bool = #operation_id_defaulted;
            const SUMMARY: ::core::option::Option<&'static str> = #summary;
            const DESCRIPTION: ::core::option::Option<&'static str> = #description;
            const DEPRECATED: bool = #deprecated;
            const PUBLIC: bool = #public;
            const TAGS: &'static [::kynos::router::operation::DeclaredTag] = #tags;
            const HEAD_OPERATION_ID: ::core::option::Option<&'static str> = #head;
            const HEAD_OPERATION_ID_DEFAULTED: bool = #head_defaulted;
        }

        #uri_impl

        #panic_strategy_check
    })
}

/// The `operationId` of the `HEAD` a `GET`'s `head` argument describes, or
/// `None` without one.
///
/// A bare `head` derives it from the handler's name, the `GET`'s own
/// `operationId`: `get_file` describes `head_file`, and any other name `n`
/// describes `head_n`. Beside an explicit `operation_id` it is refused, since
/// no spelling derived from a chosen identifier is one its author would
/// expect.
fn head_operation_id(
    method: &str,
    args: &RouteArgs,
    operation_id: &str,
) -> syn::Result<Option<String>> {
    let Some(head) = &args.head else {
        return Ok(None);
    };
    let span = match head {
        Head::Derived(span) => *span,
        Head::Named(name) => name.span(),
    };
    if method != "GET" {
        return Err(syn::Error::new(
            span,
            "`head` describes the HEAD a GET answers (RFC 9110 section 9.3.2), so only a `GET` \
             takes it; declare a HEAD of its own with `#[kynos::head]`",
        ));
    }
    let id = match head {
        Head::Derived(_) if args.operation_id.is_some() => {
            return Err(syn::Error::new(
                span,
                "a bare `head` derives the HEAD's operationId from the handler's name, and this \
                 route names its own `operation_id`; name the HEAD's too, with `head = \"...\"`",
            ));
        }
        Head::Derived(_) => {
            let rest = operation_id.strip_prefix("get_").unwrap_or(operation_id);
            format!("head_{rest}")
        }
        Head::Named(name) => name.value(),
    };
    if id == operation_id {
        return Err(syn::Error::new(
            span,
            format!("`head` names the GET's own operationId `{id}`; the HEAD needs another"),
        ));
    }
    Ok(Some(id))
}
