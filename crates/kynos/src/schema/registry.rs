//! Collecting the schemas a description refers to.

use std::collections::{HashMap, hash_map::Entry};

use kynos_openapi::{ComponentName, Components, Schema as OpenApiSchema};

use crate::{
    middleware::contribution::ContributionConflict,
    schema::{MapKey, NamedInstantiation, Schema},
};

/// Collects the schemas a description refers to.
///
/// Registration is idempotent and cycle-safe: a type that refers to itself
/// registers a placeholder before descending, so a recursive structure produces
/// a `$ref` rather than looping.
#[derive(Debug, Default)]
pub struct Registry {
    /// What the description will carry under `components`.
    components: Components,

    /// Which Rust type defined each component name.
    ///
    /// Identity is [`std::any::type_name`], since [`Schema`] carries no
    /// `'static` bound for [`std::any::TypeId`]. It lets the same type resolve
    /// twice without being described twice.
    origins: HashMap<String, &'static str>,

    /// The descents that have not finished, outermost first.
    ///
    /// A name reserved by the same type resolves to a `$ref` whose target does
    /// not exist yet, which is what breaks a cycle. Identity is the
    /// [`std::any::type_name`] [`origins`](Registry::origins) uses.
    reserved: Vec<Reservation>,

    /// The inlinings that have not finished, outermost first: each anonymous
    /// type's [`std::any::type_name`], beside how many reservations were held
    /// when it began.
    ///
    /// The same type again under the same count reached itself with no named
    /// type between, which nothing can stand in for.
    inlining: Vec<(&'static str, usize)>,

    /// The names [`Registry::name_instantiation`] gave, under the component
    /// name each replaces.
    renamed: HashMap<String, Renamed>,

    /// Instantiation names [`Registry::name_instantiation`] refused,
    /// deduplicated.
    instantiation_names: Vec<InstantiationNameRefusal>,

    /// Conflicts [`Registry::resolve`] found, which it cannot return.
    conflicts: Vec<SchemaConflict>,

    /// Map key patterns [`Registry::admit_key_pattern`] refused, deduplicated.
    key_patterns: Vec<UnenforceableKeyPattern>,

    /// Conflicts [`Registry::declare_security_scheme`] found, deduplicated.
    ///
    /// A contested scheme name is not a [`SchemaConflict`]: the remedy is a
    /// different `SecurityScheme::NAME`, not a renamed schema.
    scheme_conflicts: Vec<ContributionConflict>,

    /// The extension members
    /// [`Router::problem_members`](crate::router::Router::problem_members)
    /// adds to every problem Kynos renders itself: each one's name, schema, and
    /// whether every problem sends it. Read by each such problem's response.
    problem_members: Vec<(String, OpenApiSchema, bool)>,
}

impl Registry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a schema for `T`, registering it if it is named and new.
    ///
    /// A named type is registered under [`Schema::name`] and the caller gets a
    /// `$ref`; an anonymous one is inlined. The name is reserved before the
    /// descent into `T`'s own fields and registered once it finishes, so a
    /// self-referential type reaching its reservation produces a `$ref`.
    ///
    /// A name claimed by two structurally different types is recorded rather
    /// than returned; the router reports what accumulated when it is built.
    ///
    /// # Panics
    ///
    /// When an anonymous type reaches itself with no named type between: a
    /// hand implementation doing so, or inlined instantiations of generic
    /// derived types referring to one another, whose description would have no
    /// end; named instantiations are `$ref`s to each other. `#[derive(Schema)]`
    /// refuses the direct case, a generic type naming itself, at compile time.
    ///
    /// A name [`Router::name_instantiation`](crate::router::Router::name_instantiation)
    /// declared replaces the one `T` returns, and its description the body's
    /// own.
    pub fn resolve<T: Schema>(&mut self) -> OpenApiSchema {
        let Some(name) = T::name() else {
            return self.inline::<T>();
        };
        let (name, description) = match self.renamed.get(name.as_str()) {
            Some(renamed) => (renamed.name.clone(), renamed.description),
            None => (name, None),
        };

        let key = name.as_str().to_owned();
        let reference = OpenApiSchema::component(&key);
        let origin = std::any::type_name::<T>();

        // Mid-descent into this very type: the reference stands in for the
        // body still being built, which terminates a cycle.
        if self
            .reserved
            .iter()
            .any(|reservation| reservation.name == key && reservation.origin == origin)
        {
            return reference;
        }

        // Defined, by this very type: the body cannot have changed.
        if self.origins.get(&key).is_some_and(|owner| *owner == origin) {
            return reference;
        }

        self.reserved.push(Reservation {
            name: key.clone(),
            origin,
            rivals: Vec::new(),
        });
        let mut schema = T::schema(self);
        if let (Some(description), OpenApiSchema::Object(keywords)) = (description, &mut schema) {
            keywords.description = Some(description.to_owned());
        }
        let Some(reservation) = self.reserved.pop() else {
            unreachable!("the reservation pushed above is the innermost one");
        };

        // A different type (another `Item`, or `Box<T>` inside `T`) reached a
        // name another descent holds; keep the body to compare once it exists.
        if let Some(holder) = self
            .reserved
            .iter_mut()
            .find(|reservation| reservation.name == key)
        {
            // Rivals file on the outermost reservation of a name, so one with
            // a holder beneath it never collected any.
            debug_assert!(reservation.rivals.is_empty());
            holder.rivals.push(schema);
            return reference;
        }

        match self.register(&name, schema) {
            // First claimant keeps the name, so a second type that agrees
            // structurally does not take ownership of it.
            Ok(_) => _ = self.origins.entry(key).or_insert(origin),
            Err(conflict) => self.conflicts.push(conflict),
        }
        for rival in reservation.rivals {
            if let Err(conflict) = self.register(&name, rival) {
                self.conflicts.push(conflict);
            }
        }
        reference
    }

