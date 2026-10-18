use kynos_openapi::{Document, Info, Opaque, OpaqueReason, Operation, PathItem, SpecVersion};

use super::Service;

/// One operation, carrying nothing a marker could collide with.
fn operation() -> Option<Box<Operation>> {
    Some(Box::default())
}

/// `flag_opaque` marks every operation a Path Item can hold, slot by slot.
///
/// The destructure is exhaustive, so a slot added to `PathItem` stops this
/// compiling until it is either asserted here or named as holding no
/// operation — the list `flag_opaque` walks is written by hand, and this is
/// what holds it to the type.
#[test]
fn flagging_marks_every_operation_slot_a_path_item_has() {
    let mut document = Document::new(SpecVersion::V3_1, Info::new("Test", "1"));
    document.paths.items.insert(
        "/things".to_owned(),
        PathItem {
            get: operation(),
            put: operation(),
            post: operation(),
            delete: operation(),
            options: operation(),
            head: operation(),
            patch: operation(),
            trace: operation(),
            #[cfg(feature = "openapi32")]
            query: operation(),
            #[cfg(feature = "openapi32")]
            additional_operations: [("COPY".to_owned(), Box::default())].into_iter().collect(),
            ..PathItem::default()
        },
    );

    Service::<()>::flag_opaque(&mut document, OpaqueReason::UntypedRoute);

    let PathItem {
        reference: _,
        summary: _,
        description: _,
        get,
        put,
        post,
        delete,
        options,
        head,
        patch,
        trace,
        #[cfg(feature = "openapi32")]
        query,
        #[cfg(feature = "openapi32")]
        additional_operations,
        servers: _,
        parameters: _,
        extensions: _,
    } = &document.paths.items["/things"];

    let mut slots = vec![
        ("get", get),
        ("put", put),
        ("post", post),
        ("delete", delete),
        ("options", options),
        ("head", head),
        ("patch", patch),
        ("trace", trace),
    ];
    #[cfg(feature = "openapi32")]
    slots.push(("query", query));

    let expected = Some(Opaque::new(OpaqueReason::UntypedRoute));
    for (slot, operation) in slots {
        let operation = operation.as_deref().expect("the slot was filled");
        assert_eq!(
            Opaque::of(operation).expect("a well-formed marker"),
            expected,
            "{slot}"
        );
    }
    #[cfg(feature = "openapi32")]
    for (method, operation) in additional_operations {
        assert_eq!(
            Opaque::of(operation).expect("a well-formed marker"),
            expected,
            "{method}"
        );
    }
}
