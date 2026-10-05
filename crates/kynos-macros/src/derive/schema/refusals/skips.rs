//! Members serde leaves out in one direction only, where no schema is true of
//! both the shape it writes and the shape it reads.

use syn::{Data, DeriveInput, Field, Fields, punctuated::Punctuated, token::Comma};

use crate::derive::schema::{
    Container,
    attributes::{
        is_described, is_flattened, is_open, is_option, is_required, is_unit_like, serde_flag,
        serde_key_span,
    },
    described_variants, is_written, one_way_skip_span, positional_members,
};

/// A variant serde writes and never reads has no closed schema true of both.
///
/// `skip_deserializing` alone keeps the variant in what serde writes and out of
/// what it reads, so a `oneOf` or `enum` listing it describes a request serde
/// refuses, and one leaving it out describes a response serde writes.
/// `skip_serializing` alone is the other way round and needs no refusal: every
/// variant serde writes is one it also reads, so the schema listing the variant
/// is true of both. A variant serde skips both ways is in no schema.
pub(super) fn reject_unread_variant(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };

    for variant in described_variants(data) {
        if let Some((_, span)) = serde_key_span(&variant.attrs, &["skip_deserializing"]) {
            return Err(syn::Error::new(
                span,
                "`skip_deserializing` makes serde write this variant and refuse to read it back, \
                 so no closed `oneOf` or `enum` is true in both directions: listing the variant \
                 describes a request serde refuses, and leaving it out describes a response \
                 serde writes. Use `#[serde(skip)]` to leave it out both ways, or drop \
                 `skip_deserializing`",
            ));
        }
    }
    Ok(())
}

/// `skip_serializing_if` on a field serde still requires on read has no
/// truthful `required`, and neither has `skip_serializing` alone, which is
/// `skip_serializing_if` with a condition that always holds.
///
/// serde may leave such a field out of what it writes, and rejects a document
/// without it on read, so listing it in `required` misdescribes a response and
/// leaving it out misdescribes a request. An `Option`, a field-level
/// `#[serde(default)]` or a struct's container `#[serde(default)]` lets the
/// field be absent both ways, which is what lets [`is_required`] leave it out;
/// this refusal reads that same rule, so the two cannot disagree. A flattened
/// field is decided before that rule, by `#[schema(open)]` alone, because serde
/// ignores any default on it. Only named fields are checked, since only an
/// object has a `required` list; a field serde never reads is in no schema, and a
/// field of a variant serde never writes is only read, where neither key changes
/// anything. A `#[serde(transparent)]` struct is not checked at all: serde writes
/// its one field's value whatever `skip_serializing_if` says, and the schema
/// describing that value has no `required` list to contradict.
pub(super) fn reject_read_required_skip(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent {
        return Ok(());
    }

    let groups: Vec<&Fields> = match &input.data {
        Data::Struct(data) => vec![&data.fields],
        Data::Enum(data) => data
            .variants
            .iter()
            .filter(|variant| is_written(variant))
            .map(|variant| &variant.fields)
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    };

    let named = groups.into_iter().filter_map(|fields| match fields {
        Fields::Named(named) => Some(&named.named),
        Fields::Unnamed(_) | Fields::Unit => None,
    });

    for field in named.flatten().filter(|field| is_described(field)) {
        let Some((key, span)) =
            serde_key_span(&field.attrs, &["skip_serializing_if", "skip_serializing"])
        else {
            continue;
        };

        // A flattened field is decided by `#[schema(open)]` alone, before any
        // default: serde ignores `#[serde(default)]` on a flattened field, at
        // field and container level alike. An open map reads absent as empty
        // and is never listed in `required`, recognised by the same pair
        // `object_body` reads. The field's Rust type is invisible here, and
        // this error aborts expansion before any `Flatten` or `OpenMap`
        // witness is emitted, so one message states a map's remedy and a
        // struct's alike.
        if is_flattened(field) {
            if is_open(field) {
                continue;
            }
            return Err(syn::Error::new(
                span,
                format!(
                    "`{key}` on a flattened field is refused unless it is \
                     `#[schema(open)]`. A flattened map must be `#[schema(open)]` for the schema \
                     to describe it, and may then skip itself, since serde reads it absent as \
                     empty. A flattened struct is written whole or not at all, so drop `{key}` \
                     to keep its members consistent with its schema. `#[serde(default)]` does \
                     not change this on a flattened field"
                ),
            ));
        }

        if !is_required(field, &container) {
            continue;
        }
        return Err(syn::Error::new(
            span,
            format!(
                "`{key}` lets serde leave this field out of what it writes, but without a \
                 `#[serde(default)]` on the field or its struct serde still requires it on \
                 read, so no `required` list is true in both directions. Add \
                 `#[serde(default)]` beside it or on the struct, or make the field an `Option`"
            ),
        ));
    }
    Ok(())
}