    /// Describes an anonymous `T` in place.
    ///
    /// Between two inlinings of one type at the same reservation count, every
    /// enclosing descent is an inlining too, and an inlining always descends,
    /// so the inner one would repeat the outer one without end.
    fn inline<T: Schema>(&mut self) -> OpenApiSchema {
        let entry = (std::any::type_name::<T>(), self.reserved.len());
        assert!(
            !self.inlining.contains(&entry),
            "`{}` refers to itself through no type with a component name, so its schema \
             would never end. A generic `#[derive(Schema)]` type is inlined rather than \
             named where one of its type arguments has no component name: make the \
             recursive type concrete, or implement `Schema` for it by hand returning a \
             `name()`",
            entry.0
        );

        self.inlining.push(entry);
        let schema = T::schema(self);
        self.inlining.pop();
        schema
    }

    /// Publishes `A`'s instantiation under [`A::NAME`](NamedInstantiation::NAME)
    /// wherever [`resolve`](Registry::resolve) would have used the component
    /// name `A` mangles, carrying [`A::DESCRIPTION`](NamedInstantiation::DESCRIPTION).
    ///
    /// Takes effect for what is resolved after it, so it precedes every
    /// operation's description. Idempotent for one name; a refusal is recorded
    /// rather than returned, for `resolve`'s reason, and the router reports it
    /// when it is built.
    pub(crate) fn name_instantiation<A: NamedInstantiation>(&mut self) {
        let instantiation = std::any::type_name::<A>();
        let refusal = match (ComponentName::new(A::NAME), A::name()) {
            (Err(_), _) => InstantiationNameRefusal::Invalid {
                instantiation: instantiation.to_owned(),
                name: A::NAME.to_owned(),
            },
            (Ok(_), None) => InstantiationNameRefusal::Anonymous {
                instantiation: instantiation.to_owned(),
                name: A::NAME.to_owned(),
            },
            (Ok(name), Some(mangled)) => match self.renamed.entry(mangled.as_str().to_owned()) {
                Entry::Vacant(vacant) => {
                    vacant.insert(Renamed {
                        name,
                        description: A::DESCRIPTION,
                    });
                    return;
                }
                Entry::Occupied(named) if named.get().name == name => return,
                Entry::Occupied(named) => InstantiationNameRefusal::Contested {
                    instantiation: mangled.as_str().to_owned(),
                    first: named.get().name.as_str().to_owned(),
                    second: name.as_str().to_owned(),
                },
            },
        };
        if !self.instantiation_names.contains(&refusal) {
            self.instantiation_names.push(refusal);
        }
    }

