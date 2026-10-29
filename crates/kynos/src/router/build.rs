//! Turning a described router into a service: filling the match table and
//! installing what serves each entry.
//!
//! The runtime half beside `describe.rs`, which owns the walk this starts from
//! and dry-runs the match table here through `TrialTable`.

use std::collections::HashSet;

use super::install::{catches, install_preflight, invalid};
use super::{
    Arc, Dispatch, EndpointTerminal, HashMap, PanicPolicy, PathEntry, Result, Router, RouterError,
    Service, TrailingSlashPolicy, dispatch,
};

#[cfg(feature = "unchecked")]
use super::ErasedInterceptor;

#[cfg(feature = "docs")]
use super::docs;

impl<C, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Finalizes the router into something servable.
    ///
    /// This is where the structural checks run, so an API that cannot be
    /// described correctly fails at startup rather than at documentation time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) with every violation
    /// found.
    ///
    /// Before any of that, returns what
    /// [describing a router refuses first](crate::Error#what-describing-a-router-refuses-first).
    pub fn build(self, context: C) -> Result<Service<C>>
    where
        C: Send + Sync + 'static,
    {
        let described = self.describe()?;
        let linked = described.linked;
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
            // `TrialTable` already reported any refusal, failing the build.
            let index = path_entry(
                &key,
                || {
                    mounted
                        .path
                        .variables()
                        .iter()
                        .map(|name| dispatch::intern(name))
                        .collect()
                },
                &mut matcher,
                &mut paths,
                &mut index_of,
            )?;

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

        if self.trailing_slashes.unwrap_or_default() == TrailingSlashPolicy::Lenient {
            register_flipped_spellings(&mut matcher, &paths);
        }

        for entry in &mut paths {
            entry.derive_allow();
        }

        // Last, so the synthesized `OPTIONS` is in no `Allow`, implemented set
        // or `paths` key.
        let implemented = dispatch::implemented(&paths);
        let method_not_allowed = self.method_not_allowed.unwrap_or_default();
        install_preflight(&mut paths, &method_not_allowed, &implemented);

        let dispatch = Arc::new(Dispatch {
            matcher,
            // Before `paths`, which moves into the dispatch.
            links: linked.then(|| Arc::new(mounted_links(&paths))),
            paths,
            context,
            observers: self.observers,
            interceptors: self.interceptors,
            catch_panics: catches::<P>(),
            not_found: self.not_found.unwrap_or_default(),
            method_not_allowed,
            trailing_slashes: self.trailing_slashes.unwrap_or_default(),
            trusted_proxies: self.trusted_proxies.unwrap_or_default(),
            implemented,
        });

        // `serve`'s own future, not one awaiting it: a wrapper would box a
        // second copy of the request beside the first.
        let service = Service::new(document, move |request| {
            Arc::clone(&dispatch).serve(request)
        });
        #[cfg(feature = "docs")]
        let service = service.with_published(published);
        Ok(service)
    }
}

/// Adds the routes no path template expresses to the match table.
///
/// # Errors
///
/// Returns [`Error::Invalid`](crate::Error::Invalid) when the table refuses a
/// pattern, which `describe` has already reported by then.
#[cfg(feature = "unchecked")]
fn install_unchecked<C>(
    unchecked: &crate::unchecked::Unchecked<C>,
    interceptors: &[Arc<dyn ErasedInterceptor<C>>],
    catch_panics: bool,
    matcher: &mut matchit::Router<usize>,
    paths: &mut Vec<PathEntry<C>>,
    index_of: &mut HashMap<String, usize>,
) -> Result<()> {
    for route in &unchecked.routes {
        // Variables from the pattern, since a catch-all is no `PathTemplate`.
        let index = path_entry(
            &route.pattern,
            || matcher_variables(&route.pattern),
            matcher,
            paths,
            index_of,
        )?;

        let mut unchecked_layers = unchecked.layers.clone();
        unchecked_layers.extend(route.layers.iter().cloned());

        // The root's own, then those of the routers the route was nested in.
        let mut covering = interceptors.to_vec();
        covering.extend(route.interceptors.iter().cloned());

        for method in &route.methods {
            paths[index].operations.push(dispatch::Served {
                // Distinct per route and method, since interceptors key on it;
                // `unchecked:` marks it as synthesized.
                operation_id: format!("unchecked:{} {}", method.as_wire_str(), route.pattern),
                method: *method,
                terminal: Arc::clone(&route.terminal),
                interceptors: covering.clone(),
                catch_panics: catch_panics || route.catch_panics,
                // Undescribed; an `Authorization` it checks itself still keeps
                // its response out of a cache.
                secured: false,
                unchecked_layers: unchecked_layers.clone(),
            });
        }
    }

    Ok(())
}

