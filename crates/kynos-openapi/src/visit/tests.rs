use super::Visit;
use crate::{
    model::{
        callback::Callback,
        document::{Document, SpecVersion},
        info::Info,
        paths::{item::PathItem, method::Method, operation::Operation},
        reference::{Ref, RefOr},
    },
    pointer::At,
};

/// Writes down each hook as it fires, with its pointer.
#[derive(Default)]
struct Record {
    events: Vec<String>,
    /// The `paths` key whose Path Item is skipped.
    skip: Option<&'static str>,
}

impl<'doc> Visit<'doc> for Record {
    fn path_item(&mut self, at: At<'_>, _item: &'doc PathItem, path: Option<&'doc str>) -> bool {
        self.events
            .push(format!("item {} {}", at.pointer(), path.unwrap_or("-")));
        path.is_none() || path != self.skip
    }

    fn operation(&mut self, at: At<'_>, item: &'doc PathItem, operation: &'doc Operation) {
        let held = item
            .operations()
            .any(|(_, held)| std::ptr::eq(held, operation));
        #[cfg(feature = "openapi32")]
        let held = held
            || item
                .additional_operations
                .values()
                .any(|held| std::ptr::eq(held.as_ref(), operation));
        assert!(
            held,
            "{} is passed with a Path Item not holding it",
            at.pointer()
        );
        self.events.push(format!("operation {}", at.pointer()));
    }

    fn operation_walked(&mut self, at: At<'_>, _operation: &'doc Operation) {
        self.events.push(format!("walked {}", at.pointer()));
    }

    fn callback_entry(&mut self, at: At<'_>, expression: &'doc str) {
        self.events
            .push(format!("entry {} {expression}", at.pointer()));
    }

    fn reference(&mut self, at: At<'_>, uri: &'doc str) {
        self.events
            .push(format!("reference {} {uri}", at.pointer()));
    }
}

/// An operation declaring two callbacks: `onEvent`, with an inline entry and a
/// referenced one, and `onReuse`, itself a reference.
fn calling_back(id: &str) -> Operation {
    let mut callback = Callback::new().with(
        "{$request.body#/url}",
        PathItem::new().with_operation(Method::Post, Operation::new(format!("{id}Hook"))),
    );
    callback
        .items
        .insert("elsewhere".to_owned(), RefOr::Ref(Ref::new("#/x")));
    let mut operation = Operation::new(id);
    operation
        .callbacks
        .insert("onEvent".to_owned(), RefOr::Item(callback));
    operation.callbacks.insert(
        "onReuse".to_owned(),
        RefOr::Ref(Ref::new("#/components/callbacks/Reusable")),
    );
    operation
}

fn document() -> Document {
    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1.0.0"));
    document.paths.items.insert(
        "/pets/{id}".to_owned(),
        PathItem::new()
            .with_operation(Method::Post, Operation::new("create"))
            .with_operation(Method::Get, calling_back("read")),
    );
    document.webhooks.insert(
        "newPet".to_owned(),
        PathItem::new().with_operation(Method::Put, Operation::new("hook")),
    );
    document.components.path_items.insert(
        "Shared".to_owned(),
        PathItem::new().with_operation(Method::Delete, Operation::new("shared")),
    );
    document.components.callbacks.insert(
        "Reusable".to_owned(),
        RefOr::Item(Callback::new().with("/back", PathItem::new())),
    );
    document
        .components
        .callbacks
        .insert("Referenced".to_owned(), RefOr::Ref(Ref::new("#/y")));
    document
}

#[test]
fn every_path_item_and_operation_is_visited_once_where_it_is_written() {
    let document = document();
    let mut record = Record::default();

    super::document(&document, &mut record);

    assert_eq!(
        record.events,
        [
            "item #/paths/~1pets~1{id} /pets/{id}",
            "operation #/paths/~1pets~1{id}/get",
            "entry #/paths/~1pets~1{id}/get/callbacks/onEvent/{$request.body#~1url} \
             {$request.body#/url}",
            "item #/paths/~1pets~1{id}/get/callbacks/onEvent/{$request.body#~1url} -",
            "operation #/paths/~1pets~1{id}/get/callbacks/onEvent/{$request.body#~1url}/post",
            "walked #/paths/~1pets~1{id}/get/callbacks/onEvent/{$request.body#~1url}/post",
            "entry #/paths/~1pets~1{id}/get/callbacks/onEvent/elsewhere elsewhere",
            "reference #/paths/~1pets~1{id}/get/callbacks/onEvent/elsewhere #/x",
            "reference #/paths/~1pets~1{id}/get/callbacks/onReuse \
             #/components/callbacks/Reusable",
            "walked #/paths/~1pets~1{id}/get",
            "operation #/paths/~1pets~1{id}/post",
            "walked #/paths/~1pets~1{id}/post",
            "item #/webhooks/newPet -",
            "operation #/webhooks/newPet/put",
            "walked #/webhooks/newPet/put",
            "item #/components/pathItems/Shared -",
            "operation #/components/pathItems/Shared/delete",
            "walked #/components/pathItems/Shared/delete",
            "entry #/components/callbacks/Reusable/~1back /back",
            "item #/components/callbacks/Reusable/~1back -",
            "reference #/components/callbacks/Referenced #/y",
        ]
    );
}

#[test]
fn a_skipped_path_item_hides_its_operations_and_their_callbacks() {
    let document = document();
    let mut record = Record {
        skip: Some("/pets/{id}"),
        ..Record::default()
    };

    super::routes(&document, &mut record);

    assert_eq!(
        record.events,
        [
            "item #/paths/~1pets~1{id} /pets/{id}",
            "item #/webhooks/newPet -",
            "operation #/webhooks/newPet/put",
            "walked #/webhooks/newPet/put",
        ]
    );
}

#[cfg(feature = "openapi32")]
#[test]
fn additional_operations_follow_the_fixed_methods_in_insertion_order() {
    let mut item = PathItem::new().with_operation(Method::Query, Operation::new("query"));
    for method in ["PURGE", "LINK"] {
        item.additional_operations
            .insert(method.to_owned(), Box::new(Operation::new(method)));
    }
    let mut record = Record::default();

    super::path_item(At::ROOT.key("item"), &item, &mut record);

    assert_eq!(
        record.events,
        [
            "item #/item -",
            "operation #/item/query",
            "walked #/item/query",
            "operation #/item/additionalOperations/PURGE",
            "walked #/item/additionalOperations/PURGE",
            "operation #/item/additionalOperations/LINK",
            "walked #/item/additionalOperations/LINK",
        ]
    );
}

#[test]
fn each_method_is_visited_under_its_path_item_field() {
    let mut item = PathItem::new();
    for &method in Method::all() {
        item.set_operation(method, Operation::new(method.as_wire_str()));
    }
    let mut record = Record::default();

    super::path_item(At::ROOT, &item, &mut record);

    let fields: Vec<String> = Method::all()
        .iter()
        .map(|method| format!("operation #/{}", method.as_wire_str().to_lowercase()))
        .collect();
    let visited: Vec<&String> = record
        .events
        .iter()
        .filter(|event| event.starts_with("operation"))
        .collect();
    assert_eq!(visited, fields.iter().collect::<Vec<_>>());
}
