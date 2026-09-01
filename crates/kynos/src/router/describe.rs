//! Turning an assembled router into a description, or into a service.
//!
//! The other half of [`Router`](super::Router): everything in `mod.rs` puts a
//! router together, and everything here reads the result. `validate` and
//! `openapi` describe it, `build` turns it into something that serves, and
//! `describe` is the one walk both go through.
//!
//! Private, and the split moves no path -- `Router` is still declared in
//! `mod.rs`, and an inherent `impl` may sit in any module of the crate.

use super::install::{
    catches, cors_conflict, error_at, highest_version, install_preflight, invalid,
    lowest_expressing, placeholder_info, pointer_token, unique_tags,
};
use super::{
    Arc, DeclaredTag, Dispatch, Document, EndpointTerminal, Error, HashMap, OperationCx,
    PanicPolicy, PathEntry, PathItem, Paths, Registry, Result, Route, Router, Service, Severity,
    SpecError, SpecVersion, TrailingSlashPolicy, Violation, dispatch,
};

// Each behind the feature that provides it, as `mod.rs` had them.
#[cfg(feature = "docs")]
use super::docs;
#[cfg(feature = "unchecked")]
use super::install::install_unchecked;

impl<C, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Checks the router without building it.
    ///
    /// Returns every violation, including warnings. Worth an integration test:
    /// it catches the mistakes that only show up across a whole API — a
    /// duplicated `operationId`, two paths that differ only in variable name, a
    /// security requirement naming a scheme nobody declared.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] if the router cannot be described at all.
    pub fn validate(&self) -> Result<Vec<Violation>>
    where
        C: 'static,
    {
        let described = self.describe()?;
        Ok(described.violations)
    }

    /// Produces the OpenAPI description, at the lowest version that expresses
    /// this API without loss.
    ///
    /// 3.1 for an API using no 3.2-only construct, and 3.2 for one that does —
    /// a `QUERY` operation, a streamed response, an `in: querystring`
    /// parameter. Lowest rather than highest, because a description a consumer
    /// can read is worth more than one that advertises a version number, and
    /// nothing is lost by saying 3.1 when 3.1 is enough.
    ///
    /// Note that this is *not* decided by the `openapi32` feature. Cargo
    /// unifies features across a dependency graph, so a crate elsewhere in the
    /// build enabling it would otherwise bump the version of a document whose
    /// own API never changed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when validation finds an error-level
    /// violation, so a misleading description is never emitted.
    pub fn openapi(&self) -> Result<Document>
    where
        C: 'static,
    {
        let described = self.describe()?;
        described.into_document()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// Targets, never downgrades: asking for a version that cannot express
    /// this API is an error listing what blocks it, not a document with the
    /// offending operations quietly missing. Reach for this when a consumer's
    /// toolchain pins a version, and let [`openapi`](Router::openapi) decide
    /// otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] on a validation error, or if the API uses a
    /// construct `version` cannot express — a Server-Sent Events response
    /// requested as 3.1, say.
    pub fn openapi_as(&self, version: SpecVersion) -> Result<Document>
    where
        C: 'static,
    {
        let described = self.describe()?;
        described.errors()?;
        described.document.emit(version).map_err(invalid)
    }

    /// Finalizes the router into something servable.
    ///
    /// This is where the structural checks run, so an API that cannot be
    /// described correctly fails at startup rather than at documentation time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] with every violation found.
    pub fn build(self, context: C) -> Result<Service<C>>
    where
        C: Send + Sync + 'static,
    {
        let described = self.describe()?;
        let document = described.into_document()?;

        // After the document exists, and after every violation has been raised.
        // Both orderings are load-bearing: a reference describes the two routes
        // that serve it, so its bytes cannot predate the document -- and an
        // entry `absorb` or `absorb_router` dropped has already failed the
        // build above, so no half of a mount reaches this unpaired.
        #[cfg(feature = "docs")]
        docs::render(&self.mounted, &document)?;

        let mut matcher = matchit::Router::new();
        let mut paths: Vec<PathEntry<C>> = Vec::new();
        let mut index_of: HashMap<String, usize> = HashMap::new();

        for mounted in self.mounted {
            let key = mounted.path.as_str().to_owned();
            let index = if let Some(index) = index_of.get(&key) {
                *index
            } else {
                let index = paths.len();
                matcher.insert(key.clone(), index).map_err(|_| {
                    invalid(SpecError::DuplicatePathTemplate {
                        template: key.clone(),
                        existing: key.clone(),
                    })
                })?;
                paths.push(PathEntry {
                    template: key.clone(),
                    matched: crate::extract::connection::MatchedPath(dispatch::intern(&key)),
                    variables: mounted
                        .path
                        .variables()
                        .iter()
                        .map(|name| dispatch::intern(name))
                        .collect(),
                    allow: dispatch::allow_header(&[]),
                    operations: Vec::new(),
                });
                index_of.insert(key.clone(), index);
                index
            };

            let method = mounted.endpoint.method();
            let operation_id = document
                .paths
                .items
                .get(&key)
                .and_then(|item| item.operation(method))
                .and_then(|operation| operation.operation_id.clone())
                .unwrap_or_default();

            let mut interceptors = self.interceptors.clone();
            interceptors.extend(mounted.interceptors);

            #[cfg(feature = "unchecked")]
            let unchecked_layers = {
                let mut layers = self.unchecked.layers.clone();
                layers.extend(mounted.unchecked_layers);
                layers
            };

            paths[index].operations.push(dispatch::Served {
                method,
                operation_id,
                terminal: Arc::new(EndpointTerminal::new(mounted.endpoint)),
                interceptors,
                catch_panics: mounted.catch_panics || catches::<P>(),
                #[cfg(feature = "unchecked")]
                unchecked_layers,
            });
        }

        #[cfg(feature = "unchecked")]
        install_unchecked(
            &self.unchecked,
            &self.interceptors,
            catches::<P>(),
            &mut matcher,
            &mut paths,
            &mut index_of,
        )?;

        if self.trailing_slashes == TrailingSlashPolicy::Lenient {
            register_flipped_spellings(&mut matcher, &paths);
        }

        for entry in &mut paths {
            let methods: Vec<_> = entry
                .operations
                .iter()
                .map(|operation| operation.method)
                .collect();
            entry.allow = dispatch::allow_header(&methods);
        }

        // After the `Allow` loop, so the synthesized `OPTIONS` is in no `Allow`
        // header, and after `describe` has already run, so it is in no `paths`
        // key either. Both are properties of *when* this happens rather than of
        // a filter someone has to maintain.
        install_preflight(&mut paths, &self.method_not_allowed);

        let dispatch = Arc::new(Dispatch {
            matcher,
            paths,
            context,
            observers: self.observers,
            not_found: self.not_found,
            method_not_allowed: self.method_not_allowed,
            trailing_slashes: self.trailing_slashes,
            trusted_proxies: self.trusted_proxies.clone(),
        });

        Ok(Service::new(document, move |request| {
            let dispatch = Arc::clone(&dispatch);
            async move { dispatch.serve(request).await }
        }))
    }

    /// Assembles the description, and everything found on the way that a
    /// `Describe` implementation had no way to return.
    /// Registers every declared security scheme under `components`.
    ///
    /// A name the specification cannot hold as a component key is a violation
    /// rather than a failure: the rest of the description is still worth
    /// emitting, and `validate` is what decides whether it is usable.
    fn declare_security_schemes(&self, registry: &mut Registry, violations: &mut Vec<Violation>) {
        for (name, scheme) in &self.security_schemes {
            match kynos_openapi::ComponentName::new(*name) {
                Ok(name) => registry.declare_security_scheme(name, scheme.clone()),
                Err(_) => violations.push(error_at(
                    "#/components/securitySchemes",
                    SpecError::InvalidComponentName {
                        name: (*name).to_owned(),
                    },
                )),
            }
        }
    }

    /// Refuses an interceptor configured with a combination it cannot honour.
    ///
    /// Everything else an interceptor says is read from its types, so the
    /// compiler has already checked it. This is the one question about a
    /// *value*, and the only interceptor that has one is `Cors` — see
    /// [`cors_conflict`].
    ///
    /// Called from `describe` rather than `build` so that `validate`,
    /// `openapi`, `openapi_as` and `build` all report it, which is the same
    /// reason `Error::Contribution` is raised there.
    fn refuse_unhonourable_interceptors(&self) -> Result<()>
    where
        C: 'static,
    {
        for interceptor in self.interceptors.iter().chain(
            self.mounted
                .iter()
                .flat_map(|mounted| &mounted.interceptors),
        ) {
            if let Some(conflict) = cors_conflict(interceptor) {
                return Err(Error::Middleware(conflict));
            }
        }

        Ok(())
    }

    fn describe(&self) -> Result<Described>
    where
        C: 'static,
    {
        let mut registry = Registry::new();
        let mut violations = self.violations.clone();

        // Read before anything is described: a configuration that cannot be
        // honoured should not produce a document at all.
        self.refuse_unhonourable_interceptors()?;

        self.declare_security_schemes(&mut registry, &mut violations);

        // Seeded with what this router and everything it absorbed declared at
        // their own scope, so a `tag()` call that covers no operation is still
        // documented; each operation then appends the metadata for the tags
        // that actually landed on it. `unique_tags` keeps the first claim on a
        // name, so an enclosing scope's metadata wins over an operation's --
        // the same rule as before this walk harvested anything.
        let mut tag_metadata = self.tag_metadata.clone();

        let mut paths = Paths::new();
        for mounted in &self.mounted {
            let key = mounted.path.as_str().to_owned();
            let location = format!("#/paths/{}", pointer_token(&key));
            let method = mounted.endpoint.method();

            // The identifier is needed before the operation exists, because it
            // is half of the `Route` an interceptor is described against. A
            // throwaway registry keeps the probe from recording a conflict the
            // real pass is about to record again.
            let operation_id = {
                let mut probe = Registry::new();
                let mut cx = OperationCx::new(&mut probe);
                mounted.endpoint.describe(&mut cx);
                cx.finish().operation_id.unwrap_or_default()
            };
            let route = Route::new(&key, &operation_id, method);

            let mut cx = OperationCx::new(&mut registry);
            mounted.endpoint.describe(&mut cx);

            // The router's own interceptors are outermost, then whatever the
            // group or nested router contributed. The endpoint described itself
            // first, so its own responses win where the two overlap.
            for interceptor in self.interceptors.iter().chain(&mounted.interceptors) {
                interceptor.describe(route, &mut cx);
            }

            for tag in self.tags.iter().chain(&mounted.tags) {
                cx.add_tag(*tag);
            }

            if mounted.catch_panics || catches::<P>() {
                let responses = dispatch::panic_responses(cx.registry());
                cx.add_responses(&responses);
            }

            // Every scope contributes through `add_tag`, so this is the one
            // place that sees what all four of them left on the operation --
            // and the endpoint's own tags are reachable nowhere else, since
            // `Endpoints::push` erased the endpoint before this router saw it.
            tag_metadata.extend(cx.declared_tags().iter().map(DeclaredTag::metadata));

            let operation = cx.finish();

            // A layer of undeclared effect covers this operation, so it stays
            // in `paths` and says it is no longer verified.
            #[cfg(feature = "unchecked")]
            let operation = {
                let mut operation = operation;
                if !self.unchecked.layers.is_empty() || !mounted.unchecked_layers.is_empty() {
                    // The only reachable failure is a marker already present in
                    // a shape Kynos never emits, which an operation Kynos just
                    // described cannot carry.
                    let _ = kynos_openapi::Opaque::new(kynos_openapi::OpaqueReason::UntypedLayer)
                        .apply_to(&mut operation);
                }
                operation
            };

            let item: &mut PathItem = paths.items.entry(key.clone()).or_default();
            if item.set_operation(method, operation).is_some() {
                violations.push(error_at(
                    format!("{location}/{}", method.as_wire_str().to_lowercase()),
                    SpecError::DuplicatePathTemplate {
                        template: format!("{method} {key}"),
                        existing: key,
                    },
                ));
            }
        }

        for check in &self.short_circuit_checks {
            if let Some(error) = check(&mut registry) {
                let violation = error_at("#", error);
                if !violations.contains(&violation) {
                    violations.push(violation);
                }
            }
        }

        if let Some(conflict) = registry.schema_conflicts().first() {
            return Err(Error::Schema(conflict.clone()));
        }
        if let Some(conflict) = registry.scheme_conflicts().first() {
            return Err(Error::Contribution(conflict.clone()));
        }

        let mut document = Document::new(
            highest_version(),
            self.info.clone().unwrap_or_else(placeholder_info),
        );
        document.servers.clone_from(&self.servers);
        document.paths = paths;
        document.tags = unique_tags(&tag_metadata);
        document.components = registry.into_components();

        // The version the description claims follows from what it uses, never
        // from a cargo feature: Cargo unifies features across a dependency
        // graph, so a flag some other crate turned on must not move it.
        let document = lowest_expressing(&document)?;

        // Before validation, because an opaque document that is not stamped is
        // an error the validator is entitled to raise.
        #[cfg(feature = "unchecked")]
        let document = {
            let mut document = document;
            self.unchecked.annotate(&mut document);
            document
        };

        let version = document.spec_version().unwrap_or_default();

        violations.extend(kynos_openapi::validate::Validator::new(version).validate(&document));

        if self.deny_unchecked_schemas {
            for violation in &mut violations {
                if violation.error == SpecError::UncheckedSchema {
                    violation.severity = Severity::Error;
                }
            }
        }

        Ok(Described {
            document,
            violations,
        })
    }
}

