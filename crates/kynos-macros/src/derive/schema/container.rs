//! What the type's own serde attributes and `#[schema(...)]` say, read once
//! per derive.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, Field, Fields, LitStr, Variant, parse::Parser, punctuated::Punctuated,
    spanned::Spanned, token::Comma,
};

use super::{
    attributes::{Sides, serde_flag, sides, variant_rename_all},
    members::{field_groups, is_written},
    refusals::check_constraint,
};
use crate::derive::attribute::Keys;
use crate::derive::common::skip_value;
use crate::doc::doc_string;

/// What the type's own serde attributes said, read rather than restated.
#[derive(Clone, Default)]
pub(super) struct Container {
    /// The container `rename`, serialize side: the name serde writes a
    /// struct's `#[serde(tag = "...")]` as.
    pub(super) rename: Option<String>,
    /// The container `rename_all` style, serialize side, or deserialize side for
    /// an identifier enum, which serde only reads; a split one is otherwise
    /// refused first ([`split_rule`](super::members::split_rule)). In
    /// [`Container::fields_of`], a variant's.
    pub(super) rename_all: Option<String>,
    /// An enum's `rename_all_fields`, each side: names a variant's fields on
    /// each side the variant's own `rename_all` leaves.
    pub(super) rename_all_fields: Sides,
    pub(super) tag: Option<String>,
    pub(super) content: Option<String>,
    /// `#[serde(transparent)]`: the wire form is the one field's value.
    pub(super) transparent: bool,
    pub(super) doc: Option<String>,
    /// A container `#[serde(default)]` on a non-unit struct.
    pub(super) default: bool,
    /// `#[serde(deny_unknown_fields)]`, which [`closed`](super::closed) follows.
    pub(super) deny_unknown_fields: bool,
    /// The visitor serde reads an enum's variants through.
    pub(super) visitor: Visitor,
}

/// The visitor serde's `Deserialize` reads an enum's variants through.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Visitor {
    /// The enum visitor, which honours each variant's `skip` keys.
    #[default]
    Enum,
    /// `#[serde(field_identifier)]` or `#[serde(variant_identifier)]`: the
    /// identifier visitor, which names every declared variant and reads no
    /// `skip` key ([`described_variants`](super::members::described_variants)).
    /// serde refuses to write such an enum.
    Identifier,
}

impl Container {
    pub(super) fn read(input: &DeriveInput) -> Self {
        let mut container = Self {
            doc: doc_string(&input.attrs),
            ..Self::default()
        };
        let mut rename_all = Sides::default();

        for attr in &input.attrs {
            if !attr.path().is_ident("serde") {
                continue;
            }
            // Shape errors in serde's own attribute are serde's to report.
            let _ = attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident() else {
                    return skip_value(&meta);
                };
                match key.to_string().as_str() {
                    "rename" => container.rename = sides(&meta)?.serialize,
                    "rename_all" => rename_all = sides(&meta)?,
                    "rename_all_fields" => container.rename_all_fields = sides(&meta)?,
                    "tag" => container.tag = string_value(&meta)?,
                    "content" => container.content = string_value(&meta)?,
                    "transparent" => container.transparent = true,
                    "deny_unknown_fields" => container.deny_unknown_fields = true,
                    "field_identifier" | "variant_identifier" => {
                        container.visitor = Visitor::Identifier;
                    }
                    _ => skip_value(&meta)?,
                }
                Ok(())
            });
        }

        // Taken once every key is read: `field_identifier` may follow it.
        container.rename_all = match container.visitor {
            Visitor::Enum => rename_all.serialize,
            Visitor::Identifier => rename_all.deserialize,
        };
        container.default = matches!(&input.data, Data::Struct(data) if !matches!(data.fields, Fields::Unit))
            && serde_flag(&input.attrs, &["default"]);

        container
    }

    /// The container a variant's fields are named under: the variant's own
    /// `rename_all`, else the enum's `rename_all_fields`, in place of the enum's
    /// `rename_all`, which serde applies to variant names alone. Serialize side
    /// for a written variant, deserialize side for a read-only one, each rule
    /// on that side, as serde fills it.
    pub(super) fn fields_of(&self, variant: &Variant) -> Self {
        let own = variant_rename_all(variant);
        let rename_all = if is_written(variant) {
            own.serialize
                .or_else(|| self.rename_all_fields.serialize.clone())
        } else {
            own.deserialize
                .or_else(|| self.rename_all_fields.deserialize.clone())
        };
        Self {
            rename_all,
            ..self.clone()
        }
    }
}

