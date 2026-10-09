//! Turning an assembled router into a description, or into a service.
//!
//! The other half of [`Router`](super::Router): everything in `mod.rs` puts a
//! router together, and everything here reads the result. `validate` and
//! `openapi` describe it, `build` turns it into something that serves, and
//! `describe` is the one walk both go through.

use std::collections::HashSet;

use super::install::{
    catches, cors_conflict, error_at, highest_version, install_preflight, invalid,
    lowest_expressing, placeholder_info, pointer_token, unique_tags,
};
use super::{
    Arc, DeclaredTag, Dispatch, Document, EndpointTerminal, Error, HashMap, OperationCx,
    PanicPolicy, PathEntry, PathItem, Paths, Registry, Result, Route, Router, Service, Severity,
    SpecError, SpecVersion, TrailingSlashPolicy, Violation, dispatch,
};

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
    /// parameter. The `openapi32` feature does not decide it, since Cargo
    /// unifies features across the dependency graph.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when validation finds an error-level
    /// violation, so a misleading description is never emitted. Also returns
    /// it, in every build, for a key without the `x-` prefix in any object's
    /// `extensions`, such as the `Info` given to [`info`](Router::info).
    pub fn openapi(&self) -> Result<Document>
    where
        C: 'static,
    {
        let described = self.describe()?;
        described.into_document()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// Targets, never downgrades: a version that cannot express this API is an
    /// error listing what blocks it. Reach for this when a consumer's toolchain
    /// pins a version, and let [`openapi`](Router::openapi) decide otherwise.
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

        // After the document and every violation: the reference describes its
        // own routes, and a dropped half has already failed the build.
        #[cfg(feature = "docs")]
        let published = docs::render::render(&self.mounted, &document)?;

        let mut matcher = matchit::Router::new();
        let mut paths: Vec<PathEntry<C>> = Vec::new();
        let mut index_of: HashMap<String, usize> = HashMap::new();

        for mounted in self.mounted {
            let key = mounted.path.as_str().to_owned();
            let index = if let Some(index) = index_of.get(&key) {
                *index
            } else {
                let index = paths.len();
                // `TrialTable` already reported any refusal, failing the build.
                matcher
                    .insert(key.clone(), index)
                    .map_err(|error| invalid(match_table_refusal(&key, error)))?;
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
            let described = document
                .paths
                .items
                .get(&key)
                .and_then(|item| item.operation(method));
            let operation_id = described
                .and_then(|operation| operation.operation_id.clone())
                .unwrap_or_default();
            let secured = described.is_some_and(declares_security);

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
                secured,
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

        // Last, so the synthesized `OPTIONS` is in no `Allow`, implemented set
        // or `paths` key.
        let implemented = dispatch::implemented(&paths);
        install_preflight(&mut paths, &self.method_not_allowed, &implemented);

        let dispatch = Arc::new(Dispatch {
            matcher,
            paths,
            context,
            observers: self.observers,
            not_found: self.not_found,
            method_not_allowed: self.method_not_allowed,
            trailing_slashes: self.trailing_slashes,
            trusted_proxies: self.trusted_proxies.clone(),
            implemented,
        });

        let service = Service::new(document, move |request| {
            let dispatch = Arc::clone(&dispatch);
            async move { dispatch.serve(request).await }
        });
        #[cfg(feature = "docs")]
        let service = service.with_published(published);
        Ok(service)
    }

    /// Registers every declared security scheme under `components`; an illegal
    /// name is a violation rather than a failure.
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

    /// Reports every pattern the match table would refuse, so `validate` sees
    /// what `build` would be refused.
    fn try_match_table(&self, violations: &mut Vec<Violation>) {
        let mut table = TrialTable::default();
        for mounted in &self.mounted {
            if let Some(error) = table.insert_template(&mounted.path) {
                violations.push(error_at(
                    format!("#/paths/{}", pointer_token(mounted.path.as_str())),
                    error,
                ));
            }
        }

        // An unchecked pattern has no `paths` key to be located at.
        #[cfg(feature = "unchecked")]
        for route in &self.unchecked.routes {
            if let Some(error) = table.insert(&route.pattern) {
                violations.push(error_at("#", error));
            }
        }
    }

    /// Refuses an interceptor configured with a combination it cannot honour;
    /// today only `Cors` (see [`cors_conflict`]).
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

    /// Assembles the description, plus every violation found on the way.
    fn describe(&self) -> Result<Described>
    where
        C: 'static,
    {
        let mut registry = Registry::new();
        let mut violations = self.violations.clone();

        self.refuse_unhonourable_interceptors()?;

        self.declare_security_schemes(&mut registry, &mut violations);

        // Scope-declared metadata first, so it wins over an operation's under
        // `unique_tags` and a tag covering no operation is still documented.
        let mut tag_metadata = self.tag_metadata.clone();

        let mut paths = Paths::new();
        for mounted in &self.mounted {
            let key = mounted.path.as_str().to_owned();
            let location = format!("#/paths/{}", pointer_token(&key));
            let method = mounted.endpoint.method();

            // The `Route` needs the id before the operation exists; a throwaway
            // registry keeps the probe from recording conflicts twice.
            let operation_id = {
                let mut probe = Registry::new();
                let mut cx = OperationCx::new(&mut probe);
                mounted.endpoint.describe(&mut cx);
                cx.finish().operation_id.unwrap_or_default()
            };
            let route = Route::new(&key, &operation_id, method);

            let mut cx = OperationCx::new(&mut registry);
            mounted.endpoint.describe(&mut cx);

            // Router's interceptors outermost, then the enclosing scopes'; the
            // endpoint described itself first, so its responses win.
            for interceptor in self.interceptors.iter().chain(&mounted.interceptors) {
                interceptor.describe(route, &mut cx);
            }

            for tag in self.tags.iter().chain(&mounted.tags) {
                cx.add_tag(*tag);
            }

            if mounted.catch_panics || catches::<P>() {
                let responses = dispatch::recovery::panic_responses(cx.registry());
                cx.add_responses(&responses);
            }

            // The one place that sees every scope's tags, the endpoint's own
            // included.
            tag_metadata.extend(cx.declared_tags().iter().map(DeclaredTag::metadata));

            let operation = cx.finish();

            // Under an undeclared-effect layer, the operation is marked opaque.
            #[cfg(feature = "unchecked")]
            let operation = {
                let mut operation = operation;
                if !self.unchecked.layers.is_empty() || !mounted.unchecked_layers.is_empty() {
                    // Fails only on a pre-existing marker, which this cannot carry.
                    let _ = kynos_openapi::Opaque::new(kynos_openapi::OpaqueReason::UntypedLayer)
                        .apply_to(&mut operation);
                }
                operation
            };

            let item: &mut PathItem = paths.items.entry(key.clone()).or_default();
            if item.set_operation(method, operation).is_some() {
                violations.push(error_at(
                    format!("{location}/{}", method.as_wire_str().to_lowercase()),
                    SpecError::DuplicateOperation { method, path: key },
                ));
            }
        }

        self.try_match_table(&mut violations);

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

        // From what the document uses, never from a (unified) cargo feature.
        let document = lowest_expressing(&document)?;

        // Before validation, which rejects an unstamped opaque document.
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
/// A pass after every declared path, so a declared spelling always wins.
/// Catch-alls (only `route_unchecked` makes one) are skipped as redundant.
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

/// A dry run of the match table `build` fills, in `build`'s insertion order,
/// which decides which of two conflicting routes is reported.
#[derive(Default)]
struct TrialTable {
    table: matchit::Router<()>,
    held: HashSet<String>,
    shapes: HashSet<String>,
}

impl TrialTable {
    /// Tries a described key, skipping a shape already held (the validator
    /// reports that as `DuplicatePathTemplate`).
    fn insert_template(&mut self, template: &kynos_openapi::PathTemplate) -> Option<SpecError> {
        if self.held.contains(template.as_str()) || !self.shapes.insert(template.normalized()) {
            return None;
        }
        self.insert(template.as_str())
    }

    /// Tries a matching pattern, as `build` inserts it; a pattern already held
    /// is not tried again.
    fn insert(&mut self, pattern: &str) -> Option<SpecError> {
        if !self.held.insert(pattern.to_owned()) {
            return None;
        }
        self.table
            .insert(pattern, ())
            .err()
            .map(|error| match_table_refusal(pattern, error))
    }
}

/// What the match table refusing `pattern` means.
pub(super) fn match_table_refusal(pattern: &str, error: matchit::InsertError) -> SpecError {
    match error {
        matchit::InsertError::Conflict { with } => SpecError::RouteConflict {
            pattern: pattern.to_owned(),
            existing: with,
        },
        _ => SpecError::InvalidRoutePattern {
            pattern: pattern.to_owned(),
        },
    }
}

/// Whether `operation` declares a security requirement: a non-empty list does,
/// even one holding the empty (anonymous) requirement beside a scheme.
fn declares_security(operation: &kynos_openapi::Operation) -> bool {
    operation
        .security
        .as_ref()
        .is_some_and(|requirements| !requirements.is_empty())
}
