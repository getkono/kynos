//! The rule [`Responses::union_from`] applies to two problem responses meeting
//! on one status.
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
    /// A shape not shown to cover the other side: unparsed, or admitting less
    /// than a narrowed side (`false`, an empty or overlapping `oneOf`). Must not
    /// be read as [`Everything`](Admits::Everything).
    Unread,
}

/// The schema admitting what both sides admit, or `None` where neither side
/// can be shown to cover the other.
fn union_of(declared: &Schema, incoming: &Schema) -> Option<Schema> {
    match (admits(declared), admits(incoming)) {
        (Admits::These(left), Admits::These(right)) => Some(rebuilt(left, right)),
        // A side admitting everything is the union.
        (Admits::Everything, _) => Some(declared.clone()),
        (_, Admits::Everything) => Some(incoming.clone()),
        // Neither covers the other: the declared entry stands, as in `merge_from`.
        (Admits::Unread, _) | (_, Admits::Unread) => None,
    }
}

/// What a side admits, as far as this rule can tell.
///
/// A bare `$ref` admits every problem document, since the caller has already
/// established both sides are `application/problem+json`. A `$ref` with
/// siblings is unread: JSON Schema 2020-12 applies them alongside it.
fn admits(schema: &Schema) -> Admits<'_> {
    let object = match schema {
        Schema::Bool(true) => return Admits::Everything,
        Schema::Bool(false) => return Admits::Unread,
        Schema::Object(object) => object,
    };

    // Compared against a reference-only schema, so no keyword list must track
    // `SchemaObject`.
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
        // An empty choice is satisfied by nothing, like `false`.
        Some([]) => Admits::Unread,
        // Every branch narrows, or the side is unread: a mixed `oneOf` admits
        // less than either branch, since a document matching both fails it.
        Some(branches) => branches
            .iter()
            .map(|branch| Some((published(branch)?, branch)))
            .collect::<Option<Vec<_>>>()
            .map_or(Admits::Unread, Admits::These),
    }
}

/// The `type` a single branch constrains a problem to.
fn published(branch: &Schema) -> Option<&str> {
    narrowing(branch).and_then(constant)
}

/// The `allOf` member of a branch fixing `type` to a `const`, which is where
/// the branch declares the members it sends beside it.
fn narrowing(branch: &Schema) -> Option<&SchemaObject> {
    let Schema::Object(object) = branch else {
        return None;
    };

    object
        .all_of
        .as_ref()?
        .iter()
        .find_map(|member| match member {
            Schema::Object(member) if constant(member).is_some() => Some(&**member),
            _ => None,
        })
}

/// [`narrowing`], to edit.
fn narrowing_mut(branch: &mut Schema) -> Option<&mut SchemaObject> {
    let Schema::Object(object) = branch else {
        return None;
    };

    object
        .all_of
        .as_mut()?
        .iter_mut()
        .find_map(|member| match member {
            Schema::Object(member) if constant(member).is_some() => Some(&mut **member),
            _ => None,
        })
}

/// The URI an object's `type` property is fixed to.
fn constant(object: &SchemaObject) -> Option<&str> {
    let Schema::Object(ty) = object.properties.get("type")? else {
        return None;
    };

    match ty.const_value.as_ref()? {
        Value::String(uri) => Some(uri.as_str()),
        _ => None,
    }
}

/// The two branch lists as one schema: deduplicated by the URI each publishes,
/// in the order they were declared, a repeated URI's members folded into the
/// branch kept.
fn rebuilt(declared: Vec<(&str, &Schema)>, incoming: Vec<(&str, &Schema)>) -> Schema {
    let mut distinct: Vec<(&str, Schema)> = Vec::with_capacity(declared.len() + incoming.len());
    for (uri, branch) in declared.into_iter().chain(incoming) {
        match distinct.iter_mut().find(|(seen, _)| *seen == uri) {
            Some((_, kept)) => fold_members(kept, branch),
            None => distinct.push((uri, branch.clone())),
        }
    }

    if distinct.len() == 1 {
        // One type, so a `oneOf` would be a choice between one thing.
        return distinct.remove(0).1;
    }

    Schema::Object(Box::new(SchemaObject {
        one_of: Some(distinct.into_iter().map(|(_, branch)| branch).collect()),
        ..SchemaObject::default()
    }))
}

/// Folds what `other` sends into `kept`, two branches publishing one URI: the
/// branch kept must match a document either side sends.
///
/// Every member either declares is declared, one the two declare differently
/// admits either schema, and a member is required only where both require it.
/// Where either branch is not a shape this reads, `kept` stands.
fn fold_members(kept: &mut Schema, other: &Schema) {
    let (Some(kept), Some(other)) = (narrowing_mut(kept), narrowing(other)) else {
        return;
    };

    for (name, schema) in &other.properties {
        // The `const` both sides already agree on.
        if name == "type" {
            continue;
        }
        match kept.properties.get_mut(name) {
            None => {
                kept.properties.insert(name.clone(), schema.clone());
            }
            Some(declared) if declared == schema => {}
            Some(declared) => *declared = either(declared, schema),
        }
    }

    let both = other.required.as_deref().unwrap_or_default();
    if let Some(required) = &mut kept.required {
        required.retain(|name| both.contains(name));
    }
    if kept.required.as_ref().is_some_and(Vec::is_empty) {
        kept.required = None;
    }
}

/// A schema admitting what `declared` or `other` does, extending `declared`
/// where it is already a bare `anyOf`.
fn either(declared: &Schema, other: &Schema) -> Schema {
    let mut choices = match declared {
        Schema::Object(object)
            if object.any_of.is_some()
                && **object
                    == SchemaObject {
                        any_of: object.any_of.clone(),
                        ..SchemaObject::default()
                    } =>
        {
            object.any_of.clone().unwrap_or_default()
        }
        _ => vec![declared.clone()],
    };
    if !choices.contains(other) {
        choices.push(other.clone());
    }

    Schema::Object(Box::new(SchemaObject {
        any_of: Some(choices),
        ..SchemaObject::default()
    }))
}

/// Both descriptions, joined and without repeats.
///
/// Split first, since either side may already be a join.
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