/// A described router: the document it produces, and everything wrong with it.
struct Described {
    document: Document,
    violations: Vec<Violation>,
}

impl Described {
    /// Fails when any violation is error-level, so a misleading description is
    /// never emitted.
    fn errors(&self) -> Result<()> {
        let errors: Vec<Violation> = self
            .violations
            .iter()
            .filter(|violation| violation.severity == Severity::Error)
            .cloned()
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid { violations: errors })
        }
    }

    fn into_document(self) -> Result<Document> {
        self.errors()?;
        Ok(self.document)
    }
}

/// Registers the other spelling of every declared path against the entry that
/// declared it, for [`TrailingSlashPolicy::Lenient`].
///
/// Registering the flipped spelling in the match table, rather than answering
/// for it at request time, is what keeps the description exact. It enters the
/// table and nothing else: `paths` still carries one key per declared route,
/// and because both spellings share a [`PathEntry`], `MatchedPath` still
/// reports the declared template, `Allow` is still the one computed from the
/// declared operations, and the synthesized `OPTIONS` still covers both.
///
/// A second pass rather than an insert in the loop above, for two reasons. A
/// flipped spelling added early would occupy the slot a later declared route
/// needs, and a declared spelling has to win over a flipped one -- an
/// application that declares both `/users` and `/users/` keeps two distinct
/// entries. `insert` failing *is* that collision, so discarding the error is
/// the whole of the rule rather than a swallowed failure.
///
/// Catch-alls are skipped. Only `route_unchecked` can put one in the table, and
/// `/assets/{*path}` already matches everything below `/assets/`, so a flipped
/// spelling of it would be redundant at best.
fn register_flipped_spellings<C>(matcher: &mut matchit::Router<usize>, paths: &[PathEntry<C>]) {
    let flipped: Vec<(String, usize)> = paths
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.template.contains("{*"))
        .filter_map(|(index, entry)| {
            dispatch::flip_trailing_slash(&entry.template).map(|spelling| (spelling, index))
        })
        .collect();

    for (spelling, index) in flipped {
        // A collision means the application declared that spelling itself, and
        // what it declared stands.
        let _ = matcher.insert(spelling, index);
    }
}