/// The field grammar's keys a string takes, which bound a `#[schema(string)]`
/// type's text when written on the type.
const TEXT_KEYS: &[&str] = &[
    "min_length",
    "max_length",
    "pattern",
    "content_encoding",
    "content_media_type",
];

/// What a type's own `#[schema(...)]` says.
#[derive(Default)]
pub(crate) struct TypeSchema {
    /// `constraint_problem = ...`: the problem a broken bound answers with.
    pub(crate) constraint_problem: Option<Expr>,
    /// `string`, with the bounds and examples written beside it.
    pub(crate) string: Option<StringForm>,
}

/// A `#[schema(string, ...)]` type: one string, which `FromStr` reads and
/// `Display` writes, whatever the type declares.
pub(crate) struct StringForm {
    /// The `string` key, where what the key requires of the type is reported.
    pub(crate) span: Span,
    /// A `String` field carrying the text's bounds, so they are described and
    /// checked as any string field's are.
    pub(crate) text: Field,
    /// `examples(...)`, in the order written.
    pub(crate) examples: Vec<LitStr>,
    /// `format = "..."`: the format the text is in.
    pub(crate) format: Option<LitStr>,
}

/// The type's own `#[schema(...)]`: `constraint_problem = ...`, the problem a
/// broken bound answers with, written once; and `string`, which describes the
/// type as the text its `Display` writes, bounded by [`TEXT_KEYS`],
/// illustrated by `examples(...)` and named by `format = "..."`, a claim about
/// the type that its own `FromStr` enforces, as no field's could.
///
/// Any other key is refused rather than ignored: a bound written on the type
/// would otherwise read as enforced and be neither described nor checked. So
/// is a text bound or `examples` without `string`, and under `string` a
/// generic parameter or a member's own `#[schema(...)]`, which would describe
/// nothing. `constraint_type`, which `constraint_problem` replaced, is refused
/// naming it.
pub(crate) fn type_schema(input: &DeriveInput) -> syn::Result<TypeSchema> {
    let mut read = TypeSchema::default();
    let mut string: Option<Span> = None;
    let mut bounds: Vec<TokenStream2> = Vec::new();
    let mut examples: Vec<LitStr> = Vec::new();
    let mut format: Option<LitStr> = None;
    // The first text bound, `examples` or `format`, which only `string` admits.
    let mut textual: Option<(String, Span)> = None;
    let mut keys = Keys::new("is already set, and a type's `#[schema(...)]` takes each key once")
        .except(|key| key == "constraint_problem");

    for attr in &input.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            match key.as_str() {
                "constraint_type" => Err(meta.error(
                    "`constraint_type` is now `constraint_problem`, naming the whole problem: \
                     write `constraint_problem = \
                     kynos::schema::constraints::ConstraintProblem::of_type(...)`",
                )),
                "constraint_problem" => {
                    if read.constraint_problem.is_some() {
                        return Err(meta.error(
                            "`constraint_problem` is already set: a type answers a broken bound \
                             with one problem",
                        ));
                    }
                    read.constraint_problem = Some(meta.value()?.parse()?);
                    Ok(())
                }
                "string" => {
                    if meta.input.peek(syn::Token![=]) || meta.input.peek(syn::token::Paren) {
                        return Err(meta.error("`string` is a flag; write it alone"));
                    }
                    string = Some(meta.path.span());
                    Ok(())
                }
                "examples" => {
                    examples.extend(listed_examples(&meta)?);
                    textual.get_or_insert((key, meta.path.span()));
                    Ok(())
                }
                "format" => {
                    format = Some(named_format(&meta)?);
                    textual.get_or_insert((key, meta.path.span()));
                    Ok(())
                }
                _ if TEXT_KEYS.contains(&key.as_str()) => {
                    // The grammar reads the value as a field's; the fork keeps
                    // its tokens, spans and all, for the field built from them.
                    let written = meta.input.fork();
                    check_constraint(&meta)?;
                    written.parse::<syn::Token![=]>()?;
                    let value: Expr = written.parse()?;
                    let path = &meta.path;
                    bounds.push(quote!(#path = #value));
                    textual.get_or_insert((key, meta.path.span()));
                    Ok(())
                }
                _ => Err(meta.error(
                    "a type's `#[schema(...)]` takes `constraint_problem = ...`, the problem a \
                     broken bound answers with, and `string`, describing the type as the text \
                     its `Display` writes, beside that text's `min_length`, `max_length`, \
                     `pattern`, `content_encoding`, `content_media_type`, `format` and \
                     `examples(...)`; any other bound goes on a field, whose value it holds",
                )),
            }
        })?;
    }

    let Some(span) = string else {
        return match textual {
            Some((key, at)) => Err(syn::Error::new(
                at,
                format!(
                    "`{key}` on a type describes the text of a `#[schema(string)]` type, and \
                     this type is described by what it declares. Add `string` where `FromStr` \
                     and `Display` carry the type as one string, or put a bound on the field \
                     whose value it holds"
                ),
            )),
            None => Ok(read),
        };
    };
    reject_string_declaration(input)?;

    // Unnamed, as a newtype's member is, and parsed rather than built, so
    // the bounds keep the spans they were written at.
    let text = Field::parse_unnamed.parse2(quote! {
        #[schema(#(#bounds),*)]
        ::std::string::String
    })?;
    read.string = Some(StringForm {
        span,
        text,
        examples,
        format,
    });
    Ok(read)
}

