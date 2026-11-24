//! `#[derive(ApiError)]`.
//!
//! ```text
//! #[problem( base = <prefix> )]                    on the type, optional
//! #[problem( describe( <code> = "<text>" [, <code> = "<text>"]* ) )]
//!                                                  on an enum, optional
//! #[problem( subset_of = <type> )]                 on an enum, alone, instead of
//!                                                  every list below it
//! #[problem( <member> [, <member>]* )]             on each variant, or on a struct
//! #[problem(transparent)]                          on a one-field variant, instead;
//!                                                  implied by `#[error(transparent)]`
//!                                                  on a variant with no `status`
//! #[problem( variant_of = <path>::<variant> )]     on a variant, alone, instead
//! #[problem(extension)]                            on a named field, optional
//! #[problem(headers)]                              on one field, optional
//! #[problem(retry_after)]                          on one field, optional
//!
//! prefix := "<URI prefix>"
//!         | <path to a `const` of type `&str`>
//!         | default                                the crate's `kynos::problem_base!`
//! member := status = <400..=599>                   required, exactly once
//!         | title = "<human-readable summary>"
//!         | type = "<absolute URI>"
//!         | extension( <name> = <value> [, <name> = <value>]* )
//! value  := <string, number or boolean literal>
//!         | <path to a `const` whose type is `Serialize`>
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
//! `describe` writes a status's description as [`written`] says.
//!
//! An `extension` field is sent through `kynos::__private::problem::to_member`,
//! its type's `Serialize`, and declared by its type's `Schema` on its failure's
//! branch, required; an `Option<T>` is sent only when it holds a value, so it
//! is declared by `T` and optional. One that refuses to serialize breaks out of
//! the problem block with the 500 `unserializable_member` builds. Its name,
//! unprefixed, is refused where RFC 9457 defines it, as `with_extension`
//! would drop the member the schema declares.
//!
//! An `extension(name = value)` member is a variant's, or a struct's, never
//! an enum's: sent through `to_constant`, which also refuses `null`, before
//! the fields', and
//! declared, required, by `kynos::__private::problem::constant` as a `const`
//! of what it serializes to, which the shared-URI branch unions into an
//! `enum`. Its name is refused where RFC 9457 defines it or a field on the
//! same failure is published under it.
//!
//! A `headers` field holds a header group, its type implementing
//! `EncodeHeaders`: `into_response` writes it onto that failure's problem, and
//! `Responses` declares it under that failure's status alone.
//!
//! A `retry_after` field holds a delay, its type implementing
//! `kynos::__private::problem::RetryAfterField`: a `Duration`, or an
//! `Option<Duration>` sent only when it holds one. It is written and declared
//! as `kynos::http::retry_after::RetryAfter`, joined to the failure's
//! `headers` group where it has one, and a group naming `Retry-After` beside
//! it is refused in a `const` block of that arm.
//!
//! A transparent variant answers as the `ApiError` it holds: its problem is
//! that type's, and its statuses and responses join the enum's, the statuses
//! in `const` items of the expansion's own. Its response is that type's too,
//! so the headers the held error sends reach the wire. Each is read through
//! `kynos::__private::problem::HeldError`, so a held type that is no
//! `ApiError` is reported as that, at the field, rather than once per trait
//! each use needs.
//!
//! A `subset_of` enum's variants are some of the derived enum `<type>` names,
//! by name and fields; it converts into that enum and answers and is described
//! as it, as [`subset`] says. A `variant_of` variant is one variant of such an
//! enum, in an enum otherwise its own, and converts back, as [`variant_of`]
//! says.
//!
//! `into_response` first hands a 5xx failure to
//! `kynos::__private::problem::Failed`, which logs it under `trace`; the
//! feature is the facade's, so the expansion emits the call either way.

mod args;
mod describe;
mod emit;
mod fields;
mod outcomes;
mod subset;
mod variant_of;
mod written;

use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote, quote_spanned};
use syn::{Data, DeriveInput, ExprPath, Ident, Member, Type, ext::IdentExt, spanned::Spanned};

