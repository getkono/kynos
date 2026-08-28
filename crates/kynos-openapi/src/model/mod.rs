//! The OpenAPI object model.
//!
//! Data and invariant-preserving constructors, and nothing else. Producing an
//! artifact from this model lives in [`crate::emit`]; checking one against the
//! specification lives in [`crate::validate`].
//!
//! This is the subtree that would become a standalone IR crate if the
//! satellite-crate boundary described in `docs/architecture.md` is ever drawn.

pub mod body;
pub mod callback;
pub mod components;
pub mod document;
pub mod example;
pub mod extensions;
pub mod external_docs;
pub mod info;
pub mod link;
// Private: it declares one deserializer the model's own fields point at, and
// nothing a caller has a path to.
mod nullable;
pub mod parameter;
pub mod paths;
pub mod reference;
pub mod response;
pub mod schema;
pub mod security;
pub mod server;
pub mod tag;

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    const GATE: &str = "#[cfg(feature = \"openapi32\")]";

    /// Whether `line` declares a field rather than a variant or an arm.
    fn declares_a_field(line: &str) -> bool {
        let declaration = line.strip_prefix("pub ").unwrap_or(line);
        let Some((identifier, rest)) = declaration.split_once(": ") else {
            return false;
        };

        line.ends_with(',')
            && !identifier.is_empty()
            && identifier.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            && identifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && rest.starts_with(|c: char| c.is_ascii_uppercase())
    }

    /// Whether `line` opens a struct-shaped enum variant, such as `ApiKey {`.
    fn opens_a_variant(line: &str) -> bool {
        line.ends_with('{')
            && line
                .split_whitespace()
                .next()
                .is_some_and(|first| first.starts_with(|c: char| c.is_ascii_uppercase()))
    }

    /// Whether `line` falls inside the braced body opened at `header`.
    fn within_body(lines: &[&str], header: usize, line: usize) -> bool {
        let mut depth = 0i32;
        for (index, current) in lines.iter().enumerate().skip(header) {
            depth += i32::try_from(current.matches('{').count()).expect("a short line")
                - i32::try_from(current.matches('}').count()).expect("a short line");
            if index >= header && depth == 0 {
                return line <= index;
            }
        }
        true
    }

    /// Every enum variant that gains a field behind `openapi32`, paired with
    /// whether that variant carries `#[non_exhaustive]`.
    fn variants_gaining_a_gated_field() -> Vec<(String, bool)> {
        fn walk(directory: &Path, found: &mut Vec<(String, bool)>) {
            let entries = fs::read_dir(directory).expect("the model sources are beside this test");
            for entry in entries.map(|entry| entry.expect("a readable directory entry")) {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    collect(
                        &fs::read_to_string(&path).expect("a readable source file"),
                        found,
                    );
                }
            }
        }

        fn collect(source: &str, found: &mut Vec<(String, bool)>) {
            let lines: Vec<&str> = source.lines().map(str::trim).collect();

            for (index, line) in lines.iter().enumerate() {
                if *line != GATE {
                    continue;
                }

                // Step past any further attributes and documentation to reach
                // whatever this gate actually gates.
                let gated = lines[index + 1..]
                    .iter()
                    .find(|line| !line.starts_with("#[") && !line.starts_with("//"));
                if !gated.is_some_and(|line| declares_a_field(line)) {
                    continue;
                }

                // The item the field lives in. Only an enum can have variants,
                // and a gated field in a struct is the ordinary case that
                // `#[non_exhaustive]` on the struct would not help with anyway.
                let Some(item) = lines[..index].iter().rposition(|line| {
                    let declaration = line.strip_prefix("pub ").unwrap_or(line);
                    declaration.starts_with("enum ") || declaration.starts_with("struct ")
                }) else {
                    continue;
                };
                let declaration = lines[item].strip_prefix("pub ").unwrap_or(lines[item]);
                let Some(enum_name) = declaration
                    .strip_prefix("enum ")
                    .and_then(|rest| rest.split_whitespace().next())
                else {
                    continue;
                };

                // Only within the enum's own body. Past its closing brace sit
                // the `impl` blocks, whose constructors write `deprecated:
                // None,` -- a struct literal that has the shape of a field
                // declaration and is not one.
                if !within_body(&lines, item, index) {
                    continue;
                }

                // The variant the field lives in, searched only within the
                // enum so a struct's fields above cannot be mistaken for one.
                let Some(offset) = lines[item + 1..index]
                    .iter()
                    .rposition(|line| opens_a_variant(line))
                else {
                    continue;
                };
                let variant = item + 1 + offset;
                let name = lines[variant]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('{');

                let non_exhaustive = lines[..variant]
                    .iter()
                    .rev()
                    .take_while(|line| line.starts_with("#[") || line.starts_with("//"))
                    .any(|line| *line == "#[non_exhaustive]");

                found.push((format!("{enum_name}::{name}"), non_exhaustive));
            }
        }

        let mut found = Vec::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/model"),
            &mut found,
        );
        assert!(
            !found.is_empty(),
            "the scan found no gated variant field, so it is measuring nothing"
        );
        found
    }

    /// A variant that gains a field behind `openapi32` is `#[non_exhaustive]`.
    ///
    /// `#[non_exhaustive]` on an *enum* keeps a downstream `match` compiling
    /// when a variant is added. It says nothing about a variant's *field list*,
    /// and six variants gain a field behind the gate. Without the attribute on
    /// the variant itself, a downstream
    /// `SecurityScheme::Http { scheme, bearer_format, description, extensions }`
    /// — as a pattern or as a literal — stops compiling the moment anything
    /// else in the build turns `openapi32` on, which is precisely what
    /// "additive" is supposed to rule out. Cargo unifies features across a
    /// dependency graph, so "anything else" includes crates the author never
    /// named.
    ///
    /// Read off the source rather than transcribed, so a seventh variant is
    /// covered by having been written rather than by being remembered here.
    #[test]
    fn every_variant_that_gains_a_gated_field_is_non_exhaustive() {
        let exposed: Vec<String> = variants_gaining_a_gated_field()
            .into_iter()
            .filter(|(_, non_exhaustive)| !non_exhaustive)
            .map(|(name, _)| name)
            .collect();

        assert!(
            exposed.is_empty(),
            "{exposed:?} gain a field behind `openapi32` and are not `#[non_exhaustive]`, so \
             a downstream pattern or literal naming every field of one stops compiling when \
             an unrelated crate enables the feature"
        );
    }
}
