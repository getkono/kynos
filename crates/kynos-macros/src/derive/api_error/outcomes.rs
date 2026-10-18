//! Every way the type can fail, read from what its `#[problem(...)]` lists
//! said and the fields they mark.

use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, quote};
use syn::{
    Data, DeriveInput, Fields, GenericParam, Generics, Ident, Index, Member, Type, Variant,
    spanned::Spanned,
};

use super::{
    Failure, HeaderField, Outcome, Transparent, TypeUri,
    args::{
        Base, Position, ProblemArgs, check_field_markers, is_extension, is_headers, parse_problem,
    },
};
use crate::derive::common::doc_string;

/// Every way the type can fail, read from the `#[problem(...)]` declarations.
pub(super) fn outcomes(input: &DeriveInput) -> syn::Result<Vec<Outcome>> {
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
                    check_field_markers(&variant.fields)?;

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

                    let name = &variant.ident;

                    Ok(Outcome::Declared(Failure {
                        path: quote!(Self::#name),
                        extensions: extensions(&variant.fields),
                        headers: header_field(&variant.fields)?,
                        status,
                        type_uri: type_uri(&args, base.as_ref(), &variant.ident),
                        title: args.title,
                        doc: doc_string(&variant.attrs),
                    }))
                })
                .collect()
        }

        Data::Struct(data) => {
            check_field_markers(&data.fields)?;

            let args = parse_problem(&input.attrs, Position::Type)?;
            let Some((status, _)) = args.status else {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "this error does not say what status it produces; add \
                     `#[problem(status = ...)]`, or use an enum when it can fail several ways",
                ));
            };

            Ok(vec![Outcome::Declared(Failure {
                path: quote!(Self),
                extensions: extensions(&data.fields),
                headers: header_field(&data.fields)?,
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

    if let Some(attr) = field.attrs.iter().find(|attr| is_headers(attr)) {
        return Err(syn::Error::new(
            attr.span(),
            "a transparent variant sends the headers of the error it holds, which is not a \
             header group itself",
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

/// The one field marked `#[problem(headers)]`, refusing a second and one also
/// published as an extension member.
fn header_field(fields: &Fields) -> syn::Result<Option<Box<HeaderField>>> {
    let mut found: Option<(HeaderField, String)> = None;

    for (index, field) in fields.iter().enumerate() {
        let Some(attr) = field.attrs.iter().find(|attr| is_headers(attr)) else {
            continue;
        };

        if field.attrs.iter().any(is_extension) {
            return Err(syn::Error::new(
                attr.span(),
                "a header group is sent as response fields, not published in the body. Mark this \
                 field `extension` or `headers`, not both",
            ));
        }

        let member = field
            .ident
            .clone()
            .map_or_else(|| Member::Unnamed(Index::from(index)), Member::Named);
        if let Some((_, first)) = &found {
            return Err(syn::Error::new(
                attr.span(),
                format!(
                    "this failure already sends the header group in `{first}`; one group holds \
                     every header it sends"
                ),
            ));
        }

        let name = member.to_token_stream().to_string();
        found = Some((
            HeaderField {
                member,
                ty: field.ty.clone(),
            },
            name,
        ));
    }

    Ok(found.map(|(field, _)| Box::new(field)))
}
