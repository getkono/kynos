//! `#[derive(ApiError)]`.
//!
//! ```text
//! #[problem( base = <prefix> )]                    on the type, optional
//! #[problem( <member> [, <member>]* )]             on each variant, or on a struct
//! #[problem(transparent)]                          on a one-field variant, instead
//! #[problem(extension)]                            on a named field, optional
//!
//! prefix := "<URI prefix>"
//!         | <path to a `const` of type `&str`>
//! member := status = <400..=599>                   required, exactly once
//!         | title = "<human-readable summary>"
//!         | type = "<absolute URI>"
//! ```
//!
//! `status` is read once into `statuses()`, the `ShortCircuit` const and the
//! `Responses` keys, so the three agree. `base` is the prefix a variant without
//! its own `type` hangs its slug under; a literal is joined here, a `const`
//! in the expansion's own `const` items. `detail` comes from `Display`.
//!
//! A transparent variant answers as the `ApiError` it holds: its problem is
//! that type's, and its statuses and responses join the enum's, the statuses
//! in `const` items of the expansion's own.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, ExprPath, Fields, GenericParam, Generics, Ident,
    Index, Lit, LitInt, LitStr, Member, Type, Variant, parse_macro_input, spanned::Spanned,
};

use crate::derive::common::{doc_string, skip_value};

/// The range a problem detail's status may fall in: RFC 9457 errors, 4xx and
/// 5xx.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 400..=599;

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
                ::kynos::response::IntoResponse::into_response(
                    ::kynos::error::problem::IntoProblem::into_problem(self),
                )
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

