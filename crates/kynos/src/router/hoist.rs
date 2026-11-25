//! Naming each response several operations declare alike once, under
//! `components/responses`, and referring to it from each of them.
//!
//! Called by `describe.rs` after the application's `map_openapi` edits, so an
//! edit reads every response inline, and every component it declares is
//! there to be kept rather than overwritten; and after the document is
//! validated, so a violation inside a response is reported at each operation
//! that declares it.

use std::collections::{BTreeMap, HashMap, HashSet};

use kynos_openapi::{
    ComponentName, Components, PathItem, Paths, Ref, RefOr, Response,
    model::body::mime_names::APPLICATION_PROBLEM_JSON,
};
use serde_json::Value;

use crate::error::problem::ABOUT_BLANK;

/// A response as declared at one key: the key, then its serialized bytes.
type Declared = (String, String);

/// Replaces each inline response two or more operations declare under the
/// same key with the same bytes by a `$ref` to one `components/responses`
/// entry.
///
/// The entry is a component the application declared with those bytes, where
/// one exists, and otherwise a new one named for what the response says, as
/// [`derived`] spells it. Where two different responses derive one name, or
/// the name is already declared or referred to, each takes the suffix
/// [`fingerprint`] computes from its own content, so no name depends on what
/// else the document declares beyond that collision; where even that is
/// taken, the response stays inline. Bytes, not equality, decide sameness,
/// so two responses differing anywhere (a description, a header, a link, the
/// order of two media types) stay apart. A response given by reference is
/// left as written.
pub(super) fn shared_responses(paths: &mut Paths, components: &mut Components) {
    let mut declared: BTreeMap<Declared, (usize, Response)> = BTreeMap::new();
    for (key, entry) in entries(paths) {
        if let Some((bytes, response)) = inline(entry) {
            declared
                .entry((key, bytes))
                .or_insert_with(|| (0, response.clone()))
                .0 += 1;
        }
    }

    let existing = existing(components);
    let mut names: HashMap<Declared, String> = HashMap::new();
    let mut minted: BTreeMap<String, Vec<(Declared, Response)>> = BTreeMap::new();
    for ((key, bytes), (uses, response)) in declared {
        if uses < 2 {
            continue;
        }
        if let Some(name) = existing.get(&bytes) {
            names.insert((key, bytes), name.clone());
        } else if let Some(name) = derived(&key, &response) {
            minted
                .entry(name)
                .or_default()
                .push(((key, bytes), response));
        }
        // A key no status pattern spells would derive an illegal name; the
        // validator has reported the key where it is written instead.
    }

    let mut taken = taken(paths, components);
    let mut fresh: BTreeMap<String, (Declared, Response)> = BTreeMap::new();
    for (name, group) in minted {
        let alone = group.len() == 1;
        for (declared, response) in group {
            let name = if alone && !taken.contains(&name) {
                name.clone()
            } else {
                format!("{name}_{}", fingerprint(&response))
            };
            // Taken only where a component or a `$ref` already holds this
            // fingerprinted name, or two fingerprints coincide; sharing is
            // then given up, never the name.
            if taken.insert(name.clone()) {
                fresh.insert(name, (declared, response));
            }
        }
    }
    for (name, (declared, response)) in fresh {
        components
            .responses
            .insert(name.clone(), RefOr::Item(response));
        names.insert(declared, name);
    }

    if names.is_empty() {
        return;
    }
    for (key, entry) in entries(paths) {
        let Some((bytes, _)) = inline(entry) else {
            continue;
        };
        if let Some(name) = names.get(&(key, bytes)) {
            *entry = RefOr::Ref(Ref::response(name));
        }
    }
}

/// Every response entry of every operation in `paths`, with its key, in
/// document order.
fn entries(paths: &mut Paths) -> impl Iterator<Item = (String, &mut RefOr<Response>)> {
    paths
        .items
        .values_mut()
        .flat_map(PathItem::operations_mut)
        .flat_map(|operation| {
            let responses = &mut operation.responses;
            let default = responses
                .default_response
                .as_mut()
                .map(|entry| ("default".to_owned(), entry));
            let keyed = responses
                .responses
                .iter_mut()
                .map(|(key, entry)| (key.clone(), entry));
            default.into_iter().chain(keyed)
        })
}

/// An inline response and its bytes; `None` for a reference, or for one
/// that does not serialize, which is then left where it is.
fn inline(entry: &RefOr<Response>) -> Option<(String, &Response)> {
    let response = entry.as_item()?;
    Some((serde_json::to_string(response).ok()?, response))
}

/// The first name under which `components` declares each inline response.
fn existing(components: &Components) -> HashMap<String, String> {
    let mut existing = HashMap::new();
    for (name, entry) in &components.responses {
        if let Some((bytes, _)) = inline(entry) {
            existing.entry(bytes).or_insert_with(|| name.clone());
        }
    }
    existing
}

