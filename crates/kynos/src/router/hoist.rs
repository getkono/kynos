//! Naming each response several operations declare alike once, under
//! `components/responses`, and referring to it from each of them.
//!
//! Called by `describe.rs` after the application's `map_openapi` edits, so an
//! edit reads every response inline, and every component it declares is
//! there to be kept rather than overwritten.

use std::collections::{BTreeMap, HashMap, HashSet};

use kynos_openapi::{ComponentName, Components, PathItem, Paths, Ref, RefOr, Response};

/// A response as declared at one key: the key, then its serialized bytes.
///
/// Ordered by key, then by bytes, so the names it mints follow from what is
/// declared and never from the order the operations were mounted in.
type Declared = (String, String);

/// Replaces each inline response two or more operations declare under the
/// same key with the same bytes by a `$ref` to one `components/responses`
/// entry.
///
/// The entry is a component the application declared with those bytes, where
/// one exists, and otherwise a new one named for the key: `Status404`,
/// `Status4XX`, `Default`, with `_2`, `_3`, ... appended in byte order where
/// two different responses share a key, and past any name already declared
/// or referred to.
/// Bytes, not equality, decide sameness, so two responses differing anywhere
/// (a description, a header, a link, the order of two media types) stay
/// apart. A response given by reference is left as written.
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
    let mut taken = taken(paths, components);
    let mut names: HashMap<Declared, String> = HashMap::new();
    for ((key, bytes), (uses, response)) in declared {
        if uses < 2 {
            continue;
        }
        let name = match existing.get(&bytes) {
            Some(name) => name.clone(),
            // A key no status pattern spells would mint an illegal name; the
            // validator reports the key where it is written instead.
            None if !ComponentName::is_valid(&base(&key)) => continue,
            None => {
                let name = unused(&base(&key), &mut taken);
                components
                    .responses
                    .insert(name.clone(), RefOr::Item(response));
                name
            }
        };
        names.insert((key, bytes), name);
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

/// The name a response declared at `key` is given before any suffix.
fn base(key: &str) -> String {
    if key == "default" {
        "Default".to_owned()
    } else {
        format!("Status{key}")
    }
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

/// `base`, or the first of `base_2`, `base_3`, ... not yet `taken`, which
/// it then takes.
fn unused(base: &str, taken: &mut HashSet<String>) -> String {
    let mut name = base.to_owned();
    let mut ordinal = 1_usize;
    while taken.contains(&name) {
        ordinal += 1;
        name = format!("{base}_{ordinal}");
    }
    taken.insert(name.clone());
    name
}
