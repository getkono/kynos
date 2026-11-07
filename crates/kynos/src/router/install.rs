//! The private machinery the builder methods, `describe.rs` and `build.rs`
//! share: installing preflights, and the small functions that shape an
//! emitted document.

use super::{
    Arc, Catch, Document, ErasedInterceptor, Error, FallbackPolicy, Info, PanicPolicy, PathEntry,
    Registry, Result, RouterError, Severity, SpecError, SpecVersion, Violation, dispatch,
};
use crate::schema::constraints::Pointer;

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

/// Refuses the first contribution the registry could not reconcile, before
/// its components reach the document.
pub(super) fn refuse_registry_conflicts(registry: &Registry) -> Result<()> {
    // First, since a contested name can leave a component conflict behind.
    if let Some(refused) = registry.instantiation_name_refusals().first() {
        return Err(Error::InstantiationName(Box::new(refused.clone())));
    }
    if let Some(conflict) = registry.schema_conflicts().first() {
        return Err(Error::Schema(conflict.clone()));
    }
    if let Some(refused) = registry.key_pattern_refusals().first() {
        return Err(Error::KeyPattern(Box::new(refused.clone())));
    }
    if let Some(conflict) = registry.scheme_conflicts().first() {
        return Err(Error::Contribution(conflict.clone()));
    }
    Ok(())
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

/// The `Accept-Query` naming the media types of `operation`'s request body,
/// where it is a `QUERY` and some content key can be a member.
#[cfg(feature = "openapi32")]
fn accept_query(
    method: kynos_openapi::Method,
    operation: &kynos_openapi::Operation,
) -> Option<crate::http::HeaderValue> {
    match operation.request_body.as_ref()? {
        kynos_openapi::RefOr::Item(body) if method == kynos_openapi::Method::Query => {
            crate::http::accept_query::from_request_body(body)
        }
        _ => None,
    }
}

/// Records on `entry` the `Accept-Query` its described `QUERY` operation
/// names, and returns it for that operation's terminal; `None` for any other
/// operation, which leaves `entry` alone.
#[cfg(feature = "openapi32")]
pub(super) fn record_accept_query<C>(
    entry: &mut PathEntry<C>,
    method: kynos_openapi::Method,
    described: Option<&kynos_openapi::Operation>,
) -> Option<crate::http::HeaderValue> {
    let value = described.and_then(|operation| accept_query(method, operation))?;
    entry.accept_query = Some(value.clone());
    Some(value)
}

/// `operation`, with the `Accept-Query` its 415 carries (RFC 10008 section
/// 2.1) declared there when it is a `QUERY`.
///
/// Only on a 415 the operation already declares, never minted: the header
/// rides on the body extractor's refusal. Not `required`, since a 415 for a
/// content coding carries none.
#[cfg(feature = "openapi32")]
pub(super) fn declare_accept_query(
    method: kynos_openapi::Method,
    mut operation: kynos_openapi::Operation,
) -> kynos_openapi::Operation {
    use kynos_openapi::RefOr;

    let Some(value) = accept_query(method, &operation) else {
        return operation;
    };
    let Some(RefOr::Item(response)) = operation.responses.responses.get_mut("415") else {
        return operation;
    };

    let example = value.to_str().unwrap_or_default().to_owned();
    response
        .headers
        .entry("Accept-Query".to_owned())
        .or_insert_with(|| {
            RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ))
                .with_description(
                    "The media types this operation accepts as its query, as an RFC 9651 \
                     structured-field List (RFC 10008 section 3); sent when the request's \
                     `Content-Type` is not one of them.",
                )
                .with_example(example),
            )
        });
    operation
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
    // A plain `OPTIONS` keeps the dispatcher's answer: a 405 or 501, or the
    // 200 a path whose `QUERY` names an `Accept-Query` answers.
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
        let preflight = crate::middleware::cors::preflight::Preflight::new(
            scopes,
            dispatch::implemented(std::slice::from_ref(entry)),
            options_implemented.then(|| entry.allow.clone()),
            method_not_allowed.clone(),
        );
        // A plain `OPTIONS` there keeps the answer it had before CORS.
        #[cfg(feature = "openapi32")]
        let preflight = preflight.with_accept_query(entry.accept_query.clone());

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
