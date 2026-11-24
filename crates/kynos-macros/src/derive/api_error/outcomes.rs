//! Every way the type can fail, read from what its `#[problem(...)]` lists
//! said and the fields they mark.

use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, quote};
use syn::{Data, DeriveInput, Fields, GenericParam, Generics, Ident, Type, Variant};

use super::{
    Extension, Failure, Outcome, TypeUri,
    args::{Base, Constant, Markers, Position, ProblemArgs, check_field_markers, parse_problem},
    fields::{header_field, retry_after_field},
    transparent::{self, error_transparent},
};
use crate::doc::doc_string;

/// Every way the type can fail, read from the `#[problem(...)]` declarations.
pub(super) fn outcomes(input: &DeriveInput, args: &ProblemArgs) -> syn::Result<Vec<Outcome>> {
    let base = args.base.as_ref();

    match &input.data {
        Data::Enum(data) => {
            // A status belongs on each variant, not the enum.
            if let Some(status) = args.status {
                return Err(syn::Error::new(
                    status.1,
                    "a status belongs on each variant, because the point of an `ApiError` enum is \
                     that its variants fail differently. Move it to the variants",
                ));
            }
            if let Some(constant) = args.constants.first() {
                return Err(syn::Error::new(
                    constant.span,
                    "a constant extension member is the value one failure sends, so it belongs \
                     on each variant sending it. Move it to the variants",
                ));
            }

            // `transparent` on the enum: every variant answers, and the enum
            // displays, as the error it holds.
            let whole = args.transparent;
            if whole.is_some() {
                transparent::display_unclaimed(input, data)?;
            } else if let Some(span) = args.from {
                return Err(syn::Error::new(
                    span,
                    "`from` on an enum builds every variant from the error it holds, so it needs \
                     `transparent` beside it; mark each transparent variant `#[problem(from)]` \
                     instead",
                ));
            }

            data.variants
                .iter()
                .map(|variant| variant_outcome(input, variant, (whole, args.from), base))
                .collect()
        }

        Data::Struct(data) => {
            if let Some(span) = args.transparent.or(args.from) {
                return Err(syn::Error::new(
                    span,
                    "`transparent` and `from` hand a variant to the error it holds, so they \
                     belong on an enum or one of its variants; a struct has no variant to hand \
                     over",
                ));
            }

            let markers = check_field_markers(&data.fields)?;

            let Some((status, _)) = args.status else {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "this error does not say what status it produces; add \
                     `#[problem(status = ...)]`, or use an enum when it can fail several ways",
                ));
            };

            let extensions = extensions(&data.fields, &markers);

            Ok(vec![Outcome::Declared(Failure {
                path: quote!(Self),
                variant: None,
                constants: constants(args.constants.clone(), &extensions)?,
                extensions,
                headers: header_field(&data.fields, &markers)?,
                retry_after: retry_after_field(&data.fields, &markers)?,
                status,
                type_uri: type_uri(args, base, &input.ident),
                title: args.title.clone(),
                doc: doc_string(&input.attrs),
            })])
        }

        Data::Union(_) => unreachable!("rejected above"),
    }
}

/// How one variant of `input` fails, `whole` and `from` the spans of a
/// `transparent` and a `from` on the enum.
fn variant_outcome(
    input: &DeriveInput,
    variant: &Variant,
    (whole, from): (Option<Span>, Option<Span>),
    base: Option<&Base>,
) -> syn::Result<Outcome> {
    let markers = check_field_markers(&variant.fields)?;
    let generics = &input.generics;

    let mut args = parse_problem(&variant.attrs, Position::Variant)?;
    if let Some(whole) = whole {
        return transparent::whole(variant, &args, whole, from, &markers, generics)
            .map(Outcome::Transparent);
    }
    if args.include.is_some() {
        return super::include::read(variant, &args, None, &markers, generics)
            .map(Outcome::Transparent);
    }
    // `#[error(transparent)]` already says the variant is what it holds; a
    // status of its own is the opt-out. Only a variant: a struct has no
    // variant to hand over, so a struct-level one implies nothing and the
    // struct still declares its status.
    let implied = error_transparent(&variant.attrs).filter(|_| args.status.is_none());
    if let (Some(span), true) = (
        args.from,
        args.variant_of.is_some() || args.transparent.or(implied).is_none(),
    ) {
        return Err(syn::Error::new(
            span,
            format!(
                "`from` builds a transparent variant from the one error it holds, and variant \
                 `{}` is not transparent; mark it `transparent`, or write the `From` it needs by \
                 hand",
                variant.ident
            ),
        ));
    }
    if let Some((wide, span)) = &args.variant_of {
        return super::variant_of::variant_of(variant, &args, wide, *span, &markers, generics)
            .map(Outcome::Transparent);
    }
    if let Some(span) = args.transparent {
        return transparent::read(variant, &args, span, args.from, &markers, generics)
            .map(Outcome::Transparent);
    }
    if let Some(span) = implied {
        return transparent::implied(variant, &args, span, &markers, generics)
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

    let name = &variant.ident;
    let extensions = extensions(&variant.fields, &markers);

    Ok(Outcome::Declared(Failure {
        path: quote!(Self::#name),
        variant: Some(name.clone()),
        constants: constants(std::mem::take(&mut args.constants), &extensions)?,
        extensions,
        headers: header_field(&variant.fields, &markers)?,
        retry_after: retry_after_field(&variant.fields, &markers)?,
        status,
        type_uri: type_uri(&args, base, &variant.ident),
        title: args.title,
        doc: doc_string(&variant.attrs),
    }))
}

/// The first of `generics`' parameters `ty` names, if any.
pub(super) fn generic_named_in(ty: &Type, generics: &Generics) -> Option<String> {
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
/// An explicit `type` wins, else `base` plus the name as a slug, `base =
/// default` reading the crate's `kynos::problem_base!`; with neither,
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
        // The item `kynos::problem_base!` declares, spanned to `default` so a
        // crate declaring none is told so there.
        Base::Default(span) => TypeUri::Joined {
            base: syn::parse_quote_spanned!(*span=> crate::__KYNOS_PROBLEM_BASE),
            slug,
        },
    })
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

/// The named fields marked `#[problem(extension)]`, by their `markers`.
fn extensions(fields: &Fields, markers: &[Markers]) -> Vec<Extension> {
    let Fields::Named(named) = fields else {
        return Vec::new();
    };

    named
        .named
        .iter()
        .zip(markers)
        .filter(|(_, marked)| marked.extension.is_some())
        .filter_map(|(field, _)| {
            field.ident.clone().map(|name| Extension {
                name,
                ty: field.ty.clone(),
            })
        })
        .collect()
}

/// The failure's constant members, refusing one named like a field it
/// publishes: one name on the wire holds one value.
fn constants(constants: Vec<Constant>, extensions: &[Extension]) -> syn::Result<Vec<Constant>> {
    for constant in &constants {
        if extensions
            .iter()
            .any(|extension| extension.wire_name() == constant.name)
        {
            return Err(syn::Error::new(
                constant.span,
                format!(
                    "the field `{}` is already published as this extension member; send it \
                     from the field or as a constant, not both",
                    constant.name
                ),
            ));
        }
    }
    Ok(constants)
}
