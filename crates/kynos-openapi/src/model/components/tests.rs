use super::ComponentName;

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
