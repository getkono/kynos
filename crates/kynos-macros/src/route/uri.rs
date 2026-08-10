//! The typed `Endpoint::uri` constructor emitted alongside each handler.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{GenericArgument, ItemFn, PathArguments, Type, spanned::Spanned};

pub(crate) fn endpoint_uri_impl(
    function: &ItemFn,
    path: &str,
    variables: &[String],
) -> syn::Result<TokenStream2> {
    let endpoint = &function.sig.ident;
    let path_type = extractor_type(function, "Path")?;
    let query_type = extractor_type(function, "Query")?;

    if variables.is_empty() && path_type.is_some() {
        return Err(syn::Error::new(
            function.sig.span(),
            "the handler extracts Path<T>, but its route has no path variables",
        ));
    }
    if !variables.is_empty() && path_type.is_none() {
        return Err(syn::Error::new(
            function.sig.span(),
            "the route has path variables, but the handler has no Path<T> extractor",
        ));
    }

    // Read from `EndpointMeta::PATH_VARIABLES` rather than rebuilt here, so
    // that what the description will say and what the handler destructures are
    // checked against one source rather than two that could drift.
    let path_assertion = path_type.as_ref().map(|path_type| {
        quote! {
            const _: () = assert!(::kynos::__private::path::path_parameter_names_match(
                <#path_type as ::kynos::extract::params::path::PathParams>::NAMES,
                <#endpoint as ::kynos::router::endpoint::EndpointMeta>::PATH_VARIABLES,
            ), "PathParams names must exactly match route variables in declaration order");
        }
    });

    let uri = match (path_type, query_type) {
        (None, None) => quote! {
            impl #endpoint {
                /// Builds this endpoint's URI.
                pub fn uri() -> ::kynos::http::Uri {
                    ::kynos::__private::uri::endpoint_uri(#path)
                }
            }
        },
        (Some(path_type), None) => quote! {
            impl #endpoint {
                /// Builds this endpoint's URI from its exact path parameters.
                pub fn uri(path: #path_type) -> ::kynos::http::Uri {
                    ::kynos::__private::uri::endpoint_uri_with_path(#path, &path)
                }
            }
        },
        (None, Some(query_type)) => quote! {
            impl #endpoint {
                /// Builds this endpoint's URI from its exact query parameters.
                pub fn uri(query: #query_type) -> ::kynos::http::Uri {
                    ::kynos::__private::uri::endpoint_uri_with_query(#path, &query)
                }
            }
        },
        (Some(path_type), Some(query_type)) => quote! {
            impl #endpoint {
                /// Builds this endpoint's URI from its exact path and query parameters.
                pub fn uri(path: #path_type, query: #query_type) -> ::kynos::http::Uri {
                    ::kynos::__private::uri::endpoint_uri_with_path_and_query(#path, &path, &query)
                }
            }
        },
    };

    Ok(quote! {
        #path_assertion
        #uri
    })
}

fn extractor_type(function: &ItemFn, extractor: &str) -> syn::Result<Option<Type>> {
    let mut found = None;
    for input in &function.sig.inputs {
        let syn::FnArg::Typed(argument) = input else {
            continue;
        };
        let Type::Path(type_path) = argument.ty.as_ref() else {
            continue;
        };
        let Some(segment) = type_path.path.segments.last() else {
            continue;
        };
        if segment.ident != extractor {
            continue;
        }
        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return Err(syn::Error::new(
                segment.span(),
                format!("{extractor} needs one type argument"),
            ));
        };
        let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
            return Err(syn::Error::new(
                arguments.span(),
                format!("{extractor} needs one type argument"),
            ));
        };
        if found.replace(inner.clone()).is_some() {
            return Err(syn::Error::new(
                argument.span(),
                format!("a handler may extract {extractor}<T> only once"),
            ));
        }
    }
    Ok(found)
}
