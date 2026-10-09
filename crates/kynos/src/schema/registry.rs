//! Collecting the schemas a description refers to.

use std::collections::HashMap;

use kynos_openapi::{ComponentName, Components, Schema as OpenApiSchema};

use crate::{
    middleware::contribution::ContributionConflict,
    schema::{MapKey, Schema},
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
    /// Identity is [`std::any::type_name`], the only per-type key available
    /// here: [`Schema`] carries no `'static` bound, so [`std::any::TypeId`] is
    /// out of reach. It is what lets the same type resolve twice without being
    /// described twice, and what makes a *second* type claiming the name
    /// describe itself so the two bodies can be compared.
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

    /// Conflicts [`Registry::resolve`] found, which it cannot return.
    conflicts: Vec<SchemaConflict>,

    /// Map key patterns [`Registry::admit_key_pattern`] refused, deduplicated.
    key_patterns: Vec<UnenforceableKeyPattern>,

    /// Conflicts [`Registry::declare_security_scheme`] found, deduplicated.
    ///
    /// A contested scheme name is not a [`SchemaConflict`]: the remedy is a
    /// different `SecurityScheme::NAME`, not a renamed schema.
    scheme_conflicts: Vec<ContributionConflict>,
}

impl Registry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a schema for `T`, registering it if it is named and new.
    ///
    /// This is where naming happens, not in [`Schema::schema`]. A named type is
    /// registered under [`Schema::name`] and the caller gets a `$ref`; an
    /// anonymous one is inlined. Registration precedes the descent into `T`'s
    /// own fields, which is what makes a self-referential type produce a `$ref`
    /// rather than recurse forever.
    ///
    /// A name claimed by two structurally different types is recorded rather
    /// than returned, because this method hands back a schema and a
    /// [`Schema`] implementation has no way to fail; the router reports what
    /// accumulated when it is built.
    ///
    /// # Panics
    ///
    /// When an anonymous type reaches itself with no named type between: a
    /// hand implementation doing so, or generic derived types referring to one
    /// another. Nothing can stand in for an inlined body still being built, so
    /// its description has no end; the alternative is overflowing the stack.
    /// `#[derive(Schema)]` refuses the direct case, a generic type naming
    /// itself, at compile time.
    pub fn resolve<T: Schema>(&mut self) -> OpenApiSchema {
        let Some(name) = T::name() else {
            return self.inline::<T>();
        };

        let key = name.as_str().to_owned();
        let reference = OpenApiSchema::component(&key);
        let origin = std::any::type_name::<T>();

        // Mid-descent into this very type: the body under this name is still
        // being built, so the reference stands in for it. Short-circuiting
        // here is what terminates a cycle.
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
        let schema = T::schema(self);
        let Some(reservation) = self.reserved.pop() else {
            unreachable!("the reservation pushed above is the innermost one");
        };

        // A different type reached a name another descent still holds -- a
        // second `Item` from another module, or `Box<T>` inside `T`, which
        // shares the name and not the `type_name`. Neither can be told apart
        // until the holder's body exists, so the body is kept for it to be
        // compared against then.
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
             named: make the recursive type concrete, or implement `Schema` for it by hand \
             returning a `name()`",
            entry.0
        );

        self.inlining.push(entry);
        let schema = T::schema(self);
        self.inlining.pop();
        schema
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
    /// recorded for [`scheme_conflicts`](Registry::scheme_conflicts) rather
    /// than returned, because a
    /// [`Describe`](crate::extract::describe::Describe) implementation cannot
    /// fail. Keeping the first claim is what leaves every requirement naming it
    /// resolvable while the conflict is reported.
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
    pub(crate) fn key_pattern_refusals(&self) -> &[UnenforceableKeyPattern] {
        &self.key_patterns
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

/// Two different types claimed the same component name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
// The remedy has to name something that exists. `#[derive(Schema)]` takes a
// component name from the Rust type's identifier and offers no attribute to
// override it, so advising one would send a reader looking for a key the
// grammar rejects.
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
