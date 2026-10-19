//! The private machinery the builder methods call: installing unchecked routes
//! and preflights, and the small functions that shape an emitted document.

use super::{
    Arc, Catch, Document, ErasedInterceptor, Error, FallbackPolicy, HashMap, Info, PanicPolicy,
    PathEntry, Result, RouterError, Severity, SpecError, SpecVersion, Violation,
    describe::match_table_refusal, dispatch,
};
use crate::schema::constraints::Pointer;

/// Adds the routes no path template expresses to the match table.
///
/// # Errors
///
/// Returns [`Error::Invalid`] when the table refuses a pattern, which
/// `describe` has already reported by then.
#[cfg(feature = "unchecked")]
pub(super) fn install_unchecked<C>(
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
/// Returns [`Error::Invalid`] when the match table refuses `key`.
pub(super) fn path_entry<C>(
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
        variables: variables(),
        allow: dispatch::allow_header(&[]),
        operations: Vec::new(),
    });
    index_of.insert(key.to_owned(), index);
    Ok(index)
}

/// The variable names a matching pattern captures; `{*name}` captures `name`,
/// as matchit reports it.
#[cfg(feature = "unchecked")]
pub(super) fn matcher_variables(pattern: &str) -> Vec<&'static str> {
    pattern
        .split('/')
        .filter_map(|segment| {
            let name = segment.strip_prefix('{')?.strip_suffix('}')?;
            let name = name.strip_prefix('*').unwrap_or(name);
            (!name.is_empty()).then(|| dispatch::intern(name))
        })
        .collect()
}

/// Whether `P` selected recovery, by type identity.
pub(super) fn catches<P: PanicPolicy>() -> bool {
    std::any::TypeId::of::<P>() == std::any::TypeId::of::<Catch>()
}

/// Refuses, in the constant evaluation calling it, selecting recovery in a
/// build that cannot unwind: under `panic = "abort"` there is nothing to catch.
#[expect(
    clippy::assertions_on_constants,
    reason = "constant per build by design; a caller's `const` block evaluates it only once recovery is selected"
)]
pub(super) const fn require_unwind() {
    assert!(
        cfg!(panic = "unwind"),
        "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
    );
}

/// The highest version this build can express, which is what a description is
/// assembled at before it is emitted downwards.
pub(super) fn highest_version() -> SpecVersion {
    #[cfg(feature = "openapi32")]
    {
        SpecVersion::V3_2
    }
    #[cfg(not(feature = "openapi32"))]
    {
        SpecVersion::V3_1
    }
}

/// The document at the lowest version expressing it without loss.
///
/// A field the model does not recognise is refused in every build; it never
/// moves the description to 3.2.
pub(super) fn lowest_expressing(document: &Document) -> Result<Document> {
    match document.emit(SpecVersion::V3_1) {
        Ok(emitted) => Ok(emitted),
        #[cfg(feature = "openapi32")]
        Err(_) => {
            let unrecognised = kynos_openapi::emit::downgrade::unrecognised_fields(document);
            if unrecognised.is_empty() {
                document.emit(SpecVersion::V3_2).map_err(invalid)
            } else {
                Err(invalid(SpecError::RequiresV3_2 {
                    blockers: unrecognised,
                }))
            }
        }
        #[cfg(not(feature = "openapi32"))]
        Err(blocked) => Err(invalid(blocked)),
    }
}

/// The visibly placeholder `info` block a router that declared none emits.
pub(super) fn placeholder_info() -> Info {
    Info::new("API", "0.0.0")
}

/// Tag metadata with every repeat of one description dropped.
///
/// Two different descriptions under one name both stay, so validation refuses
/// the document with `DuplicateTag` rather than one being dropped silently.
pub(super) fn unique_tags(declared: &[kynos_openapi::Tag]) -> Vec<kynos_openapi::Tag> {
    let mut tags: Vec<kynos_openapi::Tag> = Vec::new();
    for tag in declared {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    tags
}

/// One `Cors` mounted over a path, and the methods on that path it covers.
/// Borrowed, since `Arc` identity is what tells two configurations apart.
type CoveringCors<'a, C> = (
    &'a Arc<dyn ErasedInterceptor<C>>,
    Vec<kynos_openapi::Method>,
);

