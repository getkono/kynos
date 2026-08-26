//! Collecting the schemas a description refers to.

use std::collections::{HashMap, HashSet};

use kynos_openapi::{ComponentName, Components, Schema as OpenApiSchema};

use crate::{middleware::contribution::ContributionConflict, schema::Schema};

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

    /// Names claimed by a descent that has not finished.
    ///
    /// A reserved name resolves to a `$ref` whose target does not exist yet,
    /// which is what breaks a cycle.
    reserved: HashSet<String>,

    /// Conflicts [`Registry::resolve`] found, which it cannot return.
    conflicts: Vec<SchemaConflict>,

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
    pub fn resolve<T: Schema>(&mut self) -> OpenApiSchema {
        let Some(name) = T::name() else {
            return T::schema(self);
        };

        let key = name.as_str().to_owned();
        let reference = OpenApiSchema::component(&key);
        let origin = std::any::type_name::<T>();

        // Mid-descent: the body under this name is still being built, so the
        // reference stands in for it. Short-circuiting here is what terminates
        // a cycle, and it is also why a *different* type reaching a name while
        // that name is being defined is aliased rather than compared -- there
        // is nothing to compare it against yet.
        if self.reserved.contains(&key) {
            return reference;
        }

        // Defined, by this very type: the body cannot have changed.
        if self.origins.get(&key).is_some_and(|owner| *owner == origin) {
            return reference;
        }

        self.reserved.insert(key.clone());
        let schema = T::schema(self);
        self.reserved.remove(&key);

        match self.register(&name, schema) {
            Ok(reference) => {
                // First claimant keeps the name, so a second type that agrees
                // structurally does not take ownership of it.
                self.origins.entry(key).or_insert(origin);
                reference
            }
            Err(conflict) => {
                self.conflicts.push(conflict);
                reference
            }
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
