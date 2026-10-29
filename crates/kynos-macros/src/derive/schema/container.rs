//! What the type's own serde attributes say, read once per derive.

use syn::{Data, DeriveInput, Fields, LitStr, Variant};

use super::{
    attributes::{Sides, serde_flag, sides, variant_rename_all},
    members::is_written,
};
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

/// The `= "..."` of a nested-meta item, when it has one.
pub(super) fn string_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if !meta.input.peek(syn::Token![=]) {
        return Ok(None);
    }
    Ok(Some(meta.value()?.parse::<LitStr>()?.value()))
}
