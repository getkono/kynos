//! The `pattern` check, behind the `pattern` feature.
//!
//! The derive translates each field's pattern from ECMA-262 to the `regex`
//! dialect and compiles it when it expands, so a pattern reaching here is one
//! the engine compiles. Each field's pattern is a `static` the expansion
//! declares, and is compiled the first time a value is checked against it
//! rather than per request.
//!
//! A map key's pattern is a run-time value of `MapKey::key_constraints`, and
//! Rust has no generic statics to hold one per key type, so `key` translates
//! and compiles each distinct pattern once per process and keeps it under its
//! source. The router calls it while it is built, which refuses a pattern that
//! does not translate and leaves the compiled one in place for the first
//! request.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock},
};

use kynos_openapi::pattern::translate;
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
                // The derive compiled this same string with this same engine
                // version and configuration, so this cannot fail.
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

/// Every map key pattern compiled so far, keyed by its ECMA-262 source.
///
/// A key type's pattern is fixed in practice, so this holds as many entries as
/// there are key types declaring one, and a request finds its pattern under a
/// read lock.
static KEYS: LazyLock<RwLock<HashMap<String, Arc<Regex>>>> = LazyLock::new(RwLock::default);

/// The map key pattern `declared`, an ECMA-262 regular expression, compiled
/// once per process.
///
/// # Errors
///
/// Why `declared` does not translate, which is not cached, since the router
/// refuses it while it is built.
pub(crate) fn key(declared: &str) -> Result<Arc<Regex>, String> {
    // A poisoned lock guards a map only ever inserted into whole, so what it
    // holds is still sound.
    if let Some(compiled) = KEYS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(declared)
    {
        return Ok(Arc::clone(compiled));
    }

    let translated = translate(declared).map_err(|refusal| refusal.to_string())?;
    let compiled = Regex::new(&translated)
        .map_err(|error| format!("the `regex` engine cannot compile this pattern: {error}"))?;
    Ok(Arc::clone(
        KEYS.write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(declared.to_owned())
            .or_insert_with(|| Arc::new(compiled)),
    ))
}

/// A map key type's `pattern`, looked up once per map checked rather than per
/// key.
#[derive(Debug)]
pub(crate) struct KeyPattern {
    /// The pattern as declared, which a violation names.
    declared: String,
    compiled: Result<Arc<Regex>, String>,
}

impl KeyPattern {
    pub(crate) fn new(declared: String) -> Self {
        let compiled = key(&declared);
        Self { declared, compiled }
    }

    /// Checks a key, `text`, reporting at `at`.
    pub(crate) fn check(&self, text: &str, at: Pointer<'_>, violations: &mut Violations) {
        let declared = &self.declared;
        match &self.compiled {
            Ok(compiled) if compiled.is_match(text) => {}
            Ok(_) => violations.report(at, format!("must match the pattern `{declared}`")),
            // Reachable only outside a router, which refuses the pattern while
            // it is built. A bound nothing can check is not one a key meets.
            Err(refusal) => violations.report(
                at,
                format!("cannot be checked against the pattern `{declared}`: {refusal}"),
            ),
        }
    }
}
