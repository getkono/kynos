use super::{Example, ExampleValue};

/// The refusal of `value` beside any other value field.
const VALUE_NOT_ALONE: &str = "`value` is mutually exclusive with `dataValue`, `serializedValue` \
     and `externalValue` on an Example Object";

/// The refusal of `serializedValue` beside `externalValue`.
#[cfg(feature = "openapi32")]
const TWO_SERIALIZATIONS: &str =
    "`serializedValue` and `externalValue` are mutually exclusive on an Example Object";

/// What reading a combination of value fields yields: the example's value,
/// or the exact refusal.
type Reading = Result<Option<ExampleValue>, &'static str>;

/// Reads an Example Object holding exactly the value fields `fields` names,
/// each with a value of its type.
fn read(fields: &[&str]) -> Result<Example, String> {
    let object: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .map(|&field| {
            let value = match field {
                "value" | "dataValue" => serde_json::json!(1),
                "serializedValue" => serde_json::json!("1"),
                "externalValue" => serde_json::json!("./one.json"),
                other => panic!("`{other}` is no value field"),
            };
            (field.to_owned(), value)
        })
        .collect();
    serde_json::from_value(serde_json::Value::Object(object)).map_err(|error| error.to_string())
}

/// Every combination of the 3.1 Example Object's two value fields, against
/// `references/3.1.2.md:2250-2251`: `value` and `externalValue` are
/// mutually exclusive, and each alone, or neither, is an example.
#[cfg(not(feature = "openapi32"))]
#[test]
fn each_combination_of_value_fields_is_read_as_the_specification_says() {
    let external = || ExampleValue::External {
        uri: "./one.json".to_owned(),
    };
    let table: [(&[&str], Reading); 4] = [
        (&[], Ok(None)),
        (&["value"], Ok(Some(ExampleValue::Embedded(1.into())))),
        (&["externalValue"], Ok(Some(external()))),
        (&["value", "externalValue"], Err(VALUE_NOT_ALONE)),
    ];
    for (fields, expected) in table {
        assert_eq!(
            read(fields).map(|example| example.value().cloned()),
            expected.map_err(str::to_owned),
            "{fields:?}"
        );
    }
}

/// All sixteen combinations of the 3.2 Example Object's four value fields,
/// against `references/3.2.0.md:2332-2335`: `dataValue` requires `value`
/// absent; `serializedValue` requires `value` and `externalValue` absent;
/// `externalValue` requires `serializedValue` and `value` absent. Where
/// `value` and the other pair are both present, `value` is the refusal named.
#[cfg(feature = "openapi32")]
#[test]
fn each_combination_of_value_fields_is_read_as_the_specification_says() {
    const FIELDS: [&str; 4] = ["value", "dataValue", "serializedValue", "externalValue"];

    let embedded = || Some(ExampleValue::Embedded(1.into()));
    let external = |data: Option<serde_json::Value>| {
        Some(ExampleValue::External {
            uri: "./one.json".to_owned(),
            data,
        })
    };
    let data = |serialized: Option<&str>| {
        Some(ExampleValue::Data {
            data: 1.into(),
            serialized: serialized.map(str::to_owned),
        })
    };
    // Indexed by the bits `value`, `dataValue`, `serializedValue`,
    // `externalValue`, most significant first.
    let table: [Reading; 16] = [
        /* ---- */ Ok(None),
        /* ---e */ Ok(external(None)),
        /* --s- */ Ok(Some(ExampleValue::Serialized("1".to_owned()))),
        /* --se */ Err(TWO_SERIALIZATIONS),
        /* -d-- */ Ok(data(None)),
        /* -d-e */ Ok(external(Some(1.into()))),
        /* -ds- */ Ok(data(Some("1"))),
        /* -dse */ Err(TWO_SERIALIZATIONS),
        /* v--- */ Ok(embedded()),
        /* v--e */ Err(VALUE_NOT_ALONE),
        /* v-s- */ Err(VALUE_NOT_ALONE),
        /* v-se */ Err(VALUE_NOT_ALONE),
        /* vd-- */ Err(VALUE_NOT_ALONE),
        /* vd-e */ Err(VALUE_NOT_ALONE),
        /* vds- */ Err(VALUE_NOT_ALONE),
        /* vdse */ Err(VALUE_NOT_ALONE),
    ];
    for (bits, expected) in table.into_iter().enumerate() {
        let fields: Vec<&str> = FIELDS
            .iter()
            .enumerate()
            .filter(|(index, _)| bits & (8 >> index) != 0)
            .map(|(_, &field)| field)
            .collect();
        let read = read(&fields);
        assert_eq!(
            read.clone().map(|example| example.value().cloned()),
            expected.map_err(str::to_owned),
            "{fields:?}"
        );
        // What is read is written back as it was read.
        if let Ok(example) = read {
            let json = serde_json::to_value(&example).expect("serializable");
            let mut written: Vec<&str> = json
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            written.sort_unstable();
            let mut fields = fields;
            fields.sort_unstable();
            assert_eq!(written, fields, "{fields:?} is written back");
        }
    }
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
