use super::Example;
#[cfg(feature = "openapi32")]
use super::ExampleValue;

#[test]
fn an_embedded_value_excludes_every_other_form() {
    let error = serde_json::from_str::<Example>(r#"{"value":1,"externalValue":"./e.png"}"#)
        .expect_err("`value` is exclusive with `externalValue`");

    assert!(error.to_string().contains("mutually exclusive"));
}

#[test]
fn an_external_example_round_trips() {
    let example = Example::external("./examples/red.png");
    let json = serde_json::to_string(&example).expect("serializable");
    assert_eq!(json, r#"{"externalValue":"./examples/red.png"}"#);

    let parsed: Example = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(parsed, example);
}

#[cfg(feature = "openapi32")]
#[test]
fn data_and_its_serialization_are_one_example() {
    // Exactly the boolean query parameter the specification writes out:
    // `true` is the data and `flag=true` is the wire form, and neither can
    // be derived from the other.
    let example = Example::data_serialized(true, "flag=true");
    let json = serde_json::to_string(&example).expect("serializable");
    assert_eq!(json, r#"{"dataValue":true,"serializedValue":"flag=true"}"#);

    let parsed: Example = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(parsed, example);
    assert!(matches!(
        parsed.value(),
        Some(ExampleValue::Data {
            serialized: Some(_),
            ..
        })
    ));
}

#[cfg(feature = "openapi32")]
#[test]
fn two_serializations_of_one_example_are_rejected() {
    let error =
        serde_json::from_str::<Example>(r#"{"serializedValue":"a","externalValue":"./e.bin"}"#)
            .expect_err("`serializedValue` is exclusive with `externalValue`");

    assert!(error.to_string().contains("mutually exclusive"));
}
