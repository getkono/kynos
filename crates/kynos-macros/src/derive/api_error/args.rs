//! The `#[problem(...)]` grammar: what one item's or one field's lists said.

use proc_macro2::Span;
use syn::{Attribute, Expr, ExprLit, ExprPath, Fields, Lit, LitStr, spanned::Spanned};

use crate::derive::{
    attribute::{Keys, status, unknown},
    common::skip_value,
};

/// The range a problem detail's status may fall in: RFC 9457 errors, 4xx and
/// 5xx.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 400..=599;

/// Whether one attribute is `#[problem(extension)]`.
pub(super) fn is_extension(attr: &Attribute) -> bool {
    marks(attr, "extension")
}

/// Whether one attribute is `#[problem(headers)]`.
pub(super) fn is_headers(attr: &Attribute) -> bool {
    marks(attr, "headers")
}

/// Whether one attribute is a `#[problem(...)]` list naming the field marker
/// `key`.
fn marks(attr: &Attribute, key: &str) -> bool {
    if !attr.path().is_ident("problem") {
        return false;
    }
    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident(key) {
            found = true;
            return Ok(());
        }
        skip_value(&meta)
    });
    found
}

/// Where a `#[problem(...)]` list is written, which decides its legal members.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Position {
    Type,
    Variant,
}

/// What one item's `#[problem(...)]` lists said.
#[derive(Default)]
pub(super) struct ProblemArgs {
    pub(super) status: Option<(u16, Span)>,
    pub(super) title: Option<String>,
    pub(super) type_uri: Option<String>,
    pub(super) base: Option<Base>,
    pub(super) transparent: Option<Span>,
}

/// What `base` named: a prefix written out, or a `const` holding one.
pub(super) enum Base {
    Literal(String),
    Const(ExprPath),
}

/// Reads one item's `#[problem(...)]` lists, validating every member.
pub(super) fn parse_problem(attrs: &[Attribute], position: Position) -> syn::Result<ProblemArgs> {
    let mut args = ProblemArgs::default();
    // A second `status` is left to its own refusal, which names the response
    // it duplicates.
    let mut keys = Keys::new("is already set, and `#[problem(...)]` takes each key once")
        .except(|key| key == "status");

    for attr in attrs {
        if !attr.path().is_ident("problem") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            let span = meta.path.span();

            match key.as_str() {
                "status" => status(
                    &meta,
                    STATUS_RANGE,
                    "a problem detail describes a failure",
                    &mut args.status,
                )?,
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
                        span,
                        "`base` is the prefix every type URI shares, so it belongs on the type \
                         rather than on one variant",
                    ));
                }
                "transparent" if position == Position::Variant => {
                    args.transparent = Some(span);
                }
                "transparent" => {
                    return Err(syn::Error::new(
                        span,
                        "`transparent` hands one variant to the error it holds, so it belongs on \
                         a variant of an enum",
                    ));
                }
                "extension" => {
                    return Err(syn::Error::new(
                        span,
                        "`extension` marks a field to publish, so it belongs on a field",
                    ));
                }
                "headers" => {
                    return Err(syn::Error::new(
                        span,
                        "`headers` marks the field holding the header group a failure sends, so \
                         it belongs on that field, whose type is the group",
                    ));
                }
                other => {
                    return Err(unknown(&meta.path, other, "the `#[problem(...)]` grammar"));
                }
            }
            Ok(())
        })?;
    }

    Ok(args)
}

/// Checks each field's `#[problem(...)]` markers: `extension` or `headers`,
/// each once, and `extension` only on a named field, which has a member name.
pub(super) fn check_field_markers(fields: &Fields) -> syn::Result<()> {
    let named = matches!(fields, Fields::Named(_));

    for field in fields {
        // Across every list on the field, as on the item.
        let mut keys =
            Keys::new("is already set, and a field's `#[problem(...)]` takes each marker once");
        for attr in &field.attrs {
            if !attr.path().is_ident("problem") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                let key = keys.read(&meta.path)?;
                match key.as_str() {
                    "extension" if !named => {
                        return Err(syn::Error::new(
                            attr.span(),
                            "an extension member is published under its field's name, and this \
                             field has none. Give the variant named fields",
                        ));
                    }
                    // A marker is bare; a value would otherwise reach only
                    // syn's "expected `,`".
                    "extension" | "headers"
                        if meta.input.peek(syn::Token![=])
                            || meta.input.peek(syn::token::Paren) =>
                    {
                        return Err(syn::Error::new(
                            meta.path.span(),
                            format!(
                                "`{key}` is a bare marker and takes no value; write \
                                 `#[problem({key})]`"
                            ),
                        ));
                    }
                    "extension" | "headers" => {}
                    other => {
                        return Err(unknown(
                            &meta.path,
                            other,
                            "a field's `#[problem(...)]` grammar, which is `extension` or \
                             `headers`",
                        ));
                    }
                }
                Ok(())
            })?;
        }
    }

    Ok(())
}