use args::{Constant, Position, parse_problem};
use describe::responses;
use emit::{codes, distinct_statuses, into_problem, into_response};
use fields::HeaderField;
use outcomes::outcomes;

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`ApiError` cannot describe a union",
        ));
    }

    let args = parse_problem(&input.attrs, Position::Type)?;
    if let Some((wide, span)) = &args.subset_of {
        return subset::expand(input, &args, wide, *span);
    }

    let outcomes = outcomes(input, &args)?;
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
    let described = args.described.as_ref();
    let answered = written::answered(input, described, &statuses, &delegates, &codes)?;
    let described = described.map_or(&[][..], |(described, _)| described);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // `detail` comes from `Display`; asserted here rather than bounded so the
    // diagnostic lands on the error type, not the handler returning it.
    let display = quote! {
        const _: () = {
            #[allow(dead_code)]
            fn detail_comes_from_display #impl_generics () #where_clause {
                fn is_display<T: ::core::fmt::Display + ?::core::marker::Sized>() {}
                is_display::<#name #ty_generics>();
            }
        };
    };

    let problem = into_problem(&failures, &delegates);
    let response = into_response(&failures, &delegates);

    // An enum describes itself through the handles a subset of it reads, so
    // the two cannot be described apart.
    let (responses, handles) = if matches!(input.data, Data::Enum(_)) {
        let variants = outcomes.iter().map(|outcome| match outcome {
            Outcome::Declared(failure) => {
                let status = failure.status;
                let variant = failure
                    .variant
                    .clone()
                    .expect("an enum's failures are its variants");
                (variant, quote!(&[#status]))
            }
            Outcome::Transparent(delegate) => (delegate.variant.clone(), delegate.statuses()),
        });
        let body = responses(&failures, &statuses, &delegates, described, true);
        (
            quote!(Self::__kynos_problem_responses(
                registry,
                ::core::option::Option::None
            )),
            subset::handles(input, variants, &body),
        )
    } else {
        (
            responses(&failures, &statuses, &delegates, described, false),
            TokenStream2::new(),
        )
    };

    let impls = impls(input, &problem, &response, &responses, &codes);
    let back = variant_of::conversions_back(input, &delegates)?;

    // `Responses` and `into_problem` come from the same declarations.
    Ok(quote! {
        #display
        #answered
        #impls
        #handles
        #back
    })
}

/// The four implementations every `ApiError` gets, around their bodies:
/// `IntoProblem`, `IntoResponse`, `Responses`, and `ShortCircuit` over
/// `codes`, which `statuses()` reads too, so the two agree.
fn impls(
    input: &DeriveInput,
    problem: &TokenStream2,
    response: &TokenStream2,
    responses: &TokenStream2,
    codes: &TokenStream2,
) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
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
                    ::core::iter::Iterator::collect(::core::iter::Iterator::map(
                        codes.iter(),
                        |code| {
                            ::kynos::http::StatusCode::from_u16(*code)
                                .expect("the derive checked this code")
                        },
                    ))
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
    }
}

/// What one variant, or the struct, answers with.
enum Outcome {
    /// A failure its own `#[problem(...)]` declares.
    Declared(Failure),

    /// Another `ApiError` it answers as: its one field, or a wider variant.
    Transparent(Transparent),
}

/// A variant answering as another `ApiError`: the one field a transparent
/// variant hands over, or the wider variant `variant_of` names, built from the
/// variant's fields.
struct Transparent {
    variant: Ident,
    /// The braced bindings after `Self::Variant` that `held` is built from.
    bindings: TokenStream2,
    /// The value handed over.
    held: TokenStream2,
    /// The type answering: the field's, or the wider enum.
    ty: Type,
    /// For `variant_of`, the wider variant, whose statuses and description
    /// are its own rather than the whole wider enum's.
    of: Option<Ident>,
}

impl Transparent {
    /// A transparent variant handing over the field at `member`.
    fn field(variant: Ident, member: &Member, ty: Type) -> Self {
        Self {
            variant,
            bindings: quote!({ #member: inner }),
            held: quote!(inner),
            ty,
            of: None,
        }
    }
}

/// One way the type can fail: a pattern that matches it, and what it says.
struct Failure {
    /// `Self::Variant`, or `Self` for a struct.
    path: TokenStream2,

    /// The variant, or `None` for a struct.
    variant: Option<Ident>,

    /// The `extension(name = value)` members, in declaration order, sent
    /// before the fields'.
    constants: Vec<Constant>,

    /// The extension members, in declaration order.
    extensions: Vec<Extension>,

    /// The `#[problem(headers)]` field, if this failure sends a header group.
    headers: Option<Box<HeaderField>>,

    /// The `#[problem(retry_after)]` field, if this failure sends a delay.
    retry_after: Option<Box<HeaderField>>,

    status: u16,
    type_uri: Option<TypeUri>,
    title: Option<String>,

    /// The prose a reader already wrote, used where no `title` was given.
    doc: Option<String>,
}

/// A `#[problem(extension)]` field: its name, which is the binding the pattern
/// gives it and, unprefixed, the member it is published under, and its type.
struct Extension {
    name: Ident,
    ty: Type,
}

impl Extension {
    /// The member's name on the wire and in the schema, a raw identifier
    /// unprefixed, as serde and the `Schema` derive spell it.
    fn wire_name(&self) -> String {
        self.name.unraw().to_string()
    }

    /// The `T` of an `Option<T>` member, which is sent only when it holds a
    /// value; the recognition is syntactic, so an alias for it is sent always.
    fn optional(&self) -> Option<&Type> {
        crate::derive::common::wrapped(&self.ty, "Option")
    }
}

impl Failure {
    /// Whether the failure sends any extension member, a constant or a field.
    fn sends_members(&self) -> bool {
        !self.constants.is_empty() || !self.extensions.is_empty()
    }

    /// The pattern binding every published extension member.
    fn pattern(&self) -> TokenStream2 {
        let path = &self.path;
        let bindings = self.extensions.iter().map(|extension| &extension.name);
        quote!(#path { #(#bindings,)* .. })
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
