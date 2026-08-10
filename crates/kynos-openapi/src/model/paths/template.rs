//! Path templating: parsing, normalization and prefixing.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The error returned when a path template is malformed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPathTemplate {
    /// The template did not begin with `/`.
    #[error("path template `{0}` must begin with `/`")]
    MissingLeadingSlash(String),

    /// A `{` was opened but never closed, or a `}` appeared unopened.
    #[error("path template `{0}` has unbalanced braces")]
    UnbalancedBraces(String),

    /// A `{}` expression contained no name.
    #[error("path template `{0}` contains an empty `{{}}` expression")]
    EmptyExpression(String),

    /// The same variable name appeared more than once.
    ///
    /// A template expression must not be repeated within one path.
    #[error("path template `{template}` repeats the variable `{name}`")]
    DuplicateVariable {
        /// The offending template.
        template: String,
        /// The variable that appeared more than once.
        name: String,
    },

    /// The template contained a query string or fragment.
    #[error("path template `{0}` must not contain a query string or fragment")]
    NotAPath(String),
}

/// A parsed path template such as `/users/{id}/posts/{postId}`.
///
/// Two templates that differ only in variable name are *the same path* as far
/// as OpenAPI is concerned, so declaring both is invalid.
/// [`normalized`](PathTemplate::normalized) exists to make that comparison
/// cheap.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathTemplate {
    raw: String,
    variables: Vec<String>,
}

impl PathTemplate {
    /// Parses a path template.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathTemplate`] when the template does not start with
    /// `/`, has unbalanced or empty braces, repeats a variable, or carries a
    /// query string or fragment.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidPathTemplate> {
        let raw = raw.into();

        if !raw.starts_with('/') {
            return Err(InvalidPathTemplate::MissingLeadingSlash(raw));
        }
        if raw.contains('?') || raw.contains('#') {
            return Err(InvalidPathTemplate::NotAPath(raw));
        }

        let mut variables = Vec::new();
        let mut rest = raw.as_str();
        while let Some(open) = rest.find('{') {
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('}') else {
                return Err(InvalidPathTemplate::UnbalancedBraces(raw));
            };
            let name = &after_open[..close];
            if name.is_empty() {
                return Err(InvalidPathTemplate::EmptyExpression(raw));
            }
            if name.contains('{') {
                return Err(InvalidPathTemplate::UnbalancedBraces(raw));
            }
            if variables.iter().any(|existing| existing == name) {
                return Err(InvalidPathTemplate::DuplicateVariable {
                    name: name.to_owned(),
                    template: raw,
                });
            }
            variables.push(name.to_owned());
            rest = &after_open[close + 1..];
        }
        if rest.contains('}') {
            return Err(InvalidPathTemplate::UnbalancedBraces(raw));
        }

        Ok(Self { raw, variables })
    }

    /// The template exactly as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The variable names, in the order they appear.
    #[must_use]
    pub fn variables(&self) -> &[String] {
        &self.variables
    }

    /// The template with every variable name replaced by `{}`.
    ///
    /// Two templates are the same path if and only if their normalized forms
    /// are equal.
    #[must_use]
    pub fn normalized(&self) -> String {
        let mut out = String::with_capacity(self.raw.len());
        let mut rest = self.raw.as_str();
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            out.push_str("{}");
            let after_open = &rest[open + 1..];
            let close = after_open.find('}').expect("parse validated the braces");
            rest = &after_open[close + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Concatenates a prefix onto this template, as nesting does.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathTemplate`] when the result is not a valid template,
    /// which is how a prefix that repeats one of this template's variables is
    /// caught.
    pub fn with_prefix(&self, prefix: &str) -> Result<Self, InvalidPathTemplate> {
        let prefix = prefix.trim_end_matches('/');
        if prefix.is_empty() {
            return Ok(self.clone());
        }
        Self::parse(format!("{prefix}{}", self.raw))
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for PathTemplate {
    type Err = InvalidPathTemplate;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for PathTemplate {
    type Error = InvalidPathTemplate;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<PathTemplate> for String {
    fn from(template: PathTemplate) -> Self {
        template.raw
    }
}
