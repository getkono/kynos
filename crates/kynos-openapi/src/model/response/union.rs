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

/// The union of two problem responses, or `None` where the two are not a
/// shape this rule reads and the declared one stands.
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
    let schema = union_of(left?, right?)?;
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

/// What one side says about the documents it admits.
enum Admits<'a> {
    /// Every branch constrains `type` to a `const`, so the side is exactly the
    /// documents those branches describe.
    These(Vec<(&'a str, &'a Schema)>),
    /// Every problem document, so the side is a superset of any other.
    Everything,
    /// A shape this rule cannot show to cover the other side.
    ///
    /// Two cases land here and the decision is the same for both: a shape it
    /// does not parse at all, and one it parses as admitting strictly *less*
    /// than a narrowed side -- `false`, an empty `oneOf`, and a `oneOf` whose
    /// branches overlap. Keeping this apart from
    /// [`Everything`](Admits::Everything) is the whole of `union_of`'s
    /// soundness: reading "not narrowed" as "admits everything" is what let a
    /// schema satisfied by nothing be adopted as a status's declaration.
    Unread,
}

/// The schema admitting what both sides admit, or `None` where neither side
/// can be shown to cover the other.
fn union_of(declared: &Schema, incoming: &Schema) -> Option<Schema> {
    match (admits(declared), admits(incoming)) {
        (Admits::These(left), Admits::These(right)) => Some(rebuilt(left, right)),
        // A side admitting everything already admits every document the other
        // describes, so it is the union whichever side carries it.
        (Admits::Everything, _) => Some(declared.clone()),
        (_, Admits::Everything) => Some(incoming.clone()),
        // Nothing here covers the other, so the entry already declared stands
        // -- which is `merge_from`'s rule, and is sound because a status the
        // operation already declares is one it already described.
        (Admits::Unread, _) | (_, Admits::Unread) => None,
    }
}

/// What a side admits, as far as this rule can tell.
///
/// `true` admits every instance outright. A bare `$ref` admits every problem
/// document because of where this runs: the caller has already established
/// that both sides are `application/problem+json`, so the component being
/// referred to is the one every problem document satisfies. A `$ref` carrying
/// *siblings* is not that -- the siblings constrain, and JSON Schema 2020-12
/// applies them alongside the reference -- so it is unread rather than widest.
fn admits(schema: &Schema) -> Admits<'_> {
    let object = match schema {
        Schema::Bool(true) => return Admits::Everything,
        Schema::Bool(false) => return Admits::Unread,
        Schema::Object(object) => object,
    };

    // The reference and nothing else, compared against a schema built to be
    // exactly that -- rather than against a list of keywords this would have to
    // keep in step with `SchemaObject`.
    let bare = SchemaObject {
        reference: object.reference.clone(),
        ..SchemaObject::default()
    };

    if object.reference.is_some() && **object == bare {
        return Admits::Everything;
    }

    match object.one_of.as_deref() {
        // No choice to read, so the schema is one branch.
        None => published(schema).map_or(Admits::Unread, |uri| Admits::These(vec![(uri, schema)])),
        // A choice between nothing is satisfied by nothing, which is `false`
        // written another way rather than a narrowing of anything.
        Some([]) => Admits::Unread,
        // Every branch, or none of them. A `oneOf` where one branch narrows and
        // another does not is *narrower* than either: a document the narrowed
        // branch describes matches the unnarrowed one too, and two matches is
        // what `oneOf` forbids.
        Some(branches) => branches
            .iter()
            .map(|branch| Some((published(branch)?, branch)))
            .collect::<Option<Vec<_>>>()
            .map_or(Admits::Unread, Admits::These),
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
