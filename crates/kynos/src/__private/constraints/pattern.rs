//! The `pattern` check, behind the `pattern` feature.
//!
//! The derive translates each pattern from ECMA-262 to the `regex` dialect and
//! compiles it when it expands. Each is a `static`, compiled on first check.

use std::sync::OnceLock;

use regex::Regex;

use crate::schema::constraints::{Pointer, Textual, Violations};

/// One field's `pattern`, compiled once per process.
#[doc(hidden)]
pub struct Pattern {
    /// The pattern as declared, which the description emits and a violation
    /// names.
    declared: &'static str,
    /// [`declared`](Self::declared) in the engine's dialect.
    translated: &'static str,
    compiled: OnceLock<Regex>,
}

impl Pattern {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(declared: &'static str, translated: &'static str) -> Self {
        Self {
            declared,
            translated,
            compiled: OnceLock::new(),
        }
    }

    /// Whether `text` holds a match anywhere, since JSON Schema does not
    /// anchor a pattern.
    fn is_match(&self, text: &str) -> bool {
        self.compiled
            .get_or_init(|| {
                // The derive already compiled this string with this engine.
                Regex::new(self.translated).expect("the derive compiled this pattern")
            })
            .is_match(text)
    }
}

impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Pattern").field(&self.declared).finish()
    }
}

#[doc(hidden)]
pub fn check<T: Textual + ?Sized>(
    value: &T,
    pattern: &Pattern,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    if value.text().is_some_and(|text| !pattern.is_match(text)) {
        violations.report(at, format!("must match the pattern `{}`", pattern.declared));
    }
}
