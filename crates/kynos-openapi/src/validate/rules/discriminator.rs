//! The Discriminator Object's MUSTs, read off each schema the
//! [`schemas`](super::schemas) walk visits.
//!
//! Whether a discriminator beside no composition sits on the parent of the
//! `allOf` form depends on every other schema in the document, so that MUST
//! is judged once the walk is complete.

use std::collections::{HashMap, HashSet};

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
        if requires(object, property, components, &mut Judged::default()) {
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
/// requires anything. A `$dynamicRef`, a reference elsewhere, one to a
/// component still being followed, `not`, `dependentSchemas` and
/// `dependentRequired` could each require it unseen.
///
/// `judged` remembers each component's verdict for this `property`. A path
/// reaching a component already judged reuses its verdict, so a component is
/// followed again only after a component it assumed requiring came out
/// optional, which each component does at most once.
fn requires<'doc>(
    object: &'doc SchemaObject,
    property: &str,
    components: &'doc Components,
    judged: &mut Judged<'doc>,
) -> bool {
    let schema = |schema: &'doc Schema, judged: &mut Judged<'doc>| match schema {
        Schema::Bool(admits) => !admits,
        Schema::Object(object) => requires(object, property, components, judged),
    };

    if object
        .required
        .iter()
        .flatten()
        .any(|name| name == property)
        || object.dynamic_ref.is_some()
        || object.not.is_some()
        || !object.dependent_schemas.is_empty()
        || !object.dependent_required.is_empty()
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
        let required = match judged.verdicts.get(name.as_str()) {
            Some(&Verdict::Reached(required)) => required,
            Some(&(Verdict::Following(entered) | Verdict::Assumed(entered))) => {
                judged.lowest_cut = judged.lowest_cut.min(entered);
                true
            }
            None => {
                let entered = judged.entered;
                judged.entered += 1;
                judged.verdicts.insert(name, Verdict::Following(entered));
                let assumed_from = judged.assumed.len();
                let outer_cut = std::mem::replace(&mut judged.lowest_cut, usize::MAX);
                let required = schema(target, judged);
                // `requires` combines verdicts only through `any` and `all`,
                // so a cut assumed requiring can turn `false` into `true` and
                // never the reverse: `false` always holds, and `true` holds
                // once every component it assumed has come out requiring.
                let verdict = if !required {
                    // Each verdict assumed since entering may rest on this
                    // component requiring, which it does not.
                    for assumed in judged.assumed.drain(assumed_from..) {
                        judged.verdicts.remove(assumed);
                    }
                    judged.lowest_cut = usize::MAX;
                    Verdict::Reached(false)
                } else if judged.lowest_cut >= entered {
                    // Nothing outside this component was assumed, and every
                    // component inside it that was came out requiring.
                    for assumed in judged.assumed.drain(assumed_from..) {
                        judged.verdicts.insert(assumed, Verdict::Reached(true));
                    }
                    Verdict::Reached(true)
                } else {
                    judged.assumed.push(name);
                    Verdict::Assumed(entered)
                };
                judged.verdicts.insert(name, verdict);
                judged.lowest_cut = judged.lowest_cut.min(outer_cut);
                required
            }
        };
        if required {
            return true;
        }
    }

    object
        .all_of
        .iter()
        .flatten()
        .any(|member| schema(member, judged))
        || [&object.one_of, &object.any_of]
            .into_iter()
            .flatten()
            .any(|branches| branches.iter().all(|branch| schema(branch, judged)))
        || match (&object.if_schema, &object.then_schema, &object.else_schema) {
            (Some(_), Some(then), Some(otherwise)) => {
                schema(then, judged) && schema(otherwise, judged)
            }
            _ => false,
        }
}

/// The components [`requires`] has followed for one property.
#[derive(Debug)]
struct Judged<'doc> {
    verdicts: HashMap<&'doc str, Verdict>,
    /// How many components have been entered, numbering each in turn.
    entered: usize,
    /// The components whose verdict is [`Verdict::Assumed`], in the order
    /// they finished.
    assumed: Vec<&'doc str>,
    /// The lowest entry number of a component still being followed, or still
    /// assumed, that was reached since the innermost component being followed
    /// was entered.
    lowest_cut: usize,
}

impl Default for Judged<'_> {
    fn default() -> Self {
        Self {
            verdicts: HashMap::new(),
            entered: 0,
            assumed: Vec::new(),
            lowest_cut: usize::MAX,
        }
    }
}

/// What [`requires`] knows of one component, each numbered in the order it
/// was entered.
#[derive(Debug, Clone, Copy)]
enum Verdict {
    /// Being followed: a path reaching it again is cut, and counts as
    /// requiring.
    Following(usize),
    /// Came out requiring by assuming a component entered before it, still
    /// being followed, requires. It holds until that cycle's first component
    /// finishes, which keeps it if that component requires and drops it
    /// otherwise.
    Assumed(usize),
    /// Followed to this verdict, resting on no assumption.
    Reached(bool),
}
