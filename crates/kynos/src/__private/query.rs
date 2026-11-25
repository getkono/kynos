//! What a `#[derive(QueryParams)]` group's `parameters` does beyond building
//! each Parameter Object, and the probe by which the `Schema` derive refuses a
//! `#[param(default)]` or `#[param(flatten)]` serde reads otherwise.
//!
//! serde is probed by autoref specialization, as in
//! [`string_form`](super::string_form): `(&Group::<T>::new()).by_serde()`
//! resolves to [`ReadBySerde`] where `T` implements `DeserializeOwned`, so a
//! `QueryString<T>` reads it, and to [`UnreadBySerde`] where it does not.
//! Only the latter implements [`DefaultAgrees`] and [`FlattenAgrees`], so a
//! group no `QueryString<T>` can read, having no serde, compiles.

use std::{fmt::Display, marker::PhantomData};

use kynos_openapi::{Schema as OpenApiSchema, SchemaType, TypeSet};

use crate::{
    router::error::RouterError,
    schema::{constraints::Violations, registry::Registry},
};

/// `schema` stating `value` as its `default`, and `violations`, what checking
/// `value` against the field's bounds found, recorded on `registry` as a
/// [`ParameterDefault`](RouterError::ParameterDefault) naming `group`'s
/// parameter `name` where it is not empty.
pub fn default<T: Display>(
    registry: &mut Registry,
    group: &'static str,
    name: &str,
    schema: OpenApiSchema,
    value: &T,
    violations: Violations,
) -> OpenApiSchema {
    let text = value.to_string();
    if !violations.is_empty() {
        registry.refuse_parameter(RouterError::ParameterDefault {
            group,
            parameter: name.to_owned(),
            default: text.clone(),
            failures: violations.into_failures().into_values().collect(),
        });
    }
    with_default(schema, text)
}

/// Records on `registry`, as a
/// [`DuplicateQueryParameter`](RouterError::DuplicateQueryParameter) naming
/// `group`, each name of `parameters` that two of its members describe:
/// `members` holds each one's range, a member being one of `group`'s own
/// fields or one group it flattens. A name repeated inside one flattened
/// group is that group's to report, from its own `parameters`.
pub fn refuse_repeated(
    registry: &mut Registry,
    group: &'static str,
    parameters: &[kynos_openapi::Parameter],
    members: &[std::ops::Range<usize>],
) {
    for (index, member) in members.iter().enumerate() {
        for parameter in &parameters[member.clone()] {
            let earlier = members[..index].iter().any(|earlier| {
                parameters[earlier.clone()]
                    .iter()
                    .any(|seen| seen.name == parameter.name)
            });
            if earlier {
                registry.refuse_parameter(RouterError::DuplicateQueryParameter {
                    group,
                    parameter: parameter.name.clone(),
                });
            }
        }
    }
}

/// `schema` with `text`, what the parameter's value writes, as its `default`,
/// typed as `schema` types the value: a JSON number where it admits a number,
/// a boolean where it admits one, and the string otherwise, as a `$ref` to a
/// named string type is.
pub(crate) fn with_default(schema: OpenApiSchema, text: String) -> OpenApiSchema {
    // `true` is the empty keyword set, so promoting it loses nothing.
    let mut object = match schema {
        OpenApiSchema::Object(object) => object,
        OpenApiSchema::Bool(_) => Box::default(),
    };
    let admits = |wanted: SchemaType| match &object.ty {
        Some(TypeSet::One(ty)) => *ty == wanted,
        Some(TypeSet::Many(types)) => types.contains(&wanted),
        None => false,
    };
    let numeric = admits(SchemaType::Integer) || admits(SchemaType::Number);
    let boolean = admits(SchemaType::Boolean);
    let typed = match text.as_str() {
        "true" if boolean => serde_json::Value::Bool(true),
        "false" if boolean => serde_json::Value::Bool(false),
        _ if numeric => text.parse::<serde_json::Number>().map_or_else(
            |_| serde_json::Value::String(text),
            serde_json::Value::Number,
        ),
        _ => serde_json::Value::String(text),
    };
    object.default = Some(typed);
    OpenApiSchema::Object(object)
}

/// A query group, whose serde [`ProbeRead`] and [`ProbeUnread`] probe.
pub struct Group<T>(PhantomData<T>);

impl<T> Group<T> {
    /// The probe of `T`.
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Group<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for Group<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Group(..)")
    }
}

/// A group serde reads, so a `QueryString<T>` reads it too.
#[derive(Debug)]
pub struct ReadBySerde;

/// A group serde does not read, so only a `Query<T>` reads it.
#[derive(Debug)]
pub struct UnreadBySerde;

/// Resolves a `DeserializeOwned` group to [`ReadBySerde`].
pub trait ProbeRead {
    /// That serde reads the group.
    fn by_serde(&self) -> ReadBySerde {
        ReadBySerde
    }
}

impl<T: serde::de::DeserializeOwned> ProbeRead for Group<T> {}

/// The fallback of [`ProbeRead`], for a group with no `DeserializeOwned`.
pub trait ProbeUnread {
    /// That serde does not read the group.
    fn by_serde(&self) -> UnreadBySerde {
        UnreadBySerde
    }
}

impl<T> ProbeUnread for &Group<T> {}

/// What a group whose field carries `#[param(default = ...)]` and no serde
/// `default` must be: one serde does not read.
#[diagnostic::on_unimplemented(
    message = "serde requires this field, which `#[param(default = ...)]` fills, so \
               `QueryString<T>` would refuse what `Query<T>` reads",
    label = "serde is not told this default",
    note = "add `#[serde(default = \"...\")]` naming a function returning the same value, or \
            `#[serde(default)]` where the type's `Default` is that value",
    note = "`Schema` checks the keys `#[derive(Deserialize)]` reads, which a hand-written \
            `Deserialize` cannot be told: derive it instead, or drop `Schema`"
)]
pub trait DefaultAgrees {}

impl DefaultAgrees for UnreadBySerde {}

/// What a group whose field carries `#[param(flatten)]` and no serde
/// `flatten` must be: one serde does not read.
#[diagnostic::on_unimplemented(
    message = "serde reads this field as one nested member, which `#[param(flatten)]` reads \
               in its place, so `QueryString<T>` and `Query<T>` would read the group differently",
    label = "serde is not told this flatten",
    note = "add `#[serde(flatten)]` beside `#[param(flatten)]`, so both read the flattened \
            group's members in its place",
    note = "`Schema` checks the keys `#[derive(Deserialize)]` reads, which a hand-written \
            `Deserialize` cannot be told: derive it instead, or drop `Schema`"
)]
pub trait FlattenAgrees {}

impl FlattenAgrees for UnreadBySerde {}

/// Refuses, where `probe` is [`ReadBySerde`], a `#[param(default = ...)]`
/// serde is not told.
pub fn default_agrees<P: DefaultAgrees>(probe: &P) {
    let _ = probe;
}

/// Refuses, where `probe` is [`ReadBySerde`], a `#[param(flatten)]` serde is
/// not told.
pub fn flatten_agrees<P: FlattenAgrees>(probe: &P) {
    let _ = probe;
}
