//! The Discriminator Object's MUSTs, read off each schema the
//! [`schemas`](super::schemas) walk visits.
//!
//! Whether a discriminator beside no composition sits on the parent of the
//! `allOf` form depends on every other schema in the document, so that MUST
//! is judged once the walk is complete.

use std::collections::HashSet;

use crate::{
    model::{
        components::Components,
        document::SpecVersion,
        schema::{Schema, object::SchemaObject},
    },
    validate::violation::{SpecError, Violation},
};

/// What the discriminator rules gather over one walk.
#[derive(Debug, Default)]
pub(in crate::validate) struct Discriminators {
    /// Every `$ref` an `allOf` member names, verbatim.
    extended: HashSet<String>,
    /// Each schema holding a discriminator beside no `oneOf`, `anyOf` or
    /// `allOf`, judged once `extended` is complete.
    uncomposed: Vec<String>,
}

impl Discriminators {
    /// Reads the schema the walk visits at `location`, reporting what it can
    /// judge alone at its `discriminator`.
    pub(in crate::validate) fn visit(
        &mut self,
        location: &str,
        object: &SchemaObject,
        components: &Components,
        version: SpecVersion,
        violations: &mut Vec<Violation>,
    ) {
        self.extended.extend(
            object
                .all_of
                .iter()
                .flatten()
                .filter_map(|member| member.as_object()?.reference.clone()),
        );
        let Some(discriminator) = &object.discriminator else {
            return;
        };
        if object.one_of.is_none() && object.any_of.is_none() && object.all_of.is_none() {
            self.uncomposed.push(location.to_owned());
        }

        let property = &discriminator.property_name;
        if requires(object, property, components, &mut HashSet::new()) {
            return;
        }
        let location = format!("{location}/discriminator");
        if version.supports_3_2() {
            // Optional is legal in 3.2 beside a `defaultMapping`
            // (`references/3.2.0.md:3615`).
            #[cfg(feature = "openapi32")]
            if discriminator.default_mapping.is_none() {
                violations.push(Violation::error(
                    location,
                    SpecError::DiscriminatorWithoutDefaultMapping {
                        property: property.clone(),
                    },
                ));
            }
        } else {
            // `references/3.1.2.md:2834`.
            violations.push(Violation::error(
                location,
                SpecError::DiscriminatorPropertyNotRequired {
                    property: property.clone(),
                },
            ));
        }
    }

    /// Reports each discriminator beside no composition that no `allOf`
    /// member references by its pointer, the parent of the `allOf` form
    /// (`references/3.1.2.md:3345`) being the one such schema that is legal.
    pub(in crate::validate) fn finish(self, violations: &mut Vec<Violation>) {
        for location in self.uncomposed {
            if !self.extended.contains(&location) {
                violations.push(Violation::error(
                    format!("{location}/discriminator"),
                    SpecError::DiscriminatorWithoutComposition,
                ));
            }
        }
    }
}

/// Whether every instance of `object` must hold `property`, or this rule
/// cannot see that it need not.
///
/// Read are `required`, every `allOf` member, every `oneOf` or `anyOf` branch
/// together, both arms of a conditional, and the `#/components/schemas` entry a
/// `$ref` names, looked up as written. A `false` branch admits nothing, so
/// requires anything. A `$dynamicRef`, a reference elsewhere, and one already
/// being `following` could each require it unseen.
fn requires<'doc>(
    object: &'doc SchemaObject,
    property: &str,
    components: &'doc Components,
    following: &mut HashSet<&'doc str>,
) -> bool {
    let schema = |schema: &'doc Schema, following: &mut HashSet<&'doc str>| match schema {
        Schema::Bool(admits) => !admits,
        Schema::Object(object) => requires(object, property, components, following),
    };

    if object
        .required
        .iter()
        .flatten()
        .any(|name| name == property)
        || object.dynamic_ref.is_some()
    {
        return true;
    }
    if let Some(reference) = &object.reference {
        let Some((name, target)) = reference
            .strip_prefix("#/components/schemas/")
            .and_then(|name| components.schemas.get_key_value(name))
        else {
            return true;
        };
        if !following.insert(name) {
            return true;
        }
        let required = schema(target, following);
        following.remove(name.as_str());
        if required {
            return true;
        }
    }

    object
        .all_of
        .iter()
        .flatten()
        .any(|member| schema(member, following))
        || [&object.one_of, &object.any_of]
            .into_iter()
            .flatten()
            .any(|branches| branches.iter().all(|branch| schema(branch, following)))
        || match (&object.if_schema, &object.then_schema, &object.else_schema) {
            (Some(_), Some(then), Some(otherwise)) => {
                schema(then, following) && schema(otherwise, following)
            }
            _ => false,
        }
}
