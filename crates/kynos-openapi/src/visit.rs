//! One walk over every Path Item and Operation a document describes, each at
//! the pointer it is written at.
//!
//! A Path Item is reached under `paths`, `webhooks`, `components.pathItems`,
//! or a callback expression; a callback is reached under
//! `components.callbacks` or an operation's `callbacks`. A `$ref` is never
//! followed, so each object is visited once, where it is written, and no cycle
//! guard is needed.
//!
//! Pointers are [`At`] chains on the stack, so a visitor that reports nothing
//! allocates nothing.

use crate::{
    model::{
        callback::Callback,
        document::Document,
        paths::{item::PathItem, method::Method, operation::Operation},
    },
    pointer::At,
};

/// What a walk does at each object it reaches. Every hook defaults to nothing.
///
/// The order is fixed: a Path Item, then each operation it holds in
/// [`Method::all`] order, then each of its 3.2 `additionalOperations` in
/// insertion order. Each operation's hooks bracket the callbacks it declares.
pub(crate) trait Visit<'doc> {
    /// A Path Item, before its operations.
    ///
    /// `path` is its `paths` key, where it is held there. Returning `false`
    /// skips its operations and everything below them.
    fn path_item(&mut self, _at: At<'_>, _item: &'doc PathItem, _path: Option<&'doc str>) -> bool {
        true
    }

    /// An operation, with the Path Item holding it, before its callbacks.
    fn operation(&mut self, _at: At<'_>, _item: &'doc PathItem, _operation: &'doc Operation) {}

    /// The same operation, once its callbacks are walked.
    fn operation_walked(&mut self, _at: At<'_>, _operation: &'doc Operation) {}

    /// One entry of a callback, before the Path Item it holds, which is
    /// walked only where it is written inline.
    fn callback_entry(&mut self, _at: At<'_>, _expression: &'doc str) {}
}

/// Every Path Item in `document`: [`routes`], then `components.pathItems`,
/// then `components.callbacks`.
pub(crate) fn document<'doc>(document: &'doc Document, visit: &mut impl Visit<'doc>) {
    routes(document, visit);

    let components = At::ROOT.key("components");
    let path_items = components.key("pathItems");
    for (name, item) in &document.components.path_items {
        path_item(path_items.key(name), item, visit);
    }
    let callbacks = components.key("callbacks");
    for (name, entry) in &document.components.callbacks {
        if let Some(entry) = entry.as_item() {
            callback(callbacks.key(name), entry, visit);
        }
    }
}

/// The Path Items under `paths`, then those under `webhooks`.
pub(crate) fn routes<'doc>(document: &'doc Document, visit: &mut impl Visit<'doc>) {
    let paths = At::ROOT.key("paths");
    for (raw, item) in &document.paths.items {
        walk_path_item(paths.key(raw), item, Some(raw), visit);
    }
    let webhooks = At::ROOT.key("webhooks");
    for (name, item) in &document.webhooks {
        path_item(webhooks.key(name), item, visit);
    }
}

/// One Path Item held anywhere but `paths`, and everything below it.
pub(crate) fn path_item<'doc>(at: At<'_>, item: &'doc PathItem, visit: &mut impl Visit<'doc>) {
    walk_path_item(at, item, None, visit);
}

/// One callback's Path Items, and everything below them.
pub(crate) fn callback<'doc>(at: At<'_>, callback: &'doc Callback, visit: &mut impl Visit<'doc>) {
    for (expression, entry) in &callback.items {
        let at = at.key(expression);
        visit.callback_entry(at, expression);
        if let Some(item) = entry.as_item() {
            walk_path_item(at, item, None, visit);
        }
    }
}

fn walk_path_item<'doc>(
    at: At<'_>,
    item: &'doc PathItem,
    path: Option<&'doc str>,
    visit: &mut impl Visit<'doc>,
) {
    if !visit.path_item(at, item, path) {
        return;
    }

    for (method, operation) in item.operations() {
        walk_operation(at.key(field_name(method)), item, operation, visit);
    }

    #[cfg(feature = "openapi32")]
    {
        let section = at.key("additionalOperations");
        for (method, operation) in &item.additional_operations {
            walk_operation(section.key(method), item, operation, visit);
        }
    }
}

fn walk_operation<'doc>(
    at: At<'_>,
    item: &'doc PathItem,
    operation: &'doc Operation,
    visit: &mut impl Visit<'doc>,
) {
    visit.operation(at, item, operation);

    let callbacks = at.key("callbacks");
    for (name, entry) in &operation.callbacks {
        if let Some(entry) = entry.as_item() {
            callback(callbacks.key(name), entry, visit);
        }
    }

    visit.operation_walked(at, operation);
}

/// The Path Item field `method` is held in: its wire name, lowercased.
fn field_name(method: Method) -> &'static str {
    match method {
        Method::Get => "get",
        Method::Put => "put",
        Method::Post => "post",
        Method::Delete => "delete",
        Method::Options => "options",
        Method::Head => "head",
        Method::Patch => "patch",
        Method::Trace => "trace",
        #[cfg(feature = "openapi32")]
        Method::Query => "query",
    }
}

#[cfg(test)]
mod tests;
