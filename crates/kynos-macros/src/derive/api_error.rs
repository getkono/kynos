//! `#[derive(ApiError)]`.
//!
//! ```text
//! #[problem( base = <prefix> )]                    on the type, optional
//! #[problem( <member> [, <member>]* )]             on each variant, or on a struct
//! #[problem(transparent)]                          on a one-field variant, instead
//! #[problem(extension)]                            on a named field, optional
//! #[problem(headers)]                              on one field, optional
//!
//! prefix := "<URI prefix>"
//!         | <path to a `const` of type `&str`>
//! member := status = <400..=599>                   required, exactly once
//!         | title = "<human-readable summary>"
//!         | type = "<absolute URI>"
//! ```
//!
//! A key outside its position's grammar, a qualified key, and a key written
//! twice on one item or field, across lists too, are refused.
//!
//! `status` is read once into `statuses()`, the `ShortCircuit` const and the
//! `Responses` keys, so the three agree. `base` is the prefix a variant without
//! its own `type` hangs its slug under; a literal is joined here, a `const`
//! in the expansion's own `const` items. `detail` comes from `Display`.
//!
//! A `headers` field holds a header group, its type implementing
//! `EncodeHeaders`: `into_response` writes it onto that failure's problem, and
//! `Responses` declares it under that failure's status alone.
//!
//! A transparent variant answers as the `ApiError` it holds: its problem is
//! that type's, and its statuses and responses join the enum's, the statuses
//! in `const` items of the expansion's own. Its response is that type's too,
//! so the headers the held error sends reach the wire.

mod args;
mod emit;
mod outcomes;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote, quote_spanned};
use syn::{Data, DeriveInput, ExprPath, Ident, Member, Type, parse_macro_input, spanned::Spanned};

use emit::{codes, distinct_statuses, into_problem, into_response, responses};
use outcomes::outcomes;

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`ApiError` cannot describe a union",
        ));
    }

    let outcomes = outcomes(input)?;
    let failures: Vec<&Failure> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Declared(failure) => Some(failure),
            Outcome::Transparent(_) => None,
        })
        .collect();
    let delegates: Vec<&Transparent> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Transparent(delegate) => Some(delegate),
            Outcome::Declared(_) => None,
        })
        .collect();
    let statuses = distinct_statuses(&failures);
    let codes = codes(&outcomes, &statuses);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // `detail` comes from `Display`; asserted here rather than bounded so the
    // diagnostic lands on the error type, not the handler returning it.
    let display = quote! {
        const _: () = {
            #[allow(dead_code)]
            fn detail_comes_from_display #impl_generics () #where_clause {
                fn is_display<T: ::core::fmt::Display + ?Sized>() {}
                is_display::<#name #ty_generics>();
            }
        };
    };

    let problem = into_problem(&failures, &delegates);
    let response = into_response(&failures, &delegates);
    let responses = responses(&failures, &statuses, &delegates);

    // `Responses` and `into_problem` come from the same declarations.
    Ok(quote! {
        #display

        impl #impl_generics ::kynos::error::problem::IntoProblem
            for #name #ty_generics #where_clause
        {
            fn into_problem(self) -> ::kynos::Problem {
                #problem
            }

            fn statuses() -> &'static [::kynos::http::StatusCode] {
                // `StatusCode` has no const constructor, so the validated codes
                // are built once, on first use while the router is built.
                static STATUSES: ::std::sync::LazyLock<
                    ::std::vec::Vec<::kynos::http::StatusCode>
                > = ::std::sync::LazyLock::new(|| {
                    let codes: &'static [u16] = #codes;
                    codes
                        .iter()
                        .map(|code| {
                            ::kynos::http::StatusCode::from_u16(*code)
                                .expect("the derive checked this code")
                        })
                        .collect()
                });
                &STATUSES
            }
        }

        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                #response
            }
        }

        impl #impl_generics ::kynos::response::Responses for #name #ty_generics #where_clause {
            fn responses(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Responses {
                #responses
            }
        }

        // The same list as a `const`, so two interceptors claiming one status
        // is a compile error.
        impl #impl_generics ::kynos::response::ShortCircuit for #name #ty_generics #where_clause {
            const STATUSES: &'static [u16] = #codes;
        }
    })
}

