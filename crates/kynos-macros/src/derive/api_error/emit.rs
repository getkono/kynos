//! The statuses and what goes on the wire, from the failures read; the
//! description is `describe`'s.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{Ident, Lifetime, Type, spanned::Spanned};

use super::{Failure, HEADERS_BINDING, Outcome, Transparent};

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

    union(parts)
}

/// `parts`, each a `&'static [u16]` expression, joined in `const` items into
/// one without repeats, in order of first appearance.
pub(super) fn union(parts: impl Iterator<Item = TokenStream2>) -> TokenStream2 {
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

    dispatch(
        failures,
        delegates,
        arms,
        |ty| quote_spanned!(ty.span()=> <#ty as ::kynos::error::problem::IntoProblem>::into_problem),
    )
}

/// The `into_response` body: the problem rendered, then the failure's header
/// group written onto it, and a transparent variant answering as its field
/// answers, so the headers that error sends are sent.
///
/// Where nothing sends a header group, the problem rendered alone.
pub(super) fn into_response(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    let report = report(failures, delegates);
    let respond = respond(failures, delegates);
    quote! {
        #report
        #respond
    }
}

/// The statement logging a 5xx failure before it is consumed, through
/// `kynos::__private::problem::Failed`, which is a no-op without `trace`.
///
/// Rendered only where this type declares a 5xx; a transparent variant is left
/// to its field's own `into_response`, so one failure is logged at most once,
/// and only where that field is itself a derived `ApiError`.
fn report(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    let server: Vec<&&Failure> = failures
        .iter()
        .filter(|failure| failure.status >= 500)
        .collect();
    if server.is_empty() {
        return TokenStream2::new();
    }

    let arms = server.iter().map(|failure| {
        let path = &failure.path;
        let status = failure.status;
        let uri = failure
            .type_uri
            .as_ref()
            .map_or_else(|| quote!("about:blank"), |uri| quote!(#uri));
        quote! {
            #path { .. } => (&::kynos::__private::problem::Failed(&self)).report(#status, #uri),
        }
    });
    // A catch-all only where something is left to catch, so none is
    // unreachable.
    let rest = (server.len() < failures.len() || !delegates.is_empty()).then(|| quote!(_ => {}));

    quote! {
        {
            #[allow(unused_imports)]
            use ::kynos::__private::problem::{ReportDisplay as _, ReportSource as _};
            match &self {
                #(#arms)*
                #rest
            }
        }
    }
}

/// [`into_response`] after the failure is logged.
fn respond(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
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

    dispatch(
        failures,
        delegates,
        arms,
        |ty| quote_spanned!(ty.span()=> <#ty as ::kynos::response::IntoResponse>::into_response),
    )
}

/// A `match self` over `arms`, one per failure declared here, then an arm per
/// transparent variant handing its field to the function `delegate` names for
/// the field's type, after the `let detail` an arm reads.
fn dispatch(
    failures: &[&Failure],
    delegates: &[&Transparent],
    arms: impl Iterator<Item = TokenStream2>,
    delegate: impl Fn(&Type) -> TokenStream2,
) -> TokenStream2 {
    let handed_over = delegates.iter().map(|transparent| {
        let variant = &transparent.variant;
        let member = &transparent.member;
        let delegate = delegate(&transparent.ty);
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
    let label = Lifetime::new(PROBLEM_LABEL, Span::call_site());
    // Sent through the same `to_member` as a field, so a `const` whose type is
    // not `Serialize` is reported where its path is written, and one that
    // refuses answers the same 500.
    let with_constants = failure.constants.iter().map(|constant| {
        let name = &constant.name;
        let value = &constant.value;
        let to_member =
            quote_spanned!(value.span()=> ::kynos::__private::problem::to_member(&#value));
        quote! {
            match #to_member {
                ::core::option::Option::Some(member) => {
                    problem = problem.with_extension(#name, member);
                }
                ::core::option::Option::None => {
                    break #label ::kynos::__private::problem::unserializable_member();
                }
            }
        }
    });
    // An `Option` member is sent only when it holds a value, as `detail` is.
    let with_extensions = failure.extensions.iter().map(|extension| {
        let binding = &extension.name;
        let name = binding.to_string();
        let (ty, value) = extension.optional().map_or_else(
            || (&extension.ty, quote!(#binding)),
            |inner| (inner, quote!(value)),
        );
        // Spanned to the field, so a type that is not `Serialize` is reported
        // where it is named.
        let to_member = quote_spanned!(ty.span()=> ::kynos::__private::problem::to_member::<#ty>);
        // A `match`, so `member` is scoped to its arm and shadows no later
        // field bound under that name.
        let send = quote! {
            match #to_member(&#value) {
                ::core::option::Option::Some(member) => {
                    problem = problem.with_extension(#name, member);
                }
                ::core::option::Option::None => {
                    break #label ::kynos::__private::problem::unserializable_member();
                }
            }
        };
        if extension.optional().is_some() {
            quote! {
                if let ::core::option::Option::Some(value) = #binding {
                    #send
                }
            }
        } else {
            send
        }
    });

    let build = quote! {
        let status = ::kynos::http::StatusCode::from_u16(#status)
            .expect("the derive checked this code");
        // `new` titles it with the reason phrase, as RFC 9457 asks of
        // `about:blank`.
        let mut problem = ::kynos::Problem::new(status);
        #with_type
        #with_title
        problem.detail = detail;
        #(#with_constants)*
        #(#with_extensions)*
        problem
    };
    // Labelled only where a member can refuse to serialize, so no label goes
    // unused.
    if failure.sends_members() {
        quote!(#label: { #build })
    } else {
        quote!({ #build })
    }
}

/// The label of a problem block an unserializable extension member breaks out
/// of with the 500 that replaces it.
const PROBLEM_LABEL: &str = "'__kynos_problem";

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
