//! What the `MultipartForm` derive refuses before it emits anything: a form
//! the `Schema` derive describes and a part cannot carry in both directions.

use syn::{DeriveInput, FieldsNamed};

use crate::derive::schema::{attributes::serde_key_span, split_renamed_field, split_rule};

/// serde's keys that leave a field out of what serde reads or writes.
const SKIPS: &[&str] = &[
    "skip",
    "skip_serializing",
    "skip_deserializing",
    "skip_serializing_if",
];

/// Runs every refusal: a split name first, then each serde key a part cannot
/// follow, at the key.
pub(super) fn check(input: &DeriveInput, fields: &FieldsNamed) -> syn::Result<()> {
    if let Some(span) = split_rule(&input.attrs, "rename_all") {
        return Err(syn::Error::new(
            span,
            "a split `rename_all` whose sides differ gives every field two part names, and a part \
             carries one in both directions. Say which with `rename_all = \"...\"`",
        ));
    }
    if let Some(span) = split_renamed_field(input, fields) {
        return Err(syn::Error::new(
            span,
            "a split `rename` gives this field two part names, and a part carries one in both \
             directions. Say which with `rename = \"...\"`",
        ));
    }
    reject_unfollowed_keys(input, fields)
}

/// Refuses each serde key the `Schema` derive describes and a part cannot
/// follow, at the key.
fn reject_unfollowed_keys(input: &DeriveInput, fields: &FieldsNamed) -> syn::Result<()> {
    if let Some((_, span)) = serde_key_span(&input.attrs, &["default"]) {
        return Err(syn::Error::new(
            span,
            "a container `default` fills a field left out from the form's own `Default`, and a \
             form is read one part at a time, never built whole. Put `#[serde(default)]` on each \
             field that may be left out",
        ));
    }
    if let Some((_, span)) = serde_key_span(&input.attrs, &["tag"]) {
        return Err(syn::Error::new(
            span,
            "a container `tag` describes a required property named by the tag and holding the \
             struct's name, which no part carries in either direction. Drop `tag`, or declare \
             that property as a field of the form",
        ));
    }
    if let Some((_, span)) = serde_key_span(&input.attrs, &["transparent"]) {
        return Err(syn::Error::new(
            span,
            "`transparent` describes the form as its one field's schema, and a form carries that \
             field as a part under the field's own name. Drop `transparent`",
        ));
    }
    for field in &fields.named {
        if let Some((_, span)) = serde_key_span(&field.attrs, &["alias"]) {
            return Err(syn::Error::new(
                span,
                "an `alias` gives this field a second part name, and a part is read and written \
                 under one. Drop the `alias`, or make the name it carries the field's `rename`",
            ));
        }
        if let Some((key, span)) = serde_key_span(&field.attrs, SKIPS) {
            return Err(syn::Error::new(
                span,
                format!(
                    "`{key}` leaves this field out of what serde reads or writes, and a form \
                     reads and writes every field as its parts, so the description would not be \
                     the body. Drop `{key}`, or move the field out of the form"
                ),
            ));
        }
        if let Some((_, span)) = serde_key_span(&field.attrs, &["flatten"]) {
            return Err(syn::Error::new(
                span,
                "a `flatten`ed field is described as its own members, and a form carries each \
                 field as parts under the field's own name. Declare those members as fields of \
                 the form",
            ));
        }
    }
    Ok(())
}