/// What one variant, or the struct, answers with.
enum Outcome {
    /// A failure its own `#[problem(...)]` declares.
    Declared(Failure),

    /// The `ApiError` its one field holds.
    Transparent(Transparent),
}

/// A `#[problem(transparent)]` variant: the field it delegates to.
struct Transparent {
    variant: Ident,
    member: Member,
    ty: Type,
}

/// One way the type can fail: a pattern that matches it, and what it says.
struct Failure {
    /// `Self::Variant`, or `Self` for a struct.
    path: TokenStream2,

    /// The extension members, as the name each is published under and the
    /// binding the pattern gives it.
    extensions: Vec<(String, Ident)>,

    /// The `#[problem(headers)]` field, if this failure sends a header group.
    headers: Option<Box<HeaderField>>,

    status: u16,
    type_uri: Option<TypeUri>,
    title: Option<String>,

    /// The prose a reader already wrote, used where no `title` was given.
    doc: Option<String>,
}

/// A `#[problem(headers)]` field: where the group sits, and its type.
struct HeaderField {
    member: Member,
    ty: Type,
}

/// What `into_response` binds a failure's header group to.
const HEADERS_BINDING: &str = "__kynos_problem_headers";

impl Failure {
    /// The pattern binding every published extension member.
    fn pattern(&self) -> TokenStream2 {
        let path = &self.path;
        let bindings = self.extensions.iter().map(|(_, binding)| binding);
        quote!(#path { #(#bindings,)* .. })
    }

    /// [`pattern`](Self::pattern), binding the header group too.
    fn response_pattern(&self) -> TokenStream2 {
        let path = &self.path;
        let bindings = self.extensions.iter().map(|(_, binding)| binding);
        let headers = self.headers.as_ref().map(|field| {
            let member = &field.member;
            let binding = Ident::new(HEADERS_BINDING, Span::call_site());
            quote!(#member: #binding,)
        });
        quote!(#path { #(#bindings,)* #headers .. })
    }
}

/// A failure's type URI, as the `&'static str` expression the expansion
/// writes.
enum TypeUri {
    /// Known here, so emitted as a literal.
    Literal(String),

    /// A `const` prefix and a slug, joined at compile time by `const` items
    /// the expansion declares, so the URI stays a borrowed `&'static str`.
    Joined { base: ExprPath, slug: String },
}

impl ToTokens for TypeUri {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            Self::Literal(uri) => uri.to_tokens(tokens),
            Self::Joined { base, slug } => {
                // Spanned to the path, so a `const` that is not a `&str`
                // is reported where `base` names it. The names are prefixed
                // because these items shadow the module's: a user `const
                // BASE` would otherwise be defined as itself.
                let prefix =
                    quote_spanned!(base.span()=> const __KYNOS_PROBLEM_BASE: &str = #base;);
                tokens.extend(quote! {{
                    #prefix
                    const __KYNOS_PROBLEM_LEN: usize = __KYNOS_PROBLEM_BASE.len() + #slug.len();
                    const __KYNOS_PROBLEM_BYTES: &[u8] = &::kynos::__private::problem::join::<
                        __KYNOS_PROBLEM_LEN,
                    >(__KYNOS_PROBLEM_BASE, #slug);
                    // A `const`, so the UTF-8 check runs once, at compile time.
                    const __KYNOS_PROBLEM_URI: &str =
                        ::kynos::__private::problem::utf8(__KYNOS_PROBLEM_BYTES);
                    __KYNOS_PROBLEM_URI
                }});
            }
        }
    }
}
