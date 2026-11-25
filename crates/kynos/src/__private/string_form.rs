//! What a `#[schema(string)]` type's `schema` checks of the examples it lists.

use std::{fmt::Display, str::FromStr};

use crate::{
    router::error::ExampleRefusal,
    schema::{
        Schema,
        constraints::{Pointer, Violations},
        registry::Registry,
    },
};

/// Records on `registry` each of `examples` that `T`'s own `FromStr` refuses,
/// or whose value breaks `T`'s bounds, for the router to report.
pub fn admit_examples<T: Schema + FromStr + Display>(registry: &mut Registry, examples: &[&str]) {
    for (index, example) in examples.iter().enumerate() {
        let refusal = match example.parse::<T>() {
            Err(_) => ExampleRefusal::Unparsed,
            Ok(read) => match Violations::of(&read, Pointer::root()) {
                Ok(()) => continue,
                Err(failures) => ExampleRefusal::OutOfBounds {
                    failures: failures.into_values().collect(),
                },
            },
        };
        registry.refuse_example::<T>(index, example, refusal);
    }
}
