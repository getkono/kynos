//! The bodies of the four implementations, from the failures read.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote, quote_spanned};
use syn::{Ident, spanned::Spanned};

use super::{Extension, Failure, HEADERS_BINDING, Outcome, Transparent};

/// The statuses as a `&'static [u16]` expression, in declaration order without
/// repeats.
///
/// A literal where every status is declared here. A transparent variant's are
/// its type's `ShortCircuit::STATUSES`, joined with the rest in `const` items,
/// since a `const` slice has no concatenation.
pub(super) fn codes(outcomes: &[Outcome], statuses: &[u16]) -> TokenStream2 {
    if !outcomes
        .iter()
        .any(|outcome| matches!(outcome, Outcome::Transparent(_)))
    {
        return quote!(&[#(#statuses),*]);
    }

    let parts = outcomes.iter().map(|outcome| match outcome {
        Outcome::Declared(failure) => {
            let status = failure.status;
            quote!(&[#status])
        }
        Outcome::Transparent(delegate) => {
            let ty = &delegate.ty;
            // Spanned to the field, so an inner type that is no `ApiError` is
            // reported where it is named.
            quote_spanned!(ty.span()=> <#ty as ::kynos::response::ShortCircuit>::STATUSES)
        }
    });

    quote! {{
        const __KYNOS_PROBLEM_PARTS: &[&[u16]] = &[#(#parts),*];
        const __KYNOS_PROBLEM_UNION_LEN: usize =
            ::kynos::__private::problem::union_len(__KYNOS_PROBLEM_PARTS);
        const __KYNOS_PROBLEM_UNION: [u16; __KYNOS_PROBLEM_UNION_LEN] =
            ::kynos::__private::problem::union::<__KYNOS_PROBLEM_UNION_LEN>(
                __KYNOS_PROBLEM_PARTS,
            );
        &__KYNOS_PROBLEM_UNION
    }}
}

/// The `into_problem` body; `detail` is taken from `Display` before the
/// extension members are moved out, and only where this type declares the
/// failure, since a transparent variant's problem is its field's.
pub(super) fn into_problem(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    let arms = failures.iter().map(|failure| {
        let pattern = failure.pattern();
        let problem = problem(failure);
        quote!(#pattern => #problem)
    });

    let handed_over = delegates.iter().map(|delegate| {
        let variant = &delegate.variant;
        let member = &delegate.member;
        let ty = &delegate.ty;
        let delegate =
            quote_spanned!(ty.span()=> <#ty as ::kynos::error::problem::IntoProblem>::into_problem);
        quote! {
            Self::#variant { #member: inner } => #delegate(inner),
        }
    });

    let detail = detail(failures, delegates);

    quote! {
        #detail
        match self {
            #(#arms)*
            #(#handed_over)*
        }
    }
}

/// The `into_response` body: the problem rendered, then the failure's header
/// group written onto it, and a transparent variant answering as its field
/// answers, so the headers that error sends are sent.
///
/// Where nothing sends a header group, the problem rendered alone.
pub(super) fn into_response(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    if delegates.is_empty() && failures.iter().all(|failure| failure.headers.is_none()) {
        return quote! {
            ::kynos::response::IntoResponse::into_response(
                ::kynos::error::problem::IntoProblem::into_problem(self),
            )
        };
    }

    let arms = failures.iter().map(|failure| {
        let problem = problem(failure);
        let Some(field) = &failure.headers else {
            let pattern = failure.pattern();
            return quote! {
                #pattern => ::kynos::response::IntoResponse::into_response(#problem),
            };
        };

        let pattern = failure.response_pattern();
        let ty = &field.ty;
        let binding = Ident::new(HEADERS_BINDING, Span::call_site());
        // Spanned to the field, so a type that is no header group is reported
        // where it is named.
        let write = quote_spanned!(ty.span()=> ::kynos::__private::problem::write_headers::<#ty>);
        quote! {
            #pattern => {
                let mut response = ::kynos::response::IntoResponse::into_response(#problem);
                #write(&mut response, &#binding);
                response
            }
        }
    });

    let handed_over = delegates.iter().map(|delegate| {
        let variant = &delegate.variant;
        let member = &delegate.member;
        let ty = &delegate.ty;
        let delegate =
            quote_spanned!(ty.span()=> <#ty as ::kynos::response::IntoResponse>::into_response);
        quote! {
            Self::#variant { #member: inner } => #delegate(inner),
        }
    });

    let detail = detail(failures, delegates);

    quote! {
        #detail
        match self {
            #(#arms)*
            #(#handed_over)*
        }
    }
}

/// The `let detail` an arm of [`problem`] reads, rendered only where one does.
fn detail(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    if failures.is_empty() {
        TokenStream2::new()
    } else if delegates.is_empty() {
        quote! {
            let detail = ::core::option::Option::Some(::std::string::ToString::to_string(&self));
        }
    } else {
        let variants = delegates.iter().map(|delegate| &delegate.variant);
        quote! {
            let detail = if ::core::matches!(self, #(Self::#variants { .. })|*) {
                ::core::option::Option::None
            } else {
                ::core::option::Option::Some(::std::string::ToString::to_string(&self))
            };
        }
    }
}

/// The block building one declared failure's problem, from `detail` and the
/// extension members its pattern bound.
fn problem(failure: &Failure) -> TokenStream2 {
    let status = failure.status;

    let with_type = failure
        .type_uri
        .as_ref()
        .map(|uri| quote!(problem.type_uri = ::std::borrow::Cow::Borrowed(#uri);));
    let with_title = failure
        .title
        .as_ref()
        .map(|title| quote!(problem.title = ::std::borrow::Cow::Borrowed(#title);));
    // An `Option` member is sent only when it holds a value, as `detail` is.
    let with_extensions = failure.extensions.iter().map(|extension| {
        let binding = &extension.name;
        let name = binding.to_string();
        if extension.optional().is_some() {
            quote! {
                if let ::core::option::Option::Some(value) = #binding {
                    problem = problem.with_extension(#name, value);
                }
            }
        } else {
            quote!(problem = problem.with_extension(#name, #binding);)
        }
    });

    quote! {{
        let status = ::kynos::http::StatusCode::from_u16(#status)
            .expect("the derive checked this code");
        // `new` titles it with the reason phrase, as RFC 9457 asks of
        // `about:blank`.
        let mut problem = ::kynos::Problem::new(status);
        #with_type
        #with_title
        problem.detail = detail;
        #(#with_extensions)*
        problem
    }}
}

/// The `responses` body: one response per distinct status, over the `Problem`
/// component, shaped by `kynos::__private::problem::response`, then each
/// transparent variant's responses in declaration order, a status met twice
/// unioned rather than replaced.
pub(super) fn responses(
    failures: &[&Failure],
    statuses: &[u16],
    delegates: &[&Transparent],
) -> TokenStream2 {
    let entries = statuses.iter().map(|status| {
        // Every failure declaring this status, in declaration order.
        let declaring: Vec<&&Failure> = failures
            .iter()
            .filter(|failure| failure.status == *status)
            .collect();
        let branches = declaring.iter().map(|failure| {
            let uri = optional(failure.type_uri.as_ref());
            let summary = optional(failure.title.as_deref().or(failure.doc.as_deref()));
            let members = failure.extensions.iter().map(member);
            quote!((#uri, #summary, &[#(#members),*]))
        });
        let response =
            quote!(::kynos::__private::problem::response(&schema, #status, &[#(#branches),*]));

        if declaring.iter().all(|failure| failure.headers.is_none()) {
            return quote! {
                responses = responses.with(#status, #response);
            };
        }

        // One group per failure, `()` where it sends none, so a header only
        // some of them send is declared without being required.
        let groups = declaring.iter().map(|failure| {
            failure.headers.as_ref().map_or_else(
                || quote!(::kynos::__private::problem::headers::<()>(registry)),
                |field| {
                    let ty = &field.ty;
                    quote_spanned!(ty.span()=> ::kynos::__private::problem::headers::<#ty>(registry))
                },
            )
        });

        quote! {
            responses = responses.with(#status, {
                let mut response = #response;
                ::kynos::__private::problem::declare_headers(&mut response, &[#(#groups),*]);
                response
            });
        }
    });

    let handed_over = delegates.iter().map(|delegate| {
        let ty = &delegate.ty;
        quote_spanned! {ty.span()=>
            ::kynos::__private::problem::union_responses(
                &mut responses,
                &<#ty as ::kynos::response::Responses>::responses(registry),
            );
        }
    });

    // Resolved only where a declared status reads it.
    let schema = if !statuses.is_empty() {
        quote!(let schema = registry.resolve::<::kynos::Problem>();)
    } else if delegates.is_empty() {
        quote!(let _ = registry;)
    } else {
        TokenStream2::new()
    };

    quote! {
        #schema
        let mut responses = ::kynos::openapi::Responses::new();
        #(#entries)*
        #(#handed_over)*
        responses
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
    // Spanned to the field, so a type with no schema is reported where it is
    // named.
    let schema = quote_spanned!(ty.span()=> registry.resolve::<#ty>());
    quote!((#name, #schema, #sent))
}

/// A string the declaration may not have given, as the `Option` the helper
/// reads it as.
fn optional(value: Option<impl ToTokens>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |value| quote!(::core::option::Option::Some(#value)),
    )
}

/// The statuses in declaration order, without repeats; two variants may share
/// a status.
pub(super) fn distinct_statuses(failures: &[&Failure]) -> Vec<u16> {
    let mut seen: Vec<u16> = Vec::new();
    for failure in failures {
        if !seen.contains(&failure.status) {
            seen.push(failure.status);
        }
    }
    seen
}