/// The name `response`, declared at `key`, is given before any suffix: its
/// problem type, then the schema component its first declared media type
/// carries, then its key, such as `OutOfStockProblemStatus409`,
/// `ProblemStatus500`, `ReadingStatus200`, `Status204` or `Default`.
///
/// `None` where the key would make an illegal name; the schema and problem
/// type are dropped where they would.
fn derived(key: &str, response: &Response) -> Option<String> {
    let status = if key == "default" {
        "Default".to_owned()
    } else {
        format!("Status{key}")
    };
    if !ComponentName::is_valid(&status) {
        return None;
    }
    // The typed map keeps declaration order in every build; a `Value` sorts
    // its keys unless `serde_json/preserve_order` is on.
    let Some((media, media_type)) = response.content.first() else {
        return Some(status);
    };
    // Read by key alone below, so this `Value`'s own key order is moot.
    let media_type = serde_json::to_value(media_type).unwrap_or_default();
    let schema = media_type
        .get("schema")
        .or_else(|| media_type.get("itemSchema"));
    let subject = schema.and_then(subject).unwrap_or_default();
    let problem = if media == APPLICATION_PROBLEM_JSON {
        schema
            .and_then(problem_type)
            .map(pascal)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let name = format!("{problem}{subject}{status}");
    // A schema name holding what a component name may not, percent-encoded
    // in its `$ref`, leaves the key alone to name the response.
    Some(if ComponentName::is_valid(&name) {
        name
    } else {
        status
    })
}

/// The schema component `schema` names, directly, as the first `$ref` of its
/// `allOf`, or as the one every branch of its `oneOf` or `anyOf` names.
fn subject(schema: &Value) -> Option<&str> {
    if let Some(location) = schema.get("$ref").and_then(Value::as_str) {
        return location.strip_prefix("#/components/schemas/");
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        return all_of
            .iter()
            .find_map(|part| part.get("$ref").and_then(Value::as_str))?
            .strip_prefix("#/components/schemas/");
    }
    let branches = branches(schema)?;
    let mut named = branches.iter().map(subject);
    let first = named.next()??;
    named.all(|name| name == Some(first)).then_some(first)
}

/// The one problem type other than `about:blank` that `schema` narrows its
/// `type` member to, through its `allOf` and the branches of a `oneOf` or
/// `anyOf`; `None` where it publishes none or several.
fn problem_type(schema: &Value) -> Option<&str> {
    let mut types = HashSet::new();
    collect_types(schema, &mut types);
    types.remove(ABOUT_BLANK);
    let mut types = types.into_iter();
    let only = types.next()?;
    types.next().is_none().then_some(only)
}

fn collect_types<'s>(schema: &'s Value, types: &mut HashSet<&'s str>) {
    if let Some(narrowed) = schema
        .pointer("/properties/type/const")
        .and_then(Value::as_str)
    {
        types.insert(narrowed);
    }
    let parts = schema.get("allOf").and_then(Value::as_array);
    for part in parts
        .into_iter()
        .flatten()
        .chain(branches(schema).into_iter().flatten())
    {
        collect_types(part, types);
    }
}

fn branches(schema: &Value) -> Option<&Vec<Value>> {
    schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
}

/// The last segment of a type URI, in `PascalCase`: `OutOfStock` for
/// `https://errors.example.com/orders/out-of-stock`.
fn pascal(uri: &str) -> String {
    let segment = uri.rsplit(['/', ':', '#']).next().unwrap_or_default();
    segment
        .split(|c: char| !c.is_ascii_alphanumeric())
        .flat_map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(chars)
        })
        .collect()
}

/// Eight hex digits of the 64-bit FNV-1a hash of `response` as compact JSON
/// with every object's keys sorted, at every depth: fixed by the algorithm
/// rather than by the standard library's hasher, which may change between
/// releases, and by the sort rather than by whether
/// `serde_json/preserve_order` keeps a nested `Value`'s insertion order, so
/// a suffixed name is the same in every build.
fn fingerprint(response: &Response) -> String {
    let mut canonical = serde_json::to_value(response).unwrap_or_default();
    canonical.sort_all_objects();
    let hash = canonical
        .to_string()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("{:08x}", hash >> 32)
}

/// Every name a minted component may not take: each `components` declares,
/// and each a response `$ref` in `paths` names, declared or not, since a
/// component minted under it would answer a reference never meant for it.
fn taken(paths: &mut Paths, components: &Components) -> HashSet<String> {
    let referenced: Vec<String> = entries(paths)
        .filter_map(|(_, entry)| entry.as_ref_object())
        .filter_map(|reference| reference.location.strip_prefix("#/components/responses/"))
        .map(str::to_owned)
        .collect();
    components
        .responses
        .keys()
        .cloned()
        .chain(referenced)
        .collect()
}

#[cfg(test)]
mod tests;