    /// Registers a schema under an explicit name and returns a `$ref` to it.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaConflict`] when `name` is already registered to a
    /// structurally different schema, which is how two distinct Rust types that
    /// mangle to the same component name are caught.
    pub fn register(
        &mut self,
        name: &ComponentName,
        schema: OpenApiSchema,
    ) -> Result<OpenApiSchema, SchemaConflict> {
        if let Some(registered) = self.components.schemas.get(name.as_str()) {
            if *registered != schema {
                return Err(SchemaConflict {
                    name: name.as_str().to_owned(),
                });
            }
            return Ok(OpenApiSchema::component(name.as_str()));
        }

        Ok(self.components.insert_schema(name, schema))
    }

    /// Registers a security scheme, keeping the first claim on a contested
    /// name.
    ///
    /// Idempotent for the same scheme; a different scheme under one name is
    /// recorded for [`scheme_conflicts`](Registry::scheme_conflicts), since
    /// `Describe` cannot fail. Keeping the first claim leaves every requirement
    /// naming it resolvable.
    pub(crate) fn declare_security_scheme(
        &mut self,
        name: ComponentName,
        scheme: kynos_openapi::SecurityScheme,
    ) {
        let Some(declared) = self.components.security_schemes.get(name.as_str()) else {
            self.components.insert_security_scheme(&name, scheme);
            return;
        };

        if declared.as_item() == Some(&scheme) {
            return;
        }

        let conflict = ContributionConflict::SecurityScheme { name };
        if !self.scheme_conflicts.contains(&conflict) {
            self.scheme_conflicts.push(conflict);
        }
    }

    /// Records `K`'s key `pattern` if no check could enforce it: one that does
    /// not translate from ECMA-262, or any pattern without the `pattern`
    /// feature.
    ///
    /// Recorded rather than returned for [`resolve`](Registry::resolve)'s
    /// reason, and refused when the router is built rather than at a request,
    /// where a pattern that does not translate could only be a failure of the
    /// server. Translating it here also compiles it for the first request.
    pub(crate) fn admit_key_pattern<K: MapKey>(&mut self, pattern: &str) {
        #[cfg(feature = "pattern")]
        let refusal = crate::__private::constraints::pattern::key(pattern).err();
        #[cfg(not(feature = "pattern"))]
        let refusal = Some(
            "`kynos` enforces a pattern with a regular expression engine, which it compiles \
             in only under its `pattern` feature. Enable it, or drop the pattern: a bound \
             nothing checks would be described as though something did"
                .to_owned(),
        );

        let Some(reason) = refusal else {
            return;
        };
        let refused = UnenforceableKeyPattern {
            key: std::any::type_name::<K>().to_owned(),
            pattern: pattern.to_owned(),
            reason,
        };
        if !self.key_patterns.contains(&refused) {
            self.key_patterns.push(refused);
        }
    }

    /// Every key pattern [`admit_key_pattern`](Registry::admit_key_pattern)
    /// refused, in the order it found them.
    /// Declares `members` on every problem Kynos renders itself, before any
    /// operation is described.
    pub(crate) fn declare_problem_members(&mut self, members: Vec<(String, OpenApiSchema, bool)>) {
        self.problem_members = members;
    }

    /// What [`declare_problem_members`](Self::declare_problem_members)
    /// declared; empty where nothing did.
    pub(crate) fn problem_members(&self) -> &[(String, OpenApiSchema, bool)] {
        &self.problem_members
    }

