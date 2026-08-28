//! The private machinery the builder methods call.
//!
//! Installing a route no path template expresses, installing the preflight
//! handler, and the small functions that shape an emitted document: which spec
//! version to emit, what a missing `info` is filled with, how a tag list is
//! deduplicated, where a violation is located.
//!
//! Nothing here is public, so the split moves no path.

use super::{
    Arc, Catch, Document, ErasedInterceptor, Error, FallbackPolicy, HashMap, Info, PanicPolicy,
    PathEntry, Result, Severity, SpecError, SpecVersion, Violation, dispatch,
};

/// Adds the routes no path template expresses to the match table.
///
/// They reach the same table as every described route — they have to, or they
/// would not serve — and differ from one only in having no `paths` key to have
/// been derived from, and no variables to capture: an unchecked handler takes
/// the whole request and no extractor.
///
/// # Errors
///
/// Returns [`Error::Invalid`] when a pattern collides with one already in the
/// table under a different key.
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
        let key = route.pattern.clone();
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
                // Read from the matching pattern rather than from a
                // `PathTemplate`, which a catch-all is not one of. Leaving this
                // empty made `Dispatch`'s capture branch unreachable, so an
                // unchecked handler had to re-derive what the matcher had
                // already taken apart — including the decoding and the `..`
                // rejection `extract/params/path.rs` keeps private.
                variables: matcher_variables(&key),
                allow: dispatch::allow_header(&[]),
                operations: Vec::new(),
            });
            index_of.insert(key, index);
            index
        };

        let mut unchecked_layers = unchecked.layers.clone();
        unchecked_layers.extend(route.layers.iter().cloned());

        for method in &route.methods {
            paths[index].operations.push(dispatch::Served {
                // Distinct per route and per method, because `Next::route` hands
                // this to every interceptor: an empty string collided every
                // unchecked route into one rate-limit bucket and one metric
                // label. `unchecked:` marks it as synthesized rather than
                // something a document declares, since no document declares it.
                operation_id: format!("unchecked:{} {}", method.as_wire_str(), route.pattern),
                method: *method,
                terminal: Arc::clone(&route.terminal),
                interceptors: interceptors.to_vec(),
                catch_panics,
                unchecked_layers: unchecked_layers.clone(),
            });
        }
    }

    Ok(())
}

/// The variable names a matching pattern captures.
///
/// The router's own syntax rather than a path template: `{name}` captures
/// `name` and `{*name}` captures `name` too, since matchit reports a catch-all
/// under the bare name. A segment that is not a variable captures nothing.
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

/// Whether `P` selected recovery.
///
/// [`PanicPolicy`] is a marker with no members to read, so the policy is
/// resolved by identity — which is exactly as static as the type it comes from.
pub(super) fn catches<P: PanicPolicy>() -> bool {
    std::any::TypeId::of::<P>() == std::any::TypeId::of::<Catch>()
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
/// [`Document::emit`] already knows which constructs block a downgrade, so this
/// asks it rather than repeating the analysis.
pub(super) fn lowest_expressing(document: &Document) -> Result<Document> {
    match document.emit(SpecVersion::V3_1) {
        Ok(emitted) => Ok(emitted),
        #[cfg(feature = "openapi32")]
        Err(_) => document.emit(SpecVersion::V3_2).map_err(invalid),
        #[cfg(not(feature = "openapi32"))]
        Err(blocked) => Err(invalid(blocked)),
    }
}

/// The `info` block a router that declared none still has to emit.
///
/// OpenAPI requires a title and a version, so there is no honest way to omit
/// them; a visible placeholder is better than a plausible invention.
pub(super) fn placeholder_info() -> Info {
    Info::new("API", "0.0.0")
}

/// Tag metadata with the first claim on each name kept.
pub(super) fn unique_tags(declared: &[kynos_openapi::Tag]) -> Vec<kynos_openapi::Tag> {
    let mut tags: Vec<kynos_openapi::Tag> = Vec::new();
    for tag in declared {
        if !tags.iter().any(|existing| existing.name == tag.name) {
            tags.push(tag.clone());
        }
    }
    tags
}

/// Why Kynos will not route a path its model can nonetheless hold.
///
/// Registers a preflight answer on every path a `Cors` covers.
///
/// One `Served` per path rather than a branch in `Dispatch::serve`: a preflight
/// then flows through the machinery that already exists — the matcher finds the
/// path, `position` finds the method — and `Dispatch` needs to hold no CORS
/// configuration of its own.
///
/// Skipped where the path already declares `OPTIONS`. A hand-written operation
/// wins, and it wins by construction rather than by a race in `position`'s
/// linear scan.
///
/// The interceptor list on the synthesized entry is deliberately empty. A
/// browser sends a preflight with no credentials and no `Authorization`, so an
/// auth interceptor short-circuiting it would break CORS for every operation on
/// the path — and `docs/middleware.md` says an interceptor covers the
/// *operations* in its subtree, which a preflight is not. Observers still see
/// it, because they sit outside the chain.
/// One `Cors` mounted over a path, and the methods on that path it covers.
///
/// Borrowed rather than cloned: the identity of the `Arc` is what tells two
/// mounted configurations apart, and a clone of the configuration cannot be
/// compared once one of them can hold a predicate.
type CoveringCors<'a, C> = (
    &'a Arc<dyn ErasedInterceptor<C>>,
    Vec<kynos_openapi::Method>,
);

