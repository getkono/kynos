use kynos_openapi::{
    ComponentName, Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use super::{InstantiationNameRefusal, Registry, SchemaConflict};
use crate::schema::{NamedInstantiation, Schema};

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

/// A map key whose `key_constraints` declares `$pattern`.
macro_rules! patterned_key {
    ($name:ident, $pattern:literal) => {
        struct $name;

        impl Schema for $name {
            fn schema(_registry: &mut Registry) -> OpenApiSchema {
                OpenApiSchema::of_type(SchemaType::String)
            }
        }

        impl crate::schema::MapKey for $name {
            fn key_constraints() -> crate::schema::constraints::Constraints {
                let mut constraints = crate::schema::constraints::Constraints::default();
                constraints.pattern = Some($pattern.to_owned());
                constraints
            }
        }
    };
}

patterned_key!(Sku, "^[A-Z]{3}$");
#[cfg(feature = "pattern")]
patterned_key!(Lookahead, "a(?=b)");

/// A key pattern the engine runs is admitted.
#[cfg(feature = "pattern")]
#[test]
fn a_key_pattern_that_translates_is_admitted() {
    let mut registry = Registry::new();

    registry.resolve::<std::collections::BTreeMap<Sku, u32>>();

    assert!(registry.key_pattern_refusals().is_empty());
}

/// A key pattern no check could run is refused once, however many maps key
/// by its type, and says why.
#[cfg(feature = "pattern")]
#[test]
fn a_key_pattern_that_does_not_translate_is_refused_once() {
    let mut registry = Registry::new();

    registry.resolve::<std::collections::BTreeMap<Lookahead, u32>>();
    registry.resolve::<std::collections::HashMap<Lookahead, String>>();

    let [refused] = registry.key_pattern_refusals() else {
        panic!("one refusal: {:?}", registry.key_pattern_refusals());
    };
    assert_eq!(refused.key, std::any::type_name::<Lookahead>());
    assert_eq!(refused.pattern, "a(?=b)");
    assert!(
        refused.reason.contains("lookaround and backreferences"),
        "{}",
        refused.reason
    );
}

/// A third `Item`, unrelated to the other two and described differently.
mod c {
    use super::{OpenApiSchema, Registry, Schema, SchemaType, item, object_with};

    pub(super) struct Item;

    impl Schema for Item {
        fn schema(_registry: &mut Registry) -> OpenApiSchema {
            object_with("name", OpenApiSchema::of_type(SchemaType::String))
        }

        fn name() -> Option<kynos_openapi::ComponentName> {
            item()
        }
    }
}

/// A response whose only content is `T`, so an operation returning it puts
/// `T` in the registry `Router::build` describes with.
struct Described<T>(std::marker::PhantomData<fn() -> T>);

impl<T: Schema> crate::response::Responses for Described<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                "application/json",
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}

impl<T> crate::response::IntoResponse for Described<T> {
    fn into_response(self) -> crate::http::Response {
        crate::response::status::NoContent.into_response()
    }
}

async fn first() -> Described<b::Item> {
    Described(std::marker::PhantomData)
}

async fn second() -> Described<c::Item> {
    Described(std::marker::PhantomData)
}

/// Two unrelated types claiming one component name with different bodies,
/// each reached from its own operation, refuse the build as the schema
/// conflict naming it, rather than one silently describing the other.
#[test]
fn two_types_claiming_one_name_refuse_the_build() {
    use crate::router::endpoint::builder::EndpointBuilder;

    let path = |path: &str| kynos_openapi::PathTemplate::parse(path).expect("a usable path");
    let built = crate::Router::<()>::new()
        .mount((
            EndpointBuilder::<(), _, _>::new(kynos_openapi::Method::Get, path("/b"), first),
            EndpointBuilder::<(), _, _>::new(kynos_openapi::Method::Get, path("/c"), second),
        ))
        .build(());

    let Err(crate::Error::Schema(conflict)) = built else {
        panic!("two bodies under one name were built");
    };
    assert_eq!(
        conflict,
        SchemaConflict {
            name: "Item".to_owned(),
        }
    );
}

/// Without the engine no key pattern can be checked, so every one is refused
/// rather than described as though it were.
#[cfg(not(feature = "pattern"))]
#[test]
fn a_key_pattern_is_refused_without_the_pattern_feature() {
    let mut registry = Registry::new();

    registry.resolve::<std::collections::BTreeMap<Sku, u32>>();

    let [refused] = registry.key_pattern_refusals() else {
        panic!("one refusal: {:?}", registry.key_pattern_refusals());
    };
    assert_eq!(refused.pattern, "^[A-Z]{3}$");
    assert!(
        refused.reason.contains("`pattern` feature"),
        "{}",
        refused.reason
    );
}

