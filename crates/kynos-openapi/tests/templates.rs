//! Properties over the path-template grammar.
//!
//! The oracle these compare against is built in `support/`, alongside the
//! string rather than derived from the parser: a `TemplateCase` carries the
//! normalized form and the variable list that assembling the raw string
//! recorded, so nothing here consults the parser to decide what the parser
//! should have said.

use kynos_openapi::{PathTemplate, model::paths::template::InvalidPathTemplate};
use proptest::prelude::*;

#[path = "support/mod.rs"]
mod support;
use support::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every template the generator builds parses, and parsing answers exactly
    /// what the generator put in.
    #[test]
    fn a_well_formed_template_parses(case in arb_template_case()) {
        let parsed = PathTemplate::parse(case.raw.clone());
        prop_assert!(parsed.is_ok(), "`{}` did not parse: {:?}", case.raw, parsed);
        let template = parsed.expect("just checked");

        prop_assert_eq!(template.as_str(), case.raw.as_str());
        prop_assert_eq!(template.variables(), case.variables.as_slice());
        prop_assert_eq!(template.normalized(), case.normalized);
        prop_assert_eq!(template.to_string(), case.raw);
    }

    /// Parsing is idempotent: what a template renders as parses back to it.
    #[test]
    fn parsing_a_template_is_idempotent(case in arb_template_case()) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        let reparsed = PathTemplate::parse(template.as_str()).expect("still well formed");

        prop_assert_eq!(&reparsed, &template);
        prop_assert_eq!(reparsed.normalized(), template.normalized());
        prop_assert_eq!(
            serde_json::from_str::<PathTemplate>(
                &serde_json::to_string(&template).expect("serializable")
            ).expect("readable"),
            template
        );
    }

    /// Two templates are the same path exactly when their normalized forms
    /// agree -- renaming every variable changes the template but not the path.
    #[test]
    fn normalization_identifies_paths_up_to_variable_names(case in arb_template_case()) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        let renamed = PathTemplate::parse(case.renamed).expect("well formed");

        prop_assert_eq!(renamed.normalized(), template.normalized());
        prop_assert_eq!(renamed.variables().len(), template.variables().len());
        prop_assert_eq!(renamed == template, template.variables().is_empty());
        // With no variables there is nothing to normalize away.
        if template.variables().is_empty() {
            prop_assert_eq!(template.normalized(), template.as_str());
        }
    }

    /// Two templates whose normalized forms differ are different paths, and
    /// vice versa.
    #[test]
    fn normalized_forms_agree_exactly_for_the_same_path(
        left in arb_template_case(),
        right in arb_template_case(),
    ) {
        let left_template = PathTemplate::parse(left.raw).expect("well formed");
        let right_template = PathTemplate::parse(right.raw).expect("well formed");

        prop_assert_eq!(
            left_template.normalized() == right_template.normalized(),
            left.normalized == right.normalized
        );
    }

    /// Nothing the generator deliberately malforms is accepted.
    #[test]
    fn a_malformed_template_is_rejected(raw in arb_malformed_template()) {
        prop_assert!(PathTemplate::parse(raw.clone()).is_err(), "`{}` parsed", raw);
    }

    /// A prefix concatenates exactly, and fails exactly when it repeats one of
    /// the template's variables.
    ///
    /// Both cases draw variable names from the same stems and indices, so the
    /// two sets intersect often enough for both arms to run.
    #[test]
    fn prefixing_produces_a_template_or_an_error(
        case in arb_template_case(),
        prefix in arb_template_case(),
    ) {
        let template = PathTemplate::parse(case.raw.clone()).expect("well formed");
        // Trimming leaves either nothing or a template with no trailing `/`, so
        // joining it to a template that begins with `/` adds no empty segment:
        // a repeated variable is the only way the result can be malformed.
        let joined = format!("{}{}", prefix.raw.trim_end_matches('/'), case.raw);
        let shared: Vec<&String> = prefix
            .variables
            .iter()
            .filter(|name| case.variables.contains(name))
            .collect();

        match template.with_prefix(&prefix.raw) {
            Ok(prefixed) => {
                prop_assert!(shared.is_empty(), "`{}` accepted shared {:?}", joined, shared);
                prop_assert_eq!(prefixed.as_str(), joined.as_str());
                // A prefix contributes its own variables, ahead of these.
                let variables: Vec<String> =
                    prefix.variables.iter().chain(&case.variables).cloned().collect();
                prop_assert_eq!(prefixed.variables(), variables.as_slice());
            }
            Err(InvalidPathTemplate::DuplicateVariable { template: offending, name }) => {
                prop_assert_eq!(offending, joined);
                prop_assert!(shared.contains(&&name), "`{}` is not shared: {:?}", name, shared);
            }
            Err(other) => prop_assert!(false, "`{}` refused for {:?}", joined, other),
        }
    }
}
