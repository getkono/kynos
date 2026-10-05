//! Routes the router declines, and the ones it does not.
//!
//! This closes anti-pattern 3 in
//! [`tests/ui/PENDING.md`](ui/PENDING.md). It is an integration case rather
//! than a `trybuild` pair because the refusal is a *run-time* one with no
//! diagnostic text of its own: `PathTemplate::parse` accepts `{*path}` on
//! purpose, since a variable name is unconstrained in OpenAPI and an externally
//! authored description holding one has to round-trip. The narrower rule
//! belongs where routes are registered, and that is where it is.
//!
//! Each refusal is paired with the control that differs in exactly the property
//! under test, for the reason `PENDING.md` gives: a negative that cannot be
//! paired with a control asserts nothing.

#![cfg(all(feature = "macros", feature = "json"))]

use kynos::{
    Error, Router,
    extract::params::path::Path,
    handler::Handler,
    openapi::{Method as OpenApiMethod, PathTemplate, SpecError, Violation},
    response::status::NoContent,
    router::{endpoint::builder::EndpointBuilder, group::Group},
};

/// What `/files/{path}` captures.
#[derive(kynos::Schema, kynos::PathParams)]
struct FilePath {
    path: String,
}

/// What `/reports/{year}/{month}` captures.
#[derive(kynos::Schema, kynos::PathParams)]
struct ReportPath {
    year: u32,
    month: u32,
}

/// The handler each refused template is mounted with.
///
/// It declares the variables the *accepted* templates name, so a refusal below
/// is the path template being refused and never the handler failing to match
/// it — the two are separate checks and this file is about one of them.
async fn a_file(Path(_): Path<FilePath>) -> NoContent {
    NoContent
}

async fn a_report(Path(_): Path<ReportPath>) -> NoContent {
    NoContent
}

/// Mounts `template` on `handler` under `prefix`, and reports what building
/// said.
macro_rules! mounting {
    ($template:expr, $handler:expr) => {
        Router::<()>::new()
            .mount(EndpointBuilder::<(), _, _>::new(
                OpenApiMethod::Get,
                PathTemplate::parse($template).expect("a parsable path template"),
                $handler,
            ))
            .build(())
            .map(|_| ())
    };
}

/// The `pattern` of the one `OpaqueRoute` a result carries, if that is what it
/// is.
fn opaque_route(result: Result<(), Error>) -> Option<String> {
    match result {
        Err(Error::Invalid { violations }) => sole_opaque_route(&violations),
        _ => None,
    }
}