/// The strings `examples(...)` lists, at least one.
fn listed_examples(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<LitStr>> {
    let content;
    syn::parenthesized!(content in meta.input);
    let listed = Punctuated::<LitStr, Comma>::parse_terminated(&content)?;
    if listed.is_empty() {
        return Err(meta.error(
            "`examples(...)` lists the strings a reader is shown, so it lists at least one: \
             `examples(\"...\")`, or leave the key out",
        ));
    }
    Ok(listed.into_iter().collect())
}

/// The format `format = "..."` names, which is not empty.
fn named_format(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<LitStr> {
    let named: LitStr = meta.value()?.parse()?;
    if named.value().trim().is_empty() {
        return Err(syn::Error::new(
            named.span(),
            "`format` names the format the text is in, such as `uuid` or `date-time`, so it \
             names one: `format = \"...\"`, or leave the key out",
        ));
    }
    Ok(named)
}

/// What a `#[schema(string)]` type may not declare: a type or const
/// parameter, since one component named by the type's identifier would stand
/// for every instantiation; and a member's own `#[schema(...)]`, since no
/// member is described.
fn reject_string_declaration(input: &DeriveInput) -> syn::Result<()> {
    if let Some(parameter) = input
        .generics
        .params
        .iter()
        .find(|parameter| !matches!(parameter, syn::GenericParam::Lifetime(_)))
    {
        return Err(syn::Error::new(
            parameter.span(),
            "a `#[schema(string)]` type is one component named by its identifier, so every \
             instantiation of a generic one would be published under one name whatever its \
             arguments. Make the type concrete, or implement `Schema` by hand returning a \
             `name()` per instantiation",
        ));
    }
    let members = field_groups(input)
        .into_iter()
        .flat_map(|fields| fields.iter())
        .flat_map(|field| field.attrs.iter());
    let variants = match &input.data {
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| variant.attrs.iter())
            .collect(),
        Data::Struct(_) | Data::Union(_) => Vec::new(),
    };
    if let Some(attr) = members
        .chain(variants)
        .find(|attr| attr.path().is_ident("schema"))
    {
        return Err(syn::Error::new(
            attr.span(),
            "`#[schema(string)]` describes the type as the one string `Display` writes, so no \
             member it declares is described and a member's `#[schema(...)]` would bound \
             nothing. Bound the text on the type: `#[schema(string, max_length = ...)]`",
        ));
    }
    Ok(())
}

/// Whether the type's own `#[schema(...)]` names `string`, read leniently for
/// a derive beside `Schema`, which refuses what is malformed.
pub(crate) fn declares_string(input: &DeriveInput) -> bool {
    let mut found = false;
    for attr in &input.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            found |= meta.path.is_ident("string");
            skip_value(&meta)
        });
    }
    found
}

/// The `= "..."` of a nested-meta item, when it has one.
pub(super) fn string_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if !meta.input.peek(syn::Token![=]) {
        return Ok(None);
    }
    Ok(Some(meta.value()?.parse::<LitStr>()?.value()))
}
