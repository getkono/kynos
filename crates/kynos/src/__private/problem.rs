//! What `#[derive(ApiError)]` declares each status with.
//!
//! Emitted code cannot spell `about:blank`: the URI a problem carrying no
//! semantics of its own uses belongs to [`Problem`], and a second spelling in
//! `kynos-macros` would be a constant nothing holds to the first. So the shape
//! a status's response takes is a function here rather than tokens there, for
//! the reason [`reply`](crate::__private::reply)'s two are.

use kynos_openapi::{
    Map, MediaType, Response, Schema as OpenApiSchema, SchemaObject,
    model::{
        body::mime_names::APPLICATION_PROBLEM_JSON,
        schema::types::{SchemaType, TypeSet},
    },
};
use serde_json::Value;

use crate::{error::problem::Problem, http::StatusCode};

/// One failure answering with a status: the type URI it publishes, and the
/// summary its declaration gave it.
type Branch = (Option<&'static str>, Option<&'static str>);

/// The response one status declares.
///
/// `problem` is the shared component every branch refers to, and `branches` are
/// the failures answering with `status`, in declaration order. The result
/// narrows that component to the type URIs those failures can publish, so a
/// consumer reading the description learns which `type` a body may carry rather
/// than only that it is a problem detail.
///
/// A branch naming no URI narrows to `about:blank`, which is what
/// [`Problem::new`] sets and what the serializer writes. The alternative — a
/// bare `$ref` — would match every problem document and cost a `oneOf` its
/// exactly-one rule.
///
/// `branches` is never empty: a status no failure answers with is not a
/// narrowing of anything, and passing one panics.
#[must_use]
pub fn response(problem: &OpenApiSchema, status: u16, branches: &[Branch]) -> Response {
    // Composed from every branch, not from what survives the URI dedup: the
    // dedup exists to keep a `oneOf` sound, and prose has no such rule.
    let description = description(status, branches);
    let distinct = distinct(status, branches);

    let schema = match distinct.as_slice() {
        // The derive is the only caller and builds this list from the failures
        // that named the status, so it is never empty. A future caller that
        // passes nothing is told so, rather than handed the unnarrowed
        // component back and left to believe it narrowed something.
        [] => unreachable!(
            "a status narrows to the failures answering with it: the derive is the only \
             caller and never passes an empty branch list"
        ),
        // One branch, so a `title` would repeat what the description already
        // says about the only type this status publishes.
        [(uri, _)] => narrowed(problem, uri, None),
        several => object(SchemaObject {
            one_of: Some(
                several
                    .iter()
                    .map(|(uri, summary)| narrowed(problem, uri, *summary))
                    .collect(),
            ),
            ..SchemaObject::default()
        }),
    };

    Response::with_content(
        description,
        APPLICATION_PROBLEM_JSON,
        MediaType::new(schema),
    )
}

/// The schema's branches: resolved and deduplicated by URI in declaration
/// order.
///
/// Two failures may publish one type — the same 404 raised from two call sites
/// — and a `oneOf` repeating a `const` would be satisfied by two branches at
/// once. Where that happens the first summary is the one the surviving branch
/// titles itself with; the description is composed before this and keeps both.
fn distinct(status: u16, branches: &[Branch]) -> Vec<(String, Option<&'static str>)> {
    let mut distinct: Vec<(String, Option<&'static str>)> = Vec::with_capacity(branches.len());

    for (uri, summary) in branches {
        let uri = uri.map_or_else(|| about_blank(status), ToOwned::to_owned);
        if !distinct.iter().any(|(seen, _)| *seen == uri) {
            distinct.push((uri, *summary));
        }
    }

    distinct
}

/// The type URI a failure naming none publishes, read from `Problem` itself.
fn about_blank(status: u16) -> String {
    let status = StatusCode::from_u16(status)
        .expect("`#[derive(ApiError)]` rejects a status outside 400..=599");

    Problem::new(status).type_uri.into_owned()
}

/// The shared component, and the one thing this branch adds to it.
fn narrowed(problem: &OpenApiSchema, uri: &str, summary: Option<&str>) -> OpenApiSchema {
    let mut properties = Map::new();
    properties.insert(
        "type".to_owned(),
        object(SchemaObject {
            ty: Some(TypeSet::One(SchemaType::String)),
            const_value: Some(Value::String(uri.to_owned())),
            ..SchemaObject::default()
        }),
    );

    object(SchemaObject {
        // The summary of the problem *type*, which is what RFC 9457 section
        // 3.1.2 makes `title`. Carried per branch because a `oneOf` is where
        // several of them meet.
        title: summary.map(ToOwned::to_owned),
        all_of: Some(vec![
            problem.clone(),
            object(SchemaObject {
                properties,
                ..SchemaObject::default()
            }),
        ]),
        ..SchemaObject::default()
    })
}

/// The response's description: every summary the status's failures gave, in
/// declaration order and without repeats, falling back to the code's own
/// reason phrase.
///
/// A join rather than the first summary, because a status several failures
/// share has several things to say and a response carries one description.
/// Composed from the branches as declared: two failures publishing one URI are
/// one schema branch but remain two failures, and a reader of the description
/// is owed the name of each. Only an identical summary is dropped, since
/// repeating a sentence tells no one anything.
fn description(status: u16, branches: &[Branch]) -> String {
    let mut summaries: Vec<&str> = Vec::with_capacity(branches.len());
    for (_, summary) in branches {
        match summary {
            Some(summary) if !summaries.contains(summary) => summaries.push(summary),
            _ => {}
        }
    }

    if summaries.is_empty() {
        return StatusCode::from_u16(status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("the request failed")
            .to_owned();
    }

    summaries.join("; ")
}

/// A keyword-carrying schema.
fn object(object: SchemaObject) -> OpenApiSchema {
    OpenApiSchema::Object(Box::new(object))
}