/// The index of the entry serving `key`, adding it to the match table first
/// when no earlier operation did, with the variable names `variables` gives.
///
/// # Errors
///
/// Returns [`Error::Invalid`](crate::Error::Invalid) when the match table
/// refuses `key`.
fn path_entry<C>(
    key: &str,
    variables: impl FnOnce() -> Vec<&'static str>,
    matcher: &mut matchit::Router<usize>,
    paths: &mut Vec<PathEntry<C>>,
    index_of: &mut HashMap<String, usize>,
) -> Result<usize> {
    if let Some(index) = index_of.get(key) {
        return Ok(*index);
    }

    let index = paths.len();
    matcher
        .insert(key, index)
        .map_err(|error| invalid(match_table_refusal(key, error)))?;
    paths.push(PathEntry {
        template: key.to_owned(),
        matched: crate::extract::connection::MatchedPath(dispatch::intern(key)),
        // Interned as the names are: leaked once per path a router builds.
        variables: Box::leak(variables().into_boxed_slice()),
        allow: dispatch::allow_header(&[]),
        operations: Vec::new(),
    });
    index_of.insert(key.to_owned(), index);
    Ok(index)
}

/// The variable names a matching pattern captures; `{*name}` captures `name`,
/// as matchit reports it.
#[cfg(feature = "unchecked")]
fn matcher_variables(pattern: &str) -> Vec<&'static str> {
    pattern
        .split('/')
        .filter_map(|segment| {
            let name = segment.strip_prefix('{')?.strip_suffix('}')?;
            let name = name.strip_prefix('*').unwrap_or(name);
            (!name.is_empty()).then(|| dispatch::intern(name))
        })
        .collect()
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
pub(super) struct TrialTable {
    table: matchit::Router<()>,
    held: HashSet<String>,
    shapes: HashSet<String>,
}

impl TrialTable {
    /// Tries a described key, skipping a shape already held (the validator
    /// reports that as `DuplicatePathTemplate`).
    pub(super) fn insert_template(
        &mut self,
        template: &kynos_openapi::PathTemplate,
    ) -> Option<RouterError> {
        if self.held.contains(template.as_str()) || !self.shapes.insert(template.normalized()) {
            return None;
        }
        self.insert(template.as_str())
    }

    /// Tries a matching pattern, as `build` inserts it; a pattern already held
    /// is not tried again.
    pub(super) fn insert(&mut self, pattern: &str) -> Option<RouterError> {
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
fn match_table_refusal(pattern: &str, error: matchit::InsertError) -> RouterError {
    match error {
        matchit::InsertError::Conflict { with } => RouterError::RouteConflict {
            pattern: pattern.to_owned(),
            existing: with,
        },
        _ => RouterError::InvalidRoutePattern {
            pattern: pattern.to_owned(),
        },
    }
}

/// Where each described operation is served, by `operationId`.
///
/// An operation without one, such as a synthesized preflight, is nothing a
/// `Link` can name.
fn mounted_links<C>(paths: &[PathEntry<C>]) -> crate::extract::link::Links {
    let mut links = crate::extract::link::Links::default();
    for entry in paths {
        for operation in &entry.operations {
            if !operation.operation_id.is_empty() {
                links.insert(&operation.operation_id, entry.matched.0);
            }
        }
    }
    links
}

/// Whether `operation` declares a security requirement: a non-empty list does,
/// even one holding the empty (anonymous) requirement beside a scheme.
fn declares_security(operation: &kynos_openapi::Operation) -> bool {
    operation
        .security
        .as_ref()
        .is_some_and(|requirements| !requirements.is_empty())
}
