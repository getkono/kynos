//! The Example Object.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{extensions::Extensions, reference::RefOr},
};

/// A worked example of a parameter, request body, response body or header.
///
/// # Choosing a value field
///
/// OpenAPI 3.1 offers `value` and `externalValue`, which are mutually
/// exclusive.
///
/// 3.2 adds `dataValue` and `serializedValue`, and deprecates `value` for
/// non-JSON serialization targets — for those, `value` has
/// implementation-defined behaviour, which is exactly the kind of ambiguity
/// Kynos avoids. Prefer [`data`](Example::data) (the example as data, before
/// serialization) and [`serialized`](Example::serialized) (the example as it
/// appears on the wire) whenever `openapi32` is available and the target is not
/// JSON.
///
/// The exclusions between those four fields are not a plain one-of, which is
/// why they live in [`ExampleValue`] rather than in four `Option`s.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawExample", into = "RawExample")]
pub struct Example {
    /// A short description of the example.
    pub summary: Option<String>,

    /// A long description of the example. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    pub description: Option<String>,

    value: Option<ExampleValue>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// The example itself, in whichever form carries it.
///
/// The specification's exclusions are asymmetric, so this is not a one-of over
/// four fields. `value` excludes all three others; `serializedValue` and
/// `externalValue` exclude each other; but `dataValue` pairs with *either* of
/// them, which is how the specification's own worked examples are written. The
/// variants below are exactly the combinations that leaves.
/// `#[non_exhaustive]` because OpenAPI 3.2 adds to this and the addition is
/// `#[cfg]`-gated. Cargo unifies features across a dependency graph, so any
/// crate enabling `openapi32` enables it for every crate in the build -- and
/// without this attribute that would turn a downstream exhaustive `match` into
/// a compile error, which is not what "purely additive" is supposed to mean.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExampleValue {
    /// An embedded literal example, written to `value`.
    ///
    /// Exclusive with every other form. Deprecated by 3.2 for non-JSON
    /// serialization targets; see the type-level documentation.
    ///
    /// Named for what it is rather than for its field: a variant called `Value`
    /// would collide with [`serde_json::Value`] in rustc's shortest-path table
    /// and lengthen that type's name in every diagnostic mentioning it, this
    /// crate's or anyone else's.
    Embedded(Value),

    /// A URI identifying the serialized example, written to `externalValue`.
    ///
    /// For payloads that cannot be embedded in JSON or YAML.
    ///
    /// `#[non_exhaustive]` on the *variant*, because 3.2 adds `data` to it.
    /// The attribute on the enum covers a variant being added and says nothing
    /// about this one's field list; see the type's own documentation.
    #[non_exhaustive]
    External {
        /// The URI identifying the example.
        uri: String,

        /// The data the URI serializes, written to `dataValue`.
        #[cfg(feature = "openapi32")]
        data: Option<Value>,
    },

    /// The example as data, written to `dataValue`.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    Data {
        /// The example as a data structure, before serialization.
        data: Value,

        /// The same example on the wire, written to `serializedValue`.
        serialized: Option<String>,
    },

    /// The example exactly as it appears on the wire, written to
    /// `serializedValue`, with no data form given.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    Serialized(String),
}

impl Example {
    /// Creates an example holding an embedded value.
    pub fn new(value: impl Into<Value>) -> Self {
        Self::carrying(ExampleValue::Embedded(value.into()))
    }

    /// Creates an example pointing at an external payload.
    pub fn external(uri: impl Into<String>) -> Self {
        Self::carrying(ExampleValue::External {
            uri: uri.into(),
            #[cfg(feature = "openapi32")]
            data: None,
        })
    }

    /// Creates an example given as data, before serialization.
    ///
    /// Preferred over [`new`](Example::new) whenever the serialization target
    /// is not JSON.
    #[cfg(feature = "openapi32")]
    pub fn data(data: impl Into<Value>) -> Self {
        Self::carrying(ExampleValue::Data {
            data: data.into(),
            serialized: None,
        })
    }

    /// Creates an example given only in its serialized form.
    #[cfg(feature = "openapi32")]
    pub fn serialized(serialized: impl Into<String>) -> Self {
        Self::carrying(ExampleValue::Serialized(serialized.into()))
    }