/// A member serde leaves out in one direction only has no one position to
/// describe.
///
/// A position is on the wire or not as a whole, and so is a newtype variant's
/// payload, which serde writes as a unit variant without it. So a tuple,
/// tuple-variant or newtype-variant member carrying `skip_serializing` or
/// `skip_deserializing` alone makes serde write one shape and read another.
/// `skip_serializing_if` does the same on a tuple member, except on the last
/// position beside `#[serde(default)]`, which serde fills when the array ends
/// early and [`min_items`](crate::derive::schema::min_items) leaves out of the
/// bound.
///
/// A newtype struct is never checked, since serde ignores all three there, and
/// neither is a newtype variant's `skip_serializing_if`. A
/// `#[serde(transparent)]` struct is described by its one field rather than as
/// an array, and a variant serde skips both ways is in no schema. A variant
/// serde never writes is only read, so there only a lone `skip_deserializing`
/// is refused.
pub(super) fn reject_one_way_member_skip(input: &DeriveInput) -> syn::Result<()> {
    let container = Container::read(input);
    if container.transparent {
        return Ok(());
    }

    let groups: Vec<(&Punctuated<Field, Comma>, bool)> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() > 1 => {
                vec![(&unnamed.unnamed, true)]
            }
            Fields::Named(_) | Fields::Unnamed(_) | Fields::Unit => Vec::new(),
        },
        Data::Enum(data) => described_variants(data)
            .into_iter()
            .filter_map(|variant| match &variant.fields {
                Fields::Unnamed(unnamed) => Some((&unnamed.unnamed, is_written(variant))),
                Fields::Named(_) | Fields::Unit => None,
            })
            .collect(),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => Vec::new(),
    };

    for (members, written) in groups {
        let keys: &[&str] = if written {
            &["skip_serializing", "skip_deserializing"]
        } else {
            &["skip_deserializing"]
        };
        let positions = positional_members(members);
        for (index, field) in positions.iter().enumerate() {
            if let Some((key, span)) = one_way_skip_span(field, keys) {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "`{key}` leaves this member out in one direction only, and a tuple \
                         position or a newtype variant's payload is on the wire or not as a \
                         whole, so serde would write one shape and read another. Use \
                         `#[serde(skip)]` to leave it out both ways, or give the type named \
                         fields"
                    ),
                ));
            }

            if !written {
                continue;
            }
            let Some((_, span)) = serde_key_span(&field.attrs, &["skip_serializing_if"]) else {
                continue;
            };
            // A container default fills the end of a tuple struct as a
            // field-level one fills its own member.
            let defaulted = container.default || serde_flag(&field.attrs, &["default"]);
            let last = index + 1 == positions.len();
            if members.len() == 1 || (last && defaulted) {
                continue;
            }
            return Err(syn::Error::new(
                span,
                "`skip_serializing_if` on a tuple member is refused unless it is the last \
                 described member and carries `#[serde(default)]`. serde leaves the member out \
                 of the array it writes, which moves every later member into its position, and \
                 reads the shorter array back only when a default fills the end. Move the member \
                 last beside `#[serde(default)]`, or give the type named fields",
            ));
        }
    }
    Ok(())
}

/// A newtype variant of an adjacently tagged enum whose member serde skips has
/// no one schema, unless that member is an `Option`.
///
/// serde writes the variant as its tag alone, but reads it by its declared
/// newtype style rather than the unit style it wrote, so it demands the content
/// property and reads only `{"t":"V","c":null}`. An `Option` member reads the
/// missing content as `None`, so it round-trips as the tag-only branch `branch`
/// emits. External and internal tagging read back what they write, and a
/// member skipped one way only is refused before this is reached.
///
/// Checked on every variant the schema describes, including one serde reads
/// and never writes: serde still reads it only with its content, so the
/// tag-only branch would describe a request serde refuses.
pub(super) fn reject_skipped_adjacent_payload(input: &DeriveInput) -> syn::Result<()> {
    let Data::Enum(data) = &input.data else {
        return Ok(());
    };
    let container = Container::read(input);
    let (Some(_), Some(_)) = (&container.tag, &container.content) else {
        return Ok(());
    };

    for variant in described_variants(data) {
        let Fields::Unnamed(unnamed) = &variant.fields else {
            continue;
        };
        let Some(member) = unnamed.unnamed.first() else {
            continue;
        };
        if !is_unit_like(&variant.fields) || is_option(&member.ty) {
            continue;
        }
        let keys = &["skip", "skip_serializing", "skip_deserializing"];
        let Some((key, span)) = serde_key_span(&member.attrs, keys) else {
            continue;
        };
        return Err(syn::Error::new(
            span,
            format!(
                "`{key}` leaves out the only member of a newtype variant in an adjacently \
                 tagged enum, so serde writes the variant as its tag alone, but reads it back \
                 only with its content present, which nothing serde writes carries. Make the \
                 member an `Option`, which serde reads absent, or `#[serde(skip)]` the whole \
                 variant"
            ),
        ));
    }
    Ok(())
}