    pub(crate) fn key_pattern_refusals(&self) -> &[UnenforceableKeyPattern] {
        &self.key_patterns
    }

    /// Every name [`name_instantiation`](Registry::name_instantiation)
    /// refused, in the order it found them.
    pub(crate) fn instantiation_name_refusals(&self) -> &[InstantiationNameRefusal] {
        &self.instantiation_names
    }

    /// Every conflict [`resolve`](Registry::resolve) discovered, in the order
    /// it found them.
    pub(crate) fn schema_conflicts(&self) -> &[SchemaConflict] {
        &self.conflicts
    }

    /// Every conflict [`declare_security_scheme`](Registry::declare_security_scheme)
    /// found.
    pub(crate) fn scheme_conflicts(&self) -> &[ContributionConflict] {
        &self.scheme_conflicts
    }

    /// Consumes the registry, yielding the components to embed in the document.
    #[must_use]
    pub fn into_components(self) -> Components {
        self.components
    }
}

/// A component name held by a descent that has not finished.
#[derive(Debug)]
struct Reservation {
    /// The component name.
    name: String,

    /// The [`std::any::type_name`] of the type being described under it.
    origin: &'static str,

    /// Bodies other types described under the same name meanwhile, compared
    /// against this one once it is registered.
    rivals: Vec<OpenApiSchema>,
}

/// A name [`NamedInstantiation`] gave one instantiation.
#[derive(Debug)]
struct Renamed {
    /// The component name published in place of the mangled one.
    name: ComponentName,

    /// The component's description, in place of the body's own.
    description: Option<&'static str>,
}

/// A [`NamedInstantiation`] the registry could not honour.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InstantiationNameRefusal {
    /// Two names were given to one component: two types sharing a mangled
    /// name, such as an instantiation and a `Box` of it, each named.
    #[error(
        "the instantiation published as `{instantiation}` is named both `{first}` and \
         `{second}`; a component has one name, so drop one `name_instantiation!`"
    )]
    Contested {
        /// The mangled component name both replace.
        instantiation: String,

        /// The name registered first.
        first: String,

        /// The name registered second.
        second: String,
    },

    /// The type has no component name to replace: it is inlined wherever it
    /// is used, as an instantiation over a type argument with no name of its
    /// own is.
    #[error(
        "`{instantiation}` is inlined, so it has no component name for `{name}` to replace; \
         name its type arguments, or give it a hand `Schema` implementation returning a \
         `name()`"
    )]
    Anonymous {
        /// The instantiation, as [`std::any::type_name`] spells it.
        instantiation: String,

        /// The name it was given.
        name: String,
    },

    /// The name is not a legal component name, which matches
    /// `^[a-zA-Z0-9._-]+$`.
    #[error(
        "`{name}`, given to `{instantiation}`, is not a component name, which is ASCII \
         letters, digits, `.`, `-` and `_`"
    )]
    Invalid {
        /// The instantiation, as [`std::any::type_name`] spells it.
        instantiation: String,

        /// The name it was given.
        name: String,
    },
}

/// Two different types claimed the same component name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
// `#[derive(Schema)]` has no attribute overriding the component name, so the
// remedy must not suggest one.
#[error(
    "component name `{name}` is claimed by two structurally different schemas; \
     rename one of the Rust types, or implement `Schema` by hand for one and \
     return a different `name()`"
)]
pub struct SchemaConflict {
    /// The contested component name.
    pub name: String,
}

/// A map key type declared a `pattern` in
/// [`MapKey::key_constraints`] that no check could enforce.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the map key `{key}` declares the pattern `{pattern}`, which cannot be enforced: {reason}")]
pub struct UnenforceableKeyPattern {
    /// The key type, as [`std::any::type_name`] spells it.
    pub key: String,

    /// The pattern as declared.
    pub pattern: String,

    /// Why no check could enforce it.
    pub reason: String,
}

#[cfg(test)]
mod tests;