    /// Creates an example given as data together with its serialization.
    ///
    /// The pair a boolean query parameter needs: `true` is the data, `flag=true`
    /// is what goes on the wire, and neither implies the other.
    #[cfg(feature = "openapi32")]
    pub fn data_serialized(data: impl Into<Value>, serialized: impl Into<String>) -> Self {
        Self::carrying(ExampleValue::Data {
            data: data.into(),
            serialized: Some(serialized.into()),
        })
    }

    /// Creates an example given as data, serialized into an external payload.
    #[cfg(feature = "openapi32")]
    pub fn data_external(data: impl Into<Value>, uri: impl Into<String>) -> Self {
        Self::carrying(ExampleValue::External {
            uri: uri.into(),
            data: Some(data.into()),
        })
    }

    /// The example this object carries, if it carries one.
    ///
    /// An Example Object with only a summary is valid, if not useful.
    #[must_use]
    pub fn value(&self) -> Option<&ExampleValue> {
        self.value.as_ref()
    }

    fn carrying(value: ExampleValue) -> Self {
        Self {
            value: Some(value),
            ..Self::default()
        }
    }

    /// Sets the short summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the long description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// The wire shape: the value fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawExample {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<Value>,

    #[cfg(feature = "openapi32")]
    #[serde(rename = "dataValue", default, skip_serializing_if = "Option::is_none")]
    data_value: Option<Value>,

    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "serializedValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    serialized_value: Option<String>,

    #[serde(
        rename = "externalValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    external_value: Option<String>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// An Example Object whose value fields cannot hold together.
#[derive(Debug)]
enum ExampleConflict {
    /// `value` was set beside one of the fields that excludes it.
    ValueNotAlone,

    /// `serializedValue` and `externalValue` were both set.
    #[cfg(feature = "openapi32")]
    TwoSerializations,
}

impl std::fmt::Display for ExampleConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValueNotAlone => f.write_str(
                "`value` is mutually exclusive with `dataValue`, `serializedValue` and \
                 `externalValue` on an Example Object",
            ),
            #[cfg(feature = "openapi32")]
            Self::TwoSerializations => f.write_str(
                "`serializedValue` and `externalValue` are mutually exclusive on an Example \
                 Object",
            ),
        }
    }
}

impl TryFrom<RawExample> for Example {
    type Error = ExampleConflict;

    fn try_from(raw: RawExample) -> Result<Self, Self::Error> {
        // One total match rather than guards and a fallthrough, so that the
        // compiler is the thing checking these combinations are exhaustive.
        #[cfg(feature = "openapi32")]
        let value = match (
            raw.value,
            raw.data_value,
            raw.serialized_value,
            raw.external_value,
        ) {
            (Some(_), Some(_), _, _) | (Some(_), _, Some(_), _) | (Some(_), _, _, Some(_)) => {
                return Err(ExampleConflict::ValueNotAlone);
            }
            (_, _, Some(_), Some(_)) => return Err(ExampleConflict::TwoSerializations),

            (Some(value), None, None, None) => Some(ExampleValue::Embedded(value)),
            (None, data, None, Some(uri)) => Some(ExampleValue::External { uri, data }),
            (None, Some(data), serialized, None) => Some(ExampleValue::Data { data, serialized }),
            (None, None, Some(serialized), None) => Some(ExampleValue::Serialized(serialized)),
            (None, None, None, None) => None,
        };

        #[cfg(not(feature = "openapi32"))]
        let value = match (raw.value, raw.external_value) {
            (Some(_), Some(_)) => return Err(ExampleConflict::ValueNotAlone),
            (Some(value), None) => Some(ExampleValue::Embedded(value)),
            (None, Some(uri)) => Some(ExampleValue::External { uri }),
            (None, None) => None,
        };

        Ok(Self {
            summary: raw.summary,
            description: raw.description,
            value,
            extensions: raw.extensions,
        })
    }
}