/// A generic type as the derive implements it: named by
/// [`instantiation_name`](crate::schema::instantiation_name), and described
/// with prose of its own.
struct Page<T>(std::marker::PhantomData<T>);

impl<T: Schema> Schema for Page<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        let mut page = object_with("items", registry.resolve::<Vec<T>>());
        if let OpenApiSchema::Object(keywords) = &mut page {
            keywords.description = Some("Any page.".to_owned());
        }
        page
    }

    fn name() -> Option<ComponentName> {
        crate::schema::instantiation_name("Page", &[T::name()])
    }
}

impl NamedInstantiation for Page<Line> {
    const NAME: &'static str = "LinePage";
    const DESCRIPTION: Option<&'static str> = Some("One page of lines.");
}

/// Shares `Page<Line>`'s mangled name under another type, so naming it too
/// gives one component two names.
impl NamedInstantiation for Box<Page<Line>> {
    const NAME: &'static str = "BoxedLinePage";
}

/// Inlined wherever it is used, so it has no name to replace.
impl NamedInstantiation for Page<u32> {
    const NAME: &'static str = "CountPage";
}

/// Named with no prose of its own, and a name no component may carry.
impl NamedInstantiation for Page<Order> {
    const NAME: &'static str = "Order Page";
}

/// The mangled name is replaced wherever it would have been written, through a
/// wrapper sharing it too, and the alias's description replaces the generic
/// type's own.
#[test]
fn a_named_instantiation_replaces_the_mangled_name_everywhere() {
    let mut registry = Registry::new();
    registry.name_instantiation::<Page<Line>>();

    assert_eq!(
        registry.resolve::<Page<Line>>(),
        OpenApiSchema::component("LinePage")
    );
    assert_eq!(
        registry.resolve::<Box<Page<Line>>>(),
        OpenApiSchema::component("LinePage")
    );

    assert!(registry.instantiation_name_refusals().is_empty());
    assert!(registry.schema_conflicts().is_empty());
    let components = registry.into_components();
    assert!(!components.schemas.contains_key("Page_Line"));
    let Some(OpenApiSchema::Object(page)) = components.schemas.get("LinePage") else {
        panic!("`LinePage` is an object component: {components:?}");
    };
    assert_eq!(page.description.as_deref(), Some("One page of lines."));
}

/// Registering one name twice, as two merged routers each declaring it do, is
/// no conflict.
#[test]
fn naming_an_instantiation_twice_alike_is_idempotent() {
    let mut registry = Registry::new();
    registry.name_instantiation::<Page<Line>>();
    registry.name_instantiation::<Page<Line>>();

    assert_eq!(
        registry.resolve::<Page<Line>>(),
        OpenApiSchema::component("LinePage")
    );
    assert!(registry.instantiation_name_refusals().is_empty());
}

/// Each name the registry cannot give is recorded once, naming what it was
/// given to, and leaves the mangled name in place.
#[test]
fn a_name_the_registry_cannot_give_is_refused_once() {
    let mut registry = Registry::new();
    registry.name_instantiation::<Page<Line>>();
    for _ in 0..2 {
        registry.name_instantiation::<Box<Page<Line>>>();
        registry.name_instantiation::<Page<u32>>();
        registry.name_instantiation::<Page<Order>>();
    }

    assert_eq!(
        registry.instantiation_name_refusals(),
        [
            InstantiationNameRefusal::Contested {
                instantiation: "Page_Line".to_owned(),
                first: "LinePage".to_owned(),
                second: "BoxedLinePage".to_owned(),
            },
            InstantiationNameRefusal::Anonymous {
                instantiation: std::any::type_name::<Page<u32>>().to_owned(),
                name: "CountPage".to_owned(),
            },
            InstantiationNameRefusal::Invalid {
                instantiation: std::any::type_name::<Page<Order>>().to_owned(),
                name: "Order Page".to_owned(),
            },
        ]
    );
    assert_eq!(
        registry.resolve::<Page<Order>>(),
        OpenApiSchema::component("Page_Order")
    );
}

/// A refused name fails the build as the refusal, before the component
/// conflict a contested name could also leave.
#[test]
fn a_refused_instantiation_name_refuses_the_build() {
    let built = crate::Router::<()>::new()
        .name_instantiation::<Page<u32>>()
        .build(());

    let Err(crate::Error::InstantiationName(refused)) = built else {
        panic!("an unnamed instantiation was given a name");
    };
    assert!(matches!(
        *refused,
        InstantiationNameRefusal::Anonymous { .. }
    ));
}
