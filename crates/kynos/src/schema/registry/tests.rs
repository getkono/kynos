use kynos_openapi::{
    ComponentName, Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use super::{Registry, SchemaConflict};
use crate::schema::Schema;

/// An object schema with one property, which is all these fixtures need to
/// tell two bodies apart.
fn object_with(property: &str, schema: OpenApiSchema) -> OpenApiSchema {
    let mut object = SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        ..SchemaObject::default()
    };
    object.properties.insert(property.to_owned(), schema);
    OpenApiSchema::Object(Box::new(object))
}

fn item() -> Option<ComponentName> {
    ComponentName::new("Item").ok()
}

/// Two modules each declaring an `Item`, which is what a derive names them
/// both: the component name is the bare identifier.
mod a {
    use super::{OpenApiSchema, Registry, Schema, item, object_with};

    /// Refers to the other `Item`, so that one is reached while this one's
    /// name is still mid-descent.
    pub(super) struct Item;

    impl Schema for Item {
        fn schema(registry: &mut Registry) -> OpenApiSchema {
            object_with("child", registry.resolve::<Option<Box<super::b::Item>>>())
        }

        fn name() -> Option<kynos_openapi::ComponentName> {
            item()
        }
    }
}

mod b {
    use super::{OpenApiSchema, Registry, Schema, SchemaType, item, object_with};

    pub(super) struct Item;

    impl Schema for Item {
        fn schema(_registry: &mut Registry) -> OpenApiSchema {
            object_with("id", OpenApiSchema::of_type(SchemaType::Integer))
        }

        fn name() -> Option<kynos_openapi::ComponentName> {
            item()
        }
    }
}

/// A type that genuinely refers to itself, through the wrappers a recursive
/// field needs. `Box<Node>` shares `Node`'s name but not its `type_name`, so it
/// is the case a check on type identity alone would refuse.
struct Node;

impl Schema for Node {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object_with("next", registry.resolve::<Option<Box<Self>>>())
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Node").ok()
    }
}

/// Refers to [`Line`], a named type of its own, so `Line` finishes its descent
/// while `Order` still holds a different name.
struct Order;

impl Schema for Order {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object_with("line", registry.resolve::<Line>())
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Order").ok()
    }
}

struct Line;

impl Schema for Line {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        object_with("sku", OpenApiSchema::of_type(SchemaType::String))
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Line").ok()
    }
}

/// Only a descent holding the *same* name is a rival's holder: a named type
/// nested inside a differently named one registers under its own name.
#[test]
fn a_named_type_inside_another_registers_under_its_own_name() {
    let mut registry = Registry::new();

    registry.resolve::<Order>();

    assert!(registry.schema_conflicts().is_empty());
    let components = registry.into_components();
    assert_eq!(
        components.schemas.get("Line"),
        Some(&object_with(
            "sku",
            OpenApiSchema::of_type(SchemaType::String)
        ))
    );
    assert_eq!(
        components.schemas.get("Order"),
        Some(&object_with("line", OpenApiSchema::component("Line")))
    );
}

/// A different type reaching a name whose description has not finished is
/// still a second claimant, and has to be compared like one: aliasing it to the
/// first would describe `b::Item` as a recursive `a::Item` and drop it from the
/// document without a word.
#[test]
fn a_rival_reached_mid_descent_is_a_conflict() {
    let mut registry = Registry::new();

    registry.resolve::<a::Item>();

    assert_eq!(
        registry.schema_conflicts(),
        [SchemaConflict {
            name: "Item".to_owned(),
        }]
    );
}

/// A type reaching its own name through a transparent wrapper is the same
/// component, not a rival.
#[test]
fn a_type_reaching_itself_through_a_wrapper_is_no_conflict() {
    let mut registry = Registry::new();

    let reference = registry.resolve::<Node>();

    assert!(registry.schema_conflicts().is_empty());
    assert_eq!(reference, OpenApiSchema::component("Node"));

    let components = registry.into_components();
    let next = components
        .schemas
        .get("Node")
        .and_then(OpenApiSchema::as_object);
    assert!(
        next.is_some_and(|node| node.properties.contains_key("next")),
        "`Node` is registered with its own body"
    );
}

/// A type with no component name that refers to itself, as a generic derive
/// does: every instantiation is inlined, so nothing can stand in for the body
/// while it is being built.
struct Chain;

impl Schema for Chain {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object_with("next", registry.resolve::<Option<Box<Self>>>())
    }
}

/// Anonymous, and reaches itself only through [`Holder`], which is named.
struct Pair;

impl Schema for Pair {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object_with("holder", registry.resolve::<Holder>())
    }
}

struct Holder;

impl Schema for Holder {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        object_with("pair", registry.resolve::<Pair>())
    }

    fn name() -> Option<ComponentName> {
        ComponentName::new("Holder").ok()
    }
}

/// An inlined type reaching itself with no named type between has no finite
/// description, and saying so beats overflowing the stack.
#[test]
#[should_panic(expected = "refers to itself through no type with a component name")]
fn an_anonymous_type_reaching_itself_is_refused() {
    Registry::new().resolve::<Chain>();
}

/// A named type between two inlinings of one anonymous type is a `$ref` the
/// second one stops at, so the cycle has an end.
#[test]
fn an_anonymous_type_reaching_itself_through_a_named_one_terminates() {
    let mut registry = Registry::new();

    let pair = registry.resolve::<Pair>();

    assert_eq!(
        pair,
        object_with("holder", OpenApiSchema::component("Holder"))
    );
    assert_eq!(
        registry.into_components().schemas.get("Holder"),
        Some(&object_with(
            "pair",
            object_with("holder", OpenApiSchema::component("Holder"))
        ))
    );
}