/// The statuses as a `&'static [u16]` expression, in declaration order without
/// repeats.
///
/// A literal where every status is declared here. A transparent variant's are
/// its type's `ShortCircuit::STATUSES`, joined with the rest in `const` items,
/// since a `const` slice has no concatenation.
fn codes(outcomes: &[Outcome], statuses: &[u16]) -> TokenStream2 {
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

/// One way the type can fail: a pattern that matches it, and what it says.
struct Failure {
    /// The pattern `into_problem` matches this failure with, already carrying
    /// bindings for every published extension member.
    pattern: TokenStream2,

    /// The extension members, as the name each is published under and the
    /// binding the pattern gave it.
    extensions: Vec<(String, Ident)>,

    status: u16,
    type_uri: Option<TypeUri>,
    title: Option<String>,

    /// The prose a reader already wrote, used where no `title` was given.
    doc: Option<String>,
}

/// The `into_problem` body; `detail` is taken from `Display` before the
/// extension members are moved out, and only where this type declares the
/// failure, since a transparent variant's problem is its field's.
fn into_problem(failures: &[&Failure], delegates: &[&Transparent]) -> TokenStream2 {
    let arms = failures.iter().map(|failure| {
        let pattern = &failure.pattern;
        let status = failure.status;

        let with_type = failure
            .type_uri
            .as_ref()
            .map(|uri| quote!(problem.type_uri = ::std::borrow::Cow::Borrowed(#uri);));
        let with_title = failure
            .title
            .as_ref()
            .map(|title| quote!(problem.title = ::std::borrow::Cow::Borrowed(#title);));
        let with_extensions = failure
            .extensions
            .iter()
            .map(|(name, binding)| quote!(problem = problem.with_extension(#name, #binding);));

        quote! {
            #pattern => {
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
            }
        }
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

    // Rendered only where a declared arm reads it.
    let detail = if failures.is_empty() {
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
    };

    quote! {
        #detail
        match self {
            #(#arms)*
            #(#handed_over)*
        }
    }
}

/// The `responses` body: one response per distinct status, over the `Problem`
/// component, shaped by `kynos::__private::problem::response`, then each
/// transparent variant's responses in declaration order, a status met twice
/// unioned rather than replaced.
fn responses(failures: &[&Failure], statuses: &[u16], delegates: &[&Transparent]) -> TokenStream2 {
    let entries = statuses.iter().map(|status| {
        // Every failure declaring this status, in declaration order.
        let branches = failures
            .iter()
            .filter(|failure| failure.status == *status)
            .map(|failure| {
                let uri = optional(failure.type_uri.as_ref());
                let summary = optional(failure.title.as_deref().or(failure.doc.as_deref()));
                quote!((#uri, #summary))
            });

        quote! {
            responses = responses.with(
                #status,
                ::kynos::__private::problem::response(&schema, #status, &[#(#branches),*]),
            );
        }
    });

    let handed_over = delegates.iter().map(|delegate| {
        let ty = &delegate.ty;
        quote_spanned! {ty.span()=>
            responses.union_from(
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
fn distinct_statuses(failures: &[&Failure]) -> Vec<u16> {
    let mut seen: Vec<u16> = Vec::new();
    for failure in failures {
        if !seen.contains(&failure.status) {
            seen.push(failure.status);
        }
    }
    seen
}

/// Every way the type can fail, read from the `#[problem(...)]` declarations.
fn outcomes(input: &DeriveInput) -> syn::Result<Vec<Outcome>> {
    let base = parse_problem(&input.attrs, Position::Type)?.base;

    match &input.data {
        Data::Enum(data) => {
            // A status belongs on each variant, not the enum.
            if let Some(status) = parse_problem(&input.attrs, Position::Type)?.status {
                return Err(syn::Error::new(
                    status.1,
                    "a status belongs on each variant, because the point of an `ApiError` enum is \
                     that its variants fail differently. Move it to the variants",
                ));
            }

            data.variants
                .iter()
                .map(|variant| {
                    reject_unnamed_extensions(&variant.fields)?;

                    let args = parse_problem(&variant.attrs, Position::Variant)?;
                    if let Some(span) = args.transparent {
                        return transparent(variant, &args, span, &input.generics)
                            .map(Outcome::Transparent);
                    }

                    let Some((status, _)) = args.status else {
                        return Err(syn::Error::new(
                            variant.ident.span(),
                            format!(
                                "variant `{}` does not say what status it produces; add \
                                 `#[problem(status = ...)]`",
                                variant.ident
                            ),
                        ));
                    };

                    let extensions = extensions(&variant.fields);
                    let bindings = extensions.iter().map(|(_, binding)| binding);
                    let name = &variant.ident;

                    Ok(Outcome::Declared(Failure {
                        pattern: quote!(Self::#name { #(#bindings,)* .. }),
                        extensions,
                        status,
                        type_uri: type_uri(&args, base.as_ref(), &variant.ident),
                        title: args.title,
                        doc: doc_string(&variant.attrs),
                    }))
                })
                .collect()
        }

        Data::Struct(data) => {
            reject_unnamed_extensions(&data.fields)?;

            let args = parse_problem(&input.attrs, Position::Type)?;
            let Some((status, _)) = args.status else {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "this error does not say what status it produces; add \
                     `#[problem(status = ...)]`, or use an enum when it can fail several ways",
                ));
            };

            let extensions = extensions(&data.fields);
            let bindings = extensions.iter().map(|(_, binding)| binding);

            Ok(vec![Outcome::Declared(Failure {
                pattern: quote!(Self { #(#bindings,)* .. }),
                extensions,
                status,
                type_uri: type_uri(&args, base.as_ref(), &input.ident),
                title: args.title,
                doc: doc_string(&input.attrs),
            })])
        }

        Data::Union(_) => unreachable!("rejected above"),
    }
}

/// Reads a `#[problem(transparent)]` variant, refusing what it cannot also
/// say.
fn transparent(
    variant: &Variant,
    args: &ProblemArgs,
    span: Span,
    generics: &Generics,
) -> syn::Result<Transparent> {
    if args.status.is_some() || args.title.is_some() || args.type_uri.is_some() {
        return Err(syn::Error::new(
            span,
            "a transparent variant answers as the error it holds, so its status, title and type \
             are that error's. Drop them, or drop `transparent`",
        ));
    }

    let mut fields = variant.fields.iter();
    let (Some(field), None) = (fields.next(), fields.next()) else {
        return Err(syn::Error::new(
            variant.ident.span(),
            format!(
                "transparent variant `{}` holds exactly one field, the error it answers as",
                variant.ident
            ),
        ));
    };

    if let Some(attr) = field.attrs.iter().find(|attr| is_extension(attr)) {
        return Err(syn::Error::new(
            attr.span(),
            "a transparent variant publishes the extension members of the error it holds, not \
             the field holding it",
        ));
    }

    if let Some(param) = generic_named_in(&field.ty, generics) {
        return Err(syn::Error::new(
            field.ty.span(),
            format!(
                "a transparent variant's statuses are a `const`, which cannot depend on the \
                 type's generic parameter `{param}`; name a concrete error type"
            ),
        ));
    }

    Ok(Transparent {
        variant: variant.ident.clone(),
        member: field
            .ident
            .clone()
            .map_or_else(|| Member::Unnamed(Index::from(0)), Member::Named),
        ty: field.ty.clone(),
    })
}

/// The first of `generics`' parameters `ty` names, if any.
fn generic_named_in(ty: &Type, generics: &Generics) -> Option<String> {
    let params: Vec<String> = generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Type(param) => param.ident.to_string(),
            GenericParam::Lifetime(param) => param.lifetime.to_string(),
            GenericParam::Const(param) => param.ident.to_string(),
        })
        .collect();

    first_named(ty.to_token_stream(), &params)
}

/// The first token in `tokens`, at any depth, spelling one of `params`.
fn first_named(tokens: TokenStream2, params: &[String]) -> Option<String> {
    let mut previous_was_apostrophe = false;
    for token in tokens {
        let named = match &token {
            TokenTree::Group(group) => first_named(group.stream(), params),
            TokenTree::Ident(ident) => {
                let spelled = if previous_was_apostrophe {
                    format!("'{ident}")
                } else {
                    ident.to_string()
                };
                params.contains(&spelled).then_some(spelled)
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => None,
        };
        if named.is_some() {
            return named;
        }
        previous_was_apostrophe =
            matches!(&token, TokenTree::Punct(punct) if punct.as_char() == '\'');
    }
    None
}

/// The URI identifying this failure's *type*, if the declaration gives one.
///
/// An explicit `type` wins, else `base` plus the name as a slug; with neither,
/// the problem keeps RFC 9457's `about:blank`.
fn type_uri(args: &ProblemArgs, base: Option<&Base>, name: &Ident) -> Option<TypeUri> {
    if let Some(uri) = &args.type_uri {
        return Some(TypeUri::Literal(uri.clone()));
    }
    let slug = kebab(&name.to_string());
    base.map(|base| match base {
        Base::Literal(base) => TypeUri::Literal(format!("{base}{slug}")),
        Base::Const(path) => TypeUri::Joined {
            base: path.clone(),
            slug,
        },
    })
}

/// What `base` named: a prefix written out, or a `const` holding one.
enum Base {
    Literal(String),
    Const(ExprPath),
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

/// A Rust type or variant name as a URI slug.
fn kebab(name: &str) -> String {
    let mut slug = String::with_capacity(name.len() + 4);
    for (index, character) in name.char_indices() {
        if character.is_uppercase() && index != 0 {
            slug.push('-');
        }
        slug.extend(character.to_lowercase());
    }
    slug
}

/// The named fields marked `#[problem(extension)]`, as the name each is
/// published under and the identifier the pattern binds it to.
fn extensions(fields: &Fields) -> Vec<(String, Ident)> {
    let Fields::Named(named) = fields else {
        return Vec::new();
    };

    named
        .named
        .iter()
        .filter(|field| field.attrs.iter().any(is_extension))
        .filter_map(|field| field.ident.clone().map(|ident| (ident.to_string(), ident)))
        .collect()
}

/// Whether one attribute is `#[problem(extension)]`.
fn is_extension(attr: &Attribute) -> bool {
    if !attr.path().is_ident("problem") {
        return false;
    }
    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("extension") {
            found = true;
            return Ok(());
        }
        skip_value(&meta)
    });
    found
}

/// Where a `#[problem(...)]` list is written, which decides its legal members.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Type,
    Variant,
}

/// What one item's `#[problem(...)]` lists said.
#[derive(Default)]
struct ProblemArgs {
    status: Option<(u16, Span)>,
    title: Option<String>,
    type_uri: Option<String>,
    base: Option<Base>,
    transparent: Option<Span>,
}

/// Reads one item's `#[problem(...)]` lists, validating every member.
fn parse_problem(attrs: &[Attribute], position: Position) -> syn::Result<ProblemArgs> {
    let mut args = ProblemArgs::default();

    for attr in attrs {
        if !attr.path().is_ident("problem") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return Ok(());
            };

            match key.to_string().as_str() {
                "status" => {
                    let literal: LitInt = meta.value()?.parse()?;
                    let code: u16 = literal.base10_parse()?;
                    if !STATUS_RANGE.contains(&code) {
                        return Err(syn::Error::new(
                            literal.span(),
                            format!(
                                "a problem detail describes a failure, so its status is between \
                                 {} and {}; `{code}` is not",
                                STATUS_RANGE.start(),
                                STATUS_RANGE.end()
                            ),
                        ));
                    }
                    if args.status.is_some() {
                        return Err(syn::Error::new(
                            literal.span(),
                            "this already declares a status, and a response has one",
                        ));
                    }
                    args.status = Some((code, literal.span()));
                }
                "title" => args.title = Some(meta.value()?.parse::<LitStr>()?.value()),
                "type" => args.type_uri = Some(meta.value()?.parse::<LitStr>()?.value()),
                "base" if position == Position::Type => {
                    let mut base = meta.value()?.parse::<Expr>()?;
                    // A `macro_rules!` fragment arrives wrapped in an
                    // invisible group; a forwarded literal or path is still one.
                    while let Expr::Group(group) = base {
                        base = *group.expr;
                    }
                    args.base = Some(match base {
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(literal),
                            ..
                        }) => Base::Literal(literal.value()),
                        Expr::Path(path) => Base::Const(path),
                        other => {
                            return Err(syn::Error::new(
                                other.span(),
                                "`base` is a string literal, or the path to a `const` of type \
                                 `&str` holding one",
                            ));
                        }
                    });
                }
                "base" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`base` is the prefix every type URI shares, so it belongs on the type \
                         rather than on one variant",
                    ));
                }
                "transparent" if position == Position::Variant => {
                    args.transparent = Some(key.span());
                }
                "transparent" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`transparent` hands one variant to the error it holds, so it belongs on \
                         a variant of an enum",
                    ));
                }
                "extension" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`extension` marks a field to publish, so it belongs on a field",
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("`{other}` is not part of the `#[problem(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(args)
}

/// Refuses `#[problem(extension)]` on an unnamed field, which has no member
/// name.
fn reject_unnamed_extensions(fields: &Fields) -> syn::Result<()> {
    let Fields::Unnamed(unnamed) = fields else {
        return Ok(());
    };

    for field in &unnamed.unnamed {
        for attr in &field.attrs {
            if !attr.path().is_ident("problem") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("extension") {
                    return Err(syn::Error::new(
                        attr.span(),
                        "an extension member is published under its field's name, and this field \
                         has none. Give the variant named fields",
                    ));
                }
                skip_value(&meta)
            })?;
        }
    }

    Ok(())
}