pub(super) fn install_preflight<C: Send + Sync + 'static>(
    paths: &mut [PathEntry<C>],
    method_not_allowed: &FallbackPolicy,
) {
    for entry in paths {
        if entry
            .operations
            .iter()
            .any(|operation| operation.method == kynos_openapi::Method::Options)
        {
            continue;
        }

        // Every configuration covering this path, and the methods each one
        // covers. An interceptor mounted on a group owning `GET /x` while the
        // router owns `POST /x` advertises `GET` only, which is what keeps
        // preflight and the description agreeing about what exists.
        //
        // More than one is reachable: a group's stack is checked against the
        // router's and never against a sibling's, so two groups may cover one
        // path with a `Cors` each. Grouped by the interceptor's identity, since
        // that is what "the same `Cors`" means once a configuration can hold a
        // predicate no comparison could see through.
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

        let scopes = scopes
            .into_iter()
            .map(|(interceptor, covered)| {
                let config = cors_config(interceptor).expect("a recognised CORS interceptor");
                crate::middleware::cors::preflight::Scope::new(config.clone(), covered)
            })
            .collect();

        let preflight = crate::middleware::cors::preflight::Preflight::new(
            scopes,
            entry.allow.clone(),
            method_not_allowed.clone(),
        );

        entry.operations.push(dispatch::Served {
            method: kynos_openapi::Method::Options,
            operation_id: String::new(),
            terminal: Arc::new(dispatch::PreflightTerminal::new(preflight)),
            interceptors: Vec::new(),
            catch_panics: false,
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

/// The configuration conflict an interceptor carries, if it is one the router
/// recognises and it has one.
///
/// The only place Kynos reads an interceptor as a *value* rather than through
/// its types, and it is deliberately not a capability: the match below is a
/// closed list of two, `Cors`'s state parameter is sealed so there cannot be a
/// third, and a third-party interceptor is never asked. Nothing read here
/// reaches the description.
pub(super) fn cors_conflict<C: 'static>(
    interceptor: &Arc<dyn ErasedInterceptor<C>>,
) -> Option<crate::middleware::MiddlewareError> {
    cors_config(interceptor).and_then(crate::middleware::cors::CorsConfig::conflict)
}

/// The routing contract is narrower than the document model on purpose: a
/// catch-all matches a set of paths no single template describes, and a segment
/// carrying two variables is a shape the matcher cannot take apart. Both checks
/// belong here rather than in `PathTemplate`, which has to round-trip a
/// description it did not produce.
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
pub(super) fn error_at(location: impl Into<String>, error: SpecError) -> Violation {
    Violation {
        location: location.into(),
        severity: Severity::Error,
        error,
    }
}

/// One error-level violation, as the framework error carrying it.
pub(super) fn invalid(error: SpecError) -> Error {
    Error::Invalid {
        violations: vec![error_at("#", error)],
    }
}

/// Escapes one `paths` key for use as a JSON Pointer token, per RFC 6901.
///
/// Every key contains a `/`, so a location embedding one unescaped reads as
/// several tokens and resolves against nothing.
pub(super) fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
