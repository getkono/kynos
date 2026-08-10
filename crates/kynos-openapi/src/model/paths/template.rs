//! Path templating: parsing, normalization and prefixing.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The error returned when a path template is malformed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
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

    /// A literal segment contained a character the path grammar forbids.
    ///
    /// Outside a `{}` expression a template may only carry `pchar`: letters,
    /// digits, `-._~`, the sub-delimiters `!$&'()*+,;=`, `:`, `@`, and
    /// percent-encoded triples. Anything else — including any non-ASCII
    /// character — has to arrive percent-encoded.
    #[error(
        "path template `{template}` contains `{character}` outside a `{{}}` expression, which the \
         path grammar does not allow"
    )]
    IllegalLiteralCharacter {
        /// The offending template.
        template: String,
        /// The character that is not allowed there.
        character: char,
    },

    /// A `%` was not followed by two hexadecimal digits.
    #[error("path template `{0}` contains a `%` that does not introduce a percent-encoded triple")]
    MalformedPercentEncoding(String),

    /// Two `/` met with nothing between them.
    ///
    /// A path segment always holds at least one character. A *trailing* `/` is
    /// legal, because the grammar makes the final segment optional.
    #[error("path template `{0}` has an empty segment")]
    EmptySegment(String),
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

/// Whether `character` is `pchar`, the only thing a path literal may hold.
///
/// `pchar = unreserved / pct-encoded / sub-delims / ":" / "@"`, per RFC 3986
/// section 3.3. `%` is handled by the caller, which has the following two
/// characters in hand.
const fn is_path_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '-' | '.'
                | '_'
                | '~'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | ':'
                | '@'
        )
}

/// Checks one literal run of a template, between `{}` expressions.
///
/// `/` is the segment separator rather than `pchar`, so it is allowed here and
/// segmentation is left to callers that care about it.
fn check_literal(literal: &str, raw: &str) -> Result<(), InvalidPathTemplate> {
    let mut characters = literal.chars();
    while let Some(character) = characters.next() {
        match character {
            '/' => {}
            // Not `pchar` either, but a closing brace outside an expression is
            // a brace mistake wherever it appears, and reporting it as a stray
            // character would name the wrong problem.
            '}' => return Err(InvalidPathTemplate::UnbalancedBraces(raw.to_owned())),
            '%' => {
                let high = characters.next();
                let low = characters.next();
                if !matches!((high, low), (Some(high), Some(low))
                    if high.is_ascii_hexdigit() && low.is_ascii_hexdigit())
                {
                    return Err(InvalidPathTemplate::MalformedPercentEncoding(
                        raw.to_owned(),
                    ));
                }
            }
            _ if is_path_character(character) => {}
            _ => {
                return Err(InvalidPathTemplate::IllegalLiteralCharacter {
                    template: raw.to_owned(),
                    character,
                });
            }
        }
    }
    Ok(())
}

/// Rejects a segment with nothing in it.
///
/// `path-template = "/" *( path-segment "/" ) [ path-segment ]` and
/// `path-segment = 1*( path-literal / template-expression )`, so two `/` never
/// meet. A *trailing* `/` is legal, because the final segment is optional --
/// and `/users` and `/users/` are different paths, which is what makes the
/// trailing-slash policy an application-level decision rather than a parse
/// question.
///
/// A variable name may itself contain a `/`, so this cannot be a split.
fn check_segments(raw: &str) -> Result<(), InvalidPathTemplate> {
    let mut in_expression = false;
    let mut segment_is_empty = true;

    // The leading `/` opens the first segment rather than closing one.
    for character in raw.chars().skip(1) {
        match character {
            '{' => {
                in_expression = true;
                segment_is_empty = false;
            }
            '}' => in_expression = false,
            '/' if !in_expression => {
                if segment_is_empty {
                    return Err(InvalidPathTemplate::EmptySegment(raw.to_owned()));
                }
                segment_is_empty = true;
            }
            _ => segment_is_empty = false,
        }
    }

    Ok(())
}

impl PathTemplate {
    /// Parses a path template.
    ///
    /// Literal segments are checked against the path grammar; variable names
    /// are not, because the grammar admits every character except a brace
    /// there. A name that Kynos's router cannot match — a catch-all, say — is
    /// still a legal OpenAPI template, and this type has to be able to hold one
    /// so that an externally authored description round-trips. That narrower
    /// contract is enforced where routes are registered.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPathTemplate`] when the template does not start with
    /// `/`, has unbalanced or empty braces, repeats a variable, carries a query
    /// string or fragment, or holds a character the path grammar does not allow
    /// outside a `{}` expression.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidPathTemplate> {
        let raw = raw.into();

        if !raw.starts_with('/') {
            return Err(InvalidPathTemplate::MissingLeadingSlash(raw));
        }
        // `?` and `#` are not `pchar` either, but a template carrying one is
        // more likely a URL pasted whole than a stray character, so it keeps
        // the error that says so.
        if raw.contains('?') || raw.contains('#') {
            return Err(InvalidPathTemplate::NotAPath(raw));
        }

        let mut variables = Vec::new();
        let mut rest = raw.as_str();
        while let Some(open) = rest.find('{') {
            check_literal(&rest[..open], &raw)?;
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
        check_literal(rest, &raw)?;
        check_segments(&raw)?;

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