impl From<Example> for RawExample {
    fn from(example: Example) -> Self {
        #[cfg(feature = "openapi32")]
        let (value, data_value, serialized_value, external_value) = match example.value {
            Some(ExampleValue::Embedded(value)) => (Some(value), None, None, None),
            Some(ExampleValue::External { uri, data }) => (None, data, None, Some(uri)),
            Some(ExampleValue::Data { data, serialized }) => (None, Some(data), serialized, None),
            Some(ExampleValue::Serialized(serialized)) => (None, None, Some(serialized), None),
            None => (None, None, None, None),
        };

        #[cfg(not(feature = "openapi32"))]
        let (value, external_value) = match example.value {
            Some(ExampleValue::Embedded(value)) => (Some(value), None),
            Some(ExampleValue::External { uri }) => (None, Some(uri)),
            None => (None, None),
        };

        Self {
            summary: example.summary,
            description: example.description,
            value,
            #[cfg(feature = "openapi32")]
            data_value,
            #[cfg(feature = "openapi32")]
            serialized_value,
            external_value,
            extensions: example.extensions,
        }
    }
}

/// The examples an object shows its value with, in whichever form it uses.
///
/// A Parameter, Header or Media Type Object may carry one inline `example` or a
/// map of named `examples`, and the specification makes the two mutually
/// exclusive. An enum rather than two `Option` fields, for the reason
/// [`SecurityScheme`](crate::model::security::SecurityScheme) is one: an
/// unusable combination that cannot be spelled needs no rule to reject it.
///
/// Note that the singular form is not an [`Example`]: `example` is the value
/// itself, written inline, while `examples` maps names to Example Objects that
/// can also carry a summary, a description or an external payload.
#[derive(Clone, Debug, PartialEq)]
pub enum Examples {
    /// One example of the value, written to `example`.
    Inline(Value),

    /// Named examples, written to `examples`.
    ///
    /// Each is an [`Example`] or a reference to one in
    /// [`Components::examples`](crate::Components::examples).
    Named(Map<RefOr<Example>>),
}

/// An object setting both of the mutually exclusive example fields.
#[derive(Debug)]
pub(crate) struct ExamplesConflict;

impl std::fmt::Display for ExamplesConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("`example` and `examples` are mutually exclusive")
    }
}

/// Folds the wire fields into one form, or says they will not go.
pub(crate) fn examples_from(
    example: Option<Value>,
    examples: Option<Map<RefOr<Example>>>,
) -> Result<Option<Examples>, ExamplesConflict> {
    match (example, examples) {
        (Some(_), Some(_)) => Err(ExamplesConflict),
        (Some(value), None) => Ok(Some(Examples::Inline(value))),
        (None, Some(named)) => Ok(Some(Examples::Named(named))),
        (None, None) => Ok(None),
    }
}

/// Splits one form back into the wire fields.
pub(crate) fn examples_into(
    examples: Option<Examples>,
) -> (Option<Value>, Option<Map<RefOr<Example>>>) {
    match examples {
        Some(Examples::Inline(value)) => (Some(value), None),
        Some(Examples::Named(named)) => (None, Some(named)),
        None => (None, None),
    }
}

/// Adds a named example to whatever an object carries already.
///
/// An inline example is dropped rather than kept beside the named one: the two
/// forms exclude each other, so there is no state that holds both.
pub(crate) fn examples_with_named(
    examples: Option<Examples>,
    name: String,
    example: RefOr<Example>,
) -> Examples {
    let mut named = match examples {
        Some(Examples::Named(named)) => named,
        Some(Examples::Inline(_)) | None => Map::new(),
    };
    named.insert(name, example);
    Examples::Named(named)
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn an_example_may_carry_no_value_at_all() {
        let example = Example::default().with_summary("described but not shown");
        assert!(example.value().is_none());
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
    fn data_may_accompany_an_external_payload() {
        let example = Example::data_external(serde_json::json!({"a": 1}), "./e.bin");
        let parsed: Example =
            serde_json::from_str(&serde_json::to_string(&example).expect("serializable"))
                .expect("deserializable");
        assert_eq!(parsed, example);
    }

    #[cfg(feature = "openapi32")]
    #[test]
    fn two_serializations_of_one_example_are_rejected() {
        let error =
            serde_json::from_str::<Example>(r#"{"serializedValue":"a","externalValue":"./e.bin"}"#)
                .expect_err("`serializedValue` is exclusive with `externalValue`");

        assert!(error.to_string().contains("mutually exclusive"));
    }
}
