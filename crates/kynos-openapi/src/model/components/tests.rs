use super::{ComponentName, Components};
use crate::model::{
    paths::item::PathItem,
    reference::{Ref, RefOr},
    schema::Schema,
};

/// Each field of `components` by name, and whether it holds anything.
///
/// The destructuring has no `..`, so a field added to [`Components`] stops this
/// file compiling until it is listed here — and, through the closure check in
/// the test below, until it has a case of its own.
fn occupancy(components: &Components) -> Vec<(&'static str, bool)> {
    let Components {
        schemas,
        responses,
        parameters,
        examples,
        request_bodies,
        headers,
        security_schemes,
        links,
        callbacks,
        path_items,
        #[cfg(feature = "openapi32")]
        media_types,
        extensions,
    } = components;

    vec![
        ("schemas", !schemas.is_empty()),
        ("responses", !responses.is_empty()),
        ("parameters", !parameters.is_empty()),
        ("examples", !examples.is_empty()),
        ("request_bodies", !request_bodies.is_empty()),
        ("headers", !headers.is_empty()),
        ("security_schemes", !security_schemes.is_empty()),
        ("links", !links.is_empty()),
        ("callbacks", !callbacks.is_empty()),
        ("path_items", !path_items.is_empty()),
        #[cfg(feature = "openapi32")]
        ("media_types", !media_types.is_empty()),
        ("extensions", !extensions.is_empty()),
    ]
}

/// A `Components` holding one entry in the field `name` and nothing else.
fn holding_only(name: &str) -> Components {
    fn reference<T>() -> RefOr<T> {
        RefOr::Ref(Ref::new("#/components/x"))
    }

    let key = || "x".to_owned();
    let mut components = Components::new();
    match name {
        "schemas" => {
            components.schemas.insert(key(), Schema::component("X"));
        }
        "responses" => {
            components.responses.insert(key(), reference());
        }
        "parameters" => {
            components.parameters.insert(key(), reference());
        }
        "examples" => {
            components.examples.insert(key(), reference());
        }
        "request_bodies" => {
            components.request_bodies.insert(key(), reference());
        }
        "headers" => {
            components.headers.insert(key(), reference());
        }
        "security_schemes" => {
            components.security_schemes.insert(key(), reference());
        }
        "links" => {
            components.links.insert(key(), reference());
        }
        "callbacks" => {
            components.callbacks.insert(key(), reference());
        }
        "path_items" => {
            components.path_items.insert(key(), PathItem::new());
        }
        #[cfg(feature = "openapi32")]
        "media_types" => {
            components.media_types.insert(key(), reference());
        }
        "extensions" => {
            components.extensions.insert("x-a", 1);
        }
        other => panic!("no case populates `{other}`"),
    }
    components
}

#[test]
fn components_are_empty_only_while_every_field_is() {
    let empty = Components::new();
    assert!(empty.is_empty());
    assert!(occupancy(&empty).iter().all(|&(_, occupied)| !occupied));

    // Every field `occupancy` names has a case, because `holding_only` panics
    // on a name it cannot populate.
    for (field, _) in occupancy(&empty) {
        let components = holding_only(field);
        let occupied: Vec<_> = occupancy(&components)
            .into_iter()
            .filter_map(|(name, occupied)| occupied.then_some(name))
            .collect();

        assert_eq!(occupied, [field], "the case populates `{field}` alone");
        assert!(!components.is_empty(), "`{field}` holds an entry");
    }
}

#[test]
fn ordinary_type_names_are_valid_component_names() {
    assert!(ComponentName::new("User").is_ok());
    assert!(ComponentName::new("Order.Line_Item-v2").is_ok());
}

#[test]
fn names_outside_the_permitted_character_set_are_rejected() {
    assert!(ComponentName::new("Vec<User>").is_err());
    assert!(ComponentName::new("crate::User").is_err());
    assert!(ComponentName::new("").is_err());
    assert!(ComponentName::new("a b").is_err());
}

#[test]
fn sanitizing_mangles_a_generic_type_name_into_a_legal_key() {
    let name = ComponentName::sanitized("Vec<User>").expect("non-empty");
    assert_eq!(name.as_str(), "Vec_User");
}

#[test]
fn sanitizing_collapses_runs_and_trims_edges() {
    let name = ComponentName::sanitized("crate::model::User").expect("non-empty");
    assert_eq!(name.as_str(), "crate_model_User");
}

#[test]
fn sanitizing_an_entirely_illegal_name_still_yields_something_legal() {
    let name = ComponentName::sanitized("<>").expect("non-empty");
    assert!(ComponentName::is_valid(name.as_str()));
}
