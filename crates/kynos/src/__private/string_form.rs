//! What a `#[schema(string)]` type's `schema` checks of the examples it lists.
//!
//! serde is probed by autoref specialization: the expansion names
//! `(&Serde(value)).written()`, which resolves to [`Written`] where the type
//! implements `Serialize` and to [`Unwritten`], answering `None`, where it does
//! not, so a type with no serde still compiles. Likewise [`Reads`] and
//! [`Unread`] for `DeserializeOwned`. The type is concrete there, which the
//! resolution needs; `#[schema(string)]` refuses a type parameter.

use std::{fmt::Display, str::FromStr};

use crate::{
    router::error::ExampleRefusal,
    schema::{
        Schema,
        constraints::{Pointer, Violations},
        registry::Registry,
    },
};

/// What `T`'s serde writes a value as, and whether it reads a text as a JSON
/// string: each `None` where `T` does not implement the trait.
pub type Wire = (
    Option<Result<serde_json::Value, String>>,
    Option<Result<(), String>>,
);

/// A value whose serde [`Written`] and [`Reads`] probe.
pub struct Serde<'a, T>(pub &'a T);

impl<T> std::fmt::Debug for Serde<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Serde(..)")
    }
}

/// The JSON a `Serialize` type writes a value as.
pub trait Written {
    /// The JSON written, or why serde refused.
    fn written(&self) -> Option<Result<serde_json::Value, String>>;
}

impl<T: serde::Serialize> Written for Serde<'_, T> {
    fn written(&self) -> Option<Result<serde_json::Value, String>> {
        Some(serde_json::to_value(self.0).map_err(|error| error.to_string()))
    }
}

/// The fallback of [`Written`], for a type with no `Serialize`.
pub trait Unwritten {
    /// Nothing, since nothing is written.
    fn written(&self) -> Option<Result<serde_json::Value, String>> {
        None
    }
}

impl<T> Unwritten for &Serde<'_, T> {}

/// Whether a `DeserializeOwned` type reads a text as a JSON string.
pub trait Reads {
    /// Whether serde reads `text`, or why it refused.
    fn reads(&self, text: &str) -> Option<Result<(), String>>;
}

impl<T: serde::de::DeserializeOwned> Reads for Serde<'_, T> {
    fn reads(&self, text: &str) -> Option<Result<(), String>> {
        Some(
            serde_json::from_value::<T>(serde_json::Value::String(text.to_owned()))
                .map(drop)
                .map_err(|error| error.to_string()),
        )
    }
}

/// The fallback of [`Reads`], for a type with no `DeserializeOwned`.
pub trait Unread {
    /// Nothing, since nothing is read.
    fn reads(&self, text: &str) -> Option<Result<(), String>> {
        let _ = text;
        None
    }
}

impl<T> Unread for &Serde<'_, T> {}

/// Records on `registry` each of `examples` that `T` does not carry as the
/// string described: one its own `FromStr` refuses, whose value breaks `T`'s
/// bounds, that its `Serialize` writes as anything but the JSON string its
/// `Display` writes, or that its `Deserialize` does not read as a JSON string.
/// `wire` reports `T`'s serde, from the expansion's probes.
pub fn admit_examples<T: Schema + FromStr + Display>(
    registry: &mut Registry,
    examples: &[&str],
    wire: impl Fn(&T, &str) -> Wire,
) {
    for (index, example) in examples.iter().enumerate() {
        let Some(refusal) = refusal(example, &wire) else {
            continue;
        };
        registry.refuse_example::<T>(index, example, refusal);
    }
}

/// Why `T` does not carry `example`, if it does not.
fn refusal<T: Schema + FromStr + Display>(
    example: &str,
    wire: impl Fn(&T, &str) -> Wire,
) -> Option<ExampleRefusal> {
    let Ok(read) = example.parse::<T>() else {
        return Some(ExampleRefusal::Unparsed);
    };
    if let Err(failures) = Violations::of(&read, Pointer::root()) {
        return Some(ExampleRefusal::OutOfBounds {
            failures: failures.into_values().collect(),
        });
    }
    let (written, reads) = wire(&read, example);
    match written {
        Some(Err(reason)) => return Some(ExampleRefusal::Unserializable { reason }),
        Some(Ok(written)) => {
            let displayed = read.to_string();
            if written.as_str() != Some(displayed.as_str()) {
                return Some(ExampleRefusal::Miswritten {
                    written: written.to_string(),
                    displayed,
                });
            }
        }
        None => {}
    }
    match reads {
        Some(Err(reason)) => Some(ExampleRefusal::Undeserializable { reason }),
        Some(Ok(())) | None => None,
    }
}
