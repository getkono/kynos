//! The Server and Server Variable Objects.

use serde::{Deserialize, Serialize};

use crate::{Map, extensions::Extensions};

/// A server hosting the API.
///
/// Kynos never infers this from a bind address. The description states the
/// public URL clients use, which is frequently not the socket the process
/// listens on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    /// A URL to the target host, optionally templated with `{variable}`.
    ///
    /// May be relative to the location the description is served from. Query
    /// string and fragment components are not permitted.
    pub url: String,

    /// A name for the server, for use by tooling.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// A description of the host designated by the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A map between a variable name and its value, for URL substitution.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub variables: Map<ServerVariable>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Server {
    /// Creates a server at the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Declares a substitution variable used in the URL template.
    #[must_use]
    pub fn with_variable(mut self, name: impl Into<String>, variable: ServerVariable) -> Self {
        self.variables.insert(name.into(), variable);
        self
    }
}

/// A substitution variable for a templated [`Server::url`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerVariable {
    /// The set of values this variable may take.
    ///
    /// When present it must not be empty, and must contain
    /// [`default_value`](ServerVariable::default_value).
    #[serde(rename = "enum", default, skip_serializing_if = "Option::is_none")]
    pub enumeration: Option<Vec<String>>,

    /// The value to use for substitution when none is supplied.
    ///
    /// Unlike JSON Schema's `default`, this field is required.
    #[serde(rename = "default")]
    pub default_value: String,

    /// A description of this variable. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl ServerVariable {
    /// Creates a free-form variable with the given default.
    pub fn new(default_value: impl Into<String>) -> Self {
        Self {
            default_value: default_value.into(),
            ..Self::default()
        }
    }

    /// Creates a variable constrained to a fixed set of values.
    pub fn enumerated(
        default_value: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            enumeration: Some(values.into_iter().map(Into::into).collect()),
            default_value: default_value.into(),
            description: None,
            extensions: Extensions::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Server, ServerVariable};

    #[test]
    fn a_bare_server_serializes_to_just_its_url() {
        let json = serde_json::to_string(&Server::new("https://api.example.com")).expect("ok");
        assert_eq!(json, r#"{"url":"https://api.example.com"}"#);
    }

    #[test]
    fn enumerated_variables_carry_their_value_set() {
        let variable = ServerVariable::enumerated("v1", ["v1", "v2"]);
        assert_eq!(variable.default_value, "v1");
        assert_eq!(
            variable.enumeration.as_deref(),
            Some(&["v1".to_owned(), "v2".to_owned()][..])
        );
    }
}
