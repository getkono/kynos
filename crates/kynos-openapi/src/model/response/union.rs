//! The rule [`Responses::union_from`] applies to two problem responses meeting
//! on one status.
//!
//! Here rather than beside the method, because the method is one paragraph of
//! dispatch and this is the shape analysis it dispatches to. What the two have
//! in common is the contract, which the method states.
//!
//! [`Responses::union_from`]: crate::model::response::Responses::union_from

use serde_json::Value;

use crate::model::{
    body::mime_names::APPLICATION_PROBLEM_JSON,
    response::Response,
    schema::{Schema, object::SchemaObject},
};

/// The union of two problem responses, or `None` where the two are not that
/// shape and the declared one stands.
pub(super) fn unioned(declared: &Response, incoming: &Response) -> Option<Response> {
    let left = declared
        .content
        .get(APPLICATION_PROBLEM_JSON)?
        .schema
        .as_ref();
    let right = incoming
        .content
        .get(APPLICATION_PROBLEM_JSON)?
        .schema
        .as_ref();
    let schema = union_of(left?, right?);
    let description = joined(
        declared.description.as_deref(),
        incoming.description.as_deref(),
    );

    let mut unioned = declared.clone();
    unioned.description = description;
    if let Some(representation) = unioned.content.get_mut(APPLICATION_PROBLEM_JSON) {
        representation.schema = Some(schema);
    }

    Some(unioned)
}

/// The schema admitting what either side admits.
fn union_of(declared: &Schema, incoming: &Schema) -> Schema {
    match (narrowed(declared), narrowed(incoming)) {
        (Some(left), Some(right)) => rebuilt(left, right),
        // Not narrowed is not a defect: it is the widest thing either side
        // could have said, so it is already the union.
        (None, _) => declared.clone(),
        (_, None) => incoming.clone(),
    }
}

/// The branches of a narrowed problem schema, each with the type URI it
/// publishes, or `None` where the schema narrows nothing.
fn narrowed(schema: &Schema) -> Option<Vec<(&str, &Schema)>> {
    let Schema::Object(object) = schema else {
        return None;
    };

    match object.one_of.as_deref() {
        // No choice to read, so the schema is one branch -- and an empty
        // `oneOf` is the same answer by another route, since it constrains no
        // `type` and `published` says so.
        Some([]) | None => Some(vec![(published(schema)?, schema)]),
        // Every branch, or none of them: a `oneOf` where one branch narrows and
        // another does not is one the unnarrowed branch already satisfies for
        // every problem document, so it narrows nothing as a whole.
        Some(branches) => branches
            .iter()
            .map(|branch| Some((published(branch)?, branch)))
            .collect(),
    }
}

/// The `type` a single branch constrains a problem to.
fn published(branch: &Schema) -> Option<&str> {
    let Schema::Object(object) = branch else {
        return None;
    };

    object.all_of.as_ref()?.iter().find_map(|member| {
        let Schema::Object(member) = member else {
            return None;
        };
        let Schema::Object(ty) = member.properties.get("type")? else {
            return None;
        };

        match ty.const_value.as_ref()? {
            Value::String(uri) => Some(uri.as_str()),
            _ => None,
        }
    })
}

/// The two branch lists as one schema: deduplicated by the URI each publishes,
/// in the order they were declared.
fn rebuilt(declared: Vec<(&str, &Schema)>, incoming: Vec<(&str, &Schema)>) -> Schema {
    let mut distinct: Vec<(&str, &Schema)> = Vec::with_capacity(declared.len() + incoming.len());
    for (uri, branch) in declared.into_iter().chain(incoming) {
        if !distinct.iter().any(|(seen, _)| *seen == uri) {
            distinct.push((uri, branch));
        }
    }

    match distinct.as_slice() {
        // One type, so a `oneOf` would be a choice between one thing.
        [(_, only)] => (*only).clone(),
        several => Schema::Object(Box::new(SchemaObject {
            one_of: Some(
                several
                    .iter()
                    .map(|(_, branch)| (*branch).clone())
                    .collect(),
            ),
            ..SchemaObject::default()
        })),
    }
}

/// Both descriptions, joined and without repeats.
///
/// Split before joining, because either side may already be a join: a status
/// several failures answer with describes itself that way, and a sentence
/// arriving twice tells a reader nothing the once did not.
fn joined(declared: Option<&str>, incoming: Option<&str>) -> Option<String> {
    let mut sentences: Vec<&str> = Vec::new();
    for sentence in declared
        .into_iter()
        .chain(incoming)
        .flat_map(|description| description.split("; "))
    {
        if !sentences.contains(&sentence) {
            sentences.push(sentence);
        }
    }

    (!sentences.is_empty()).then(|| sentences.join("; "))
}