/// Registers a preflight answer on every path a `Cors` covers, unless the path
/// declares `OPTIONS` itself.
///
/// The entry has no interceptors: a browser's preflight carries no
/// credentials, so an auth interceptor would break CORS on the whole path.
pub(super) fn install_preflight<C: Send + Sync + 'static>(
    paths: &mut [PathEntry<C>],
    method_not_allowed: &FallbackPolicy,
    implemented: &[kynos_openapi::Method],
) {
    // A plain `OPTIONS` keeps the dispatcher's 405 or 501.
    let options_implemented = implemented.contains(&kynos_openapi::Method::Options);

    for entry in paths {
        if entry
            .operations
            .iter()
            .any(|operation| operation.method == kynos_openapi::Method::Options)
        {
            continue;
        }

        // Each `Cors` covering this path (sibling groups may each mount one),
        // by identity, with only the methods it actually covers.
        let mut scopes: Vec<CoveringCors<'_, C>> = Vec::new();

        for operation in &entry.operations {
            let Some(found) = operation
                .interceptors
                .iter()
                .find(|interceptor| cors_config(interceptor).is_some())
            else {
                continue;
            };

            if let Some((_, covered)) = scopes
                .iter_mut()
                .find(|(mounted, _)| Arc::ptr_eq(mounted, found))
            {
                covered.push(operation.method);
            } else {
                scopes.push((found, vec![operation.method]));
            }
        }

        if scopes.is_empty() {
            continue;
        }

        // An undeclared HEAD runs under the GET's chain, so its scope covers it.
        if !entry
            .operations
            .iter()
            .any(|operation| operation.method == kynos_openapi::Method::Head)
        {
            for (_, covered) in &mut scopes {
                if let Some(at) = covered
                    .iter()
                    .position(|method| *method == kynos_openapi::Method::Get)
                {
                    covered.insert(at + 1, kynos_openapi::Method::Head);
                }
            }
        }

        let scopes = scopes
            .into_iter()
            .map(|(interceptor, covered)| {
                let config = cors_config(interceptor).expect("a recognised CORS interceptor");
                crate::middleware::cors::preflight::Scope::new(config.clone(), covered)
            })
            .collect();

        // Every method the path answers, so a preflight can refuse one that
        // runs under no `Cors` rather than let an override approve it.
        let mut served: Vec<_> = entry
            .operations
            .iter()
            .map(|operation| operation.method)
            .collect();
        if served.contains(&kynos_openapi::Method::Get)
            && !served.contains(&kynos_openapi::Method::Head)
        {
            served.push(kynos_openapi::Method::Head);
        }

        let preflight = crate::middleware::cors::preflight::Preflight::new(
            scopes,
            served,
            options_implemented.then(|| entry.allow.clone()),
            method_not_allowed.clone(),
        );

        entry.operations.push(dispatch::Served {
            method: kynos_openapi::Method::Options,
            operation_id: String::new(),
            terminal: Arc::new(dispatch::PreflightTerminal::new(preflight)),
            interceptors: Vec::new(),
            catch_panics: false,
            secured: false,
            #[cfg(feature = "unchecked")]
            unchecked_layers: Vec::new(),
        });
    }
}

/// The CORS configuration an interceptor carries, if it is one.
pub(super) fn cors_config<C: 'static>(
    interceptor: &Arc<dyn ErasedInterceptor<C>>,
) -> Option<&crate::middleware::cors::CorsConfig> {
    use crate::middleware::cors::{Cors, Documented, Undocumented};

    let value = interceptor.as_any();

    value
        .downcast_ref::<Cors<Undocumented>>()
        .map(Cors::config)
        .or_else(|| value.downcast_ref::<Cors<Documented>>().map(Cors::config))
}

/// The configuration conflict a `Cors` interceptor carries, if any; the one
/// place an interceptor is read as a value.
pub(super) fn cors_conflict<C: 'static>(
    interceptor: &Arc<dyn ErasedInterceptor<C>>,
) -> Option<crate::middleware::MiddlewareError> {
    cors_config(interceptor).and_then(crate::middleware::cors::CorsConfig::conflict)
}

/// Why Kynos will not route a path its model can nonetheless hold: a catch-all,
/// or a segment with two variables (see docs/routing.md).
pub(super) fn unroutable(path: &kynos_openapi::PathTemplate) -> Option<SpecError> {
    let catch_all = path.variables().iter().any(|name| name.starts_with('*'));
    let crowded = path
        .normalized()
        .split('/')
        .any(|segment| segment.matches("{}").count() > 1);

    (catch_all || crowded).then(|| SpecError::OpaqueRoute {
        pattern: path.as_str().to_owned(),
    })
}

/// One error-level violation.
pub(super) fn error_at(
    location: impl Into<String>,
    error: impl Into<RouterError>,
) -> Violation<RouterError> {
    Violation {
        location: location.into(),
        severity: Severity::Error,
        error: error.into(),
    }
}

/// One error-level violation, as the framework error carrying it.
pub(super) fn invalid(error: impl Into<RouterError>) -> Error {
    Error::Invalid {
        violations: vec![error_at("#", error)],
    }
}

/// A validator's violation, in the vocabulary a router reports.
pub(super) fn lift(violation: Violation) -> Violation<RouterError> {
    Violation {
        location: violation.location,
        severity: violation.severity,
        error: RouterError::Spec(violation.error),
    }
}

/// Parses a mount-site path literal, or records why it is malformed in
/// `violations`, located at `#/paths` since it names no key yet.
pub(super) fn parse_or_record(
    path: &str,
    violations: &mut Vec<Violation<RouterError>>,
) -> Option<kynos_openapi::PathTemplate> {
    kynos_openapi::PathTemplate::parse(path)
        .map_err(|reason| {
            violations.push(error_at(
                "#/paths",
                SpecError::InvalidPathTemplate {
                    template: path.to_owned(),
                    reason,
                },
            ));
        })
        .ok()
}

/// Where one `paths` key sits in the document: `#/paths/` and the key as an
/// RFC 6901 reference token.
pub(super) fn path_location(key: &str) -> String {
    let paths = Pointer::root();
    let paths = paths.member("paths");
    format!("#{}", paths.member(key))
}
