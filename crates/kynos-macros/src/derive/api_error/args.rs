//! The `#[problem(...)]` grammar: what one item's or one field's lists said.

use proc_macro2::Span;
use syn::{
    Attribute, Expr, ExprLit, ExprPath, ExprUnary, Fields, Lit, LitStr, Token, Type, UnOp,
    ext::IdentExt, meta::ParseNestedMeta, spanned::Spanned,
};

use crate::derive::attribute::{Keys, status, unknown};

/// The range a problem detail's status may fall in: RFC 9457 errors, 4xx and
/// 5xx.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 400..=599;

/// The markers one field's `#[problem(...)]` lists carry, each at the span of
/// the attribute naming it.
#[derive(Clone, Copy, Default)]
pub(super) struct Markers {
    /// `#[problem(extension)]`: publish the field as an extension member.
    pub(super) extension: Option<Span>,
    /// `#[problem(headers)]`: the field holds the failure's header group.
    pub(super) headers: Option<Span>,
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
    /// `subset_of`: the wider `ApiError` enum this one is a subset of.
    pub(super) subset_of: Option<(Type, Span)>,
    /// `extension(name = value, ...)`: the members this failure always sends
    /// the same value of, in declaration order.
    pub(super) constants: Vec<Constant>,
}

/// One `extension(name = value)` member: sent under `name`, as `value`
/// serializes, and declared as a `const` of it.
#[derive(Clone)]
pub(super) struct Constant {
    /// The member's name on the wire, a raw identifier unprefixed.
    pub(super) name: String,
    /// Where `name` is written, for a refusal naming it.
    pub(super) span: Span,
    /// A literal, or the path to a `const`.
    pub(super) value: Expr,
}

/// The members RFC 9457 defines, which an extension member cannot take the
/// name of: the problem already sends each under its own meaning.
const REGISTERED: [&str; 5] = ["type", "title", "status", "detail", "instance"];

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
                "subset_of" if position == Position::Type => {
                    args.subset_of = Some((meta.value()?.parse::<Type>()?, span));
                }
                "subset_of" => {
                    return Err(syn::Error::new(
                        span,
                        "`subset_of` names the wider error every variant converts into, so it \
                         belongs on the enum rather than on one variant",
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
                "extension" if meta.input.peek(syn::token::Paren) => {
                    constants(&meta, &mut args.constants)?;
                }
                "extension" => {
                    return Err(syn::Error::new(
                        span,
                        "a bare `extension` marks a field to publish, so it belongs on a field; \
                         a member this failure always sends the same value of is \
                         `extension(name = value)`",
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

/// Reads the `name = value` pairs of an item's `extension(...)` list into
/// `constants`, refusing a name written twice or taken by RFC 9457, and a
/// value that is neither a literal nor a path.
fn constants(meta: &ParseNestedMeta<'_>, constants: &mut Vec<Constant>) -> syn::Result<()> {
    // Read ahead, since syn refuses an empty list as an unexpected end.
    let list;
    syn::parenthesized!(list in meta.input.fork());
    if list.is_empty() {
        return Err(syn::Error::new(
            meta.path.span(),
            "`extension(...)` names no member; write `extension(name = value)`",
        ));
    }

    meta.parse_nested_meta(|member| {
        let Some(ident) = member.path.get_ident() else {
            return Err(syn::Error::new(
                member.path.span(),
                "a constant extension member is named by one identifier, the name it is sent \
                 under",
            ));
        };
        let name = ident.unraw().to_string();
        let span = ident.span();

        if !member.input.peek(Token![=]) {
            return Err(syn::Error::new(
                span,
                format!(
                    "a constant extension member is `name = value`; give `{name}` the value \
                     this failure always sends, or mark a field `#[problem(extension)]`"
                ),
            ));
        }
        if REGISTERED.contains(&name.as_str()) {
            return Err(syn::Error::new(
                span,
                format!(
                    "`{name}` is a member RFC 9457 defines, which the problem already sends; an \
                     extension member takes a name of its own"
                ),
            ));
        }
        if constants.iter().any(|constant| constant.name == name) {
            return Err(syn::Error::new(
                span,
                format!("this failure already sends the extension member `{name}`"),
            ));
        }

        let mut value = member.value()?.parse::<Expr>()?;
        // A `macro_rules!` fragment arrives wrapped in an invisible group.
        while let Expr::Group(group) = value {
            value = *group.expr;
        }
        if !constant_value(&value) {
            return Err(syn::Error::new(
                value.span(),
                "a constant extension member's value is a string, number or boolean literal, or \
                 the path to a `const` whose type is `Serialize`",
            ));
        }

        constants.push(Constant { name, span, value });
        Ok(())
    })
}

/// Whether `value` is what a constant member may hold: a literal the wire
/// writes as one JSON scalar, a negated number, or a path.
fn constant_value(value: &Expr) -> bool {
    match value {
        Expr::Lit(ExprLit {
            lit: Lit::Str(_) | Lit::Int(_) | Lit::Float(_) | Lit::Bool(_),
            ..
        })
        | Expr::Path(_) => true,
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => matches!(
            &**expr,
            Expr::Lit(ExprLit {
                lit: Lit::Int(_) | Lit::Float(_),
                ..
            })
        ),
        _ => false,
    }
}

/// Checks each field's `#[problem(...)]` markers: `extension` or `headers`,
/// each once, and `extension` only on a named field, which has a member name
/// RFC 9457 does not define.
/// Returns what each field carries, in declaration order.
pub(super) fn check_field_markers(fields: &Fields) -> syn::Result<Vec<Markers>> {
    let named = matches!(fields, Fields::Named(_));
    let mut marked = Vec::with_capacity(fields.len());

    for field in fields {
        // Across every list on the field, as on the item.
        let mut keys =
            Keys::new("is already set, and a field's `#[problem(...)]` takes each marker once");
        let mut markers = Markers::default();
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
                    "extension" => {
                        // Named, so checked by the member it is published as.
                        let name = field
                            .ident
                            .as_ref()
                            .map(|ident| ident.unraw().to_string())
                            .unwrap_or_default();
                        if REGISTERED.contains(&name.as_str()) {
                            return Err(syn::Error::new(
                                field.ident.span(),
                                format!(
                                    "the field `{name}` would be published as a member RFC 9457 \
                                     defines, which the problem already sends; rename the field, \
                                     since an extension member takes a name of its own"
                                ),
                            ));
                        }
                        markers.extension = Some(attr.span());
                    }
                    "headers" => markers.headers = Some(attr.span()),
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
        marked.push(markers);
    }

    Ok(marked)
}