/// The same, over the violations `validate` reports rather than raises.
fn sole_opaque_route(violations: &[Violation]) -> Option<String> {
    match violations {
        [violation] => match &violation.error {
            SpecError::OpaqueRoute { pattern } => Some(pattern.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// A catch-all matches a set of paths no single template describes, so the
/// description could not say what the service serves.
#[test]
fn a_catch_all_route_is_refused() {
    assert_eq!(
        opaque_route(mounting!("/files/{*path}", a_file)).as_deref(),
        Some("/files/{*path}")
    );
}

/// The control: one variable spanning one segment is the ordinary case.
#[test]
fn a_single_segment_variable_is_accepted() {
    assert!(mounting!("/files/{path}", a_file).is_ok());
}

/// Two variables in one segment is a shape the matcher cannot take apart —
/// nothing says where the first ends.
#[test]
fn two_variables_in_one_segment_are_refused() {
    assert_eq!(
        opaque_route(mounting!("/reports/{year}{month}", a_report)).as_deref(),
        Some("/reports/{year}{month}")
    );
}

/// The control: the same two variables, one segment each.
#[test]
fn two_variables_in_two_segments_are_accepted() {
    assert!(mounting!("/reports/{year}/{month}", a_report).is_ok());
}

/// The refusal survives composition, because a prefix is applied before the
/// check rather than after it.
#[test]
fn a_catch_all_reached_through_a_prefix_is_still_refused() {
    let refused = Router::<()>::new()
        .group(
            Group::<()>::new("/v1").mount(EndpointBuilder::<(), _, _>::new(
                OpenApiMethod::Get,
                PathTemplate::parse("/files/{*path}").expect("a parsable path template"),
                a_file,
            )),
        )
        .build(())
        .map(|_| ());

    assert_eq!(opaque_route(refused).as_deref(), Some("/v1/files/{*path}"));
}

/// Mounts `method template` on `handler`.
fn at<H: Handler<(), A>, A>(
    method: OpenApiMethod,
    template: &str,
    handler: H,
) -> EndpointBuilder<(), H, A> {
    EndpointBuilder::<(), _, _>::new(
        method,
        PathTemplate::parse(template).expect("a parsable path template"),
        handler,
    )
}

/// What `/files/{name}` captures, under a name `FilePath` does not use.
#[derive(kynos::Schema, kynos::PathParams)]
struct FileName {
    name: String,
}

async fn a_named_file(Path(_): Path<FileName>) -> NoContent {
    NoContent
}

/// The violations a refused build carries.
fn refusal(result: Result<(), Error>) -> Vec<Violation> {
    match result {
        Err(Error::Invalid { violations }) => violations,
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(()) => panic!("built"),
    }
}

/// One operation per method and path: a second `GET /files/{path}` would have
/// no `paths` slot of its own, so the build refuses rather than serving one of
/// the two.
///
/// It is reported as the duplicate operation it is, not as two templates
/// differing in variable names: there is one template here, with one variable.
#[test]
fn mounting_one_method_on_one_path_twice_fails_the_build() {
    let violations = refusal(
        Router::<()>::new()
            .mount(at(OpenApiMethod::Get, "/files/{path}", a_file))
            .mount(at(OpenApiMethod::Get, "/files/{path}", a_file))
            .build(())
            .map(|_| ()),
    );

    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert!(
        !matches!(violation.error, SpecError::DuplicatePathTemplate { .. }),
        "{violation}"
    );
    assert_eq!(
        violation.to_string(),
        "error at #/paths/~1files~1{path}/get: operation `GET /files/{path}` is described \
         more than once; a path item holds one operation per method"
    );

    // The control: the same path under a second method is a second operation.
    assert!(
        Router::<()>::new()
            .mount(at(OpenApiMethod::Get, "/files/{path}", a_file))
            .mount(at(OpenApiMethod::Delete, "/files/{path}", a_file))
            .build(())
            .is_ok()
    );
}

/// `/files/{path}` and `/files/{name}` match exactly the same requests, so the
/// second is refused as a duplicate of the first.
#[test]
fn two_templates_differing_only_in_variable_names_fail_the_build() {
    let violations = refusal(
        Router::<()>::new()
            .mount(at(OpenApiMethod::Get, "/files/{path}", a_file))
            .mount(at(OpenApiMethod::Get, "/files/{name}", a_named_file))
            .build(())
            .map(|_| ()),
    );

    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert_eq!(
        violation.error,
        SpecError::DuplicatePathTemplate {
            template: "/files/{name}".to_owned(),
            existing: "/files/{path}".to_owned(),
        }
    );
    assert_eq!(
        violation.to_string(),
        "error at #/paths/~1files~1{name}: path `/files/{name}` is the same path as \
         `/files/{path}`; templates that differ only in variable name are identical"
    );

    // The control: the same two variables behind different literals.
    assert!(
        Router::<()>::new()
            .mount(at(OpenApiMethod::Get, "/files/{path}", a_file))
            .mount(at(OpenApiMethod::Get, "/folders/{name}", a_named_file))
            .build(())
            .is_ok()
    );
}

/// Every entry point that assembles a description sees it, not only `build`.
///
/// A router that `validate` called clean and `build` refused would be a router
/// whose description could be published and never served. `validate` *reports*
/// where the others raise, which is the one difference between them.
#[test]
fn every_entry_point_reports_an_unroutable_path() {
    let router = || {
        Router::<()>::new().mount(EndpointBuilder::<(), _, _>::new(
            OpenApiMethod::Get,
            PathTemplate::parse("/files/{*path}").expect("a parsable path template"),
            a_file,
        ))
    };

    let reported = router().validate().expect("a validatable router");
    assert_eq!(
        sole_opaque_route(&reported).as_deref(),
        Some("/files/{*path}")
    );

    assert_eq!(
        opaque_route(router().openapi().map(|_| ())).as_deref(),
        Some("/files/{*path}")
    );
    assert_eq!(
        opaque_route(router().build(()).map(|_| ())).as_deref(),
        Some("/files/{*path}")
    );
}
