//! Properties over the path-template grammar.
//!
//! The oracle these compare against is built in `support/`, alongside the
//! string rather than derived from the parser: a `TemplateCase` carries the
//! normalized form and the variable list that assembling the raw string
//! recorded, so nothing here consults the parser to decide what the parser
//! should have said.

use kynos_openapi::PathTemplate;
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

    /// A prefix concatenates, or fails for a reason the grammar states.
    #[test]
    fn prefixing_produces_a_template_or_an_error(
        case in arb_template_case(),
        prefix in arb_template(),
    ) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        if let Ok(prefixed) = template.with_prefix(&prefix) {
            prop_assert!(prefixed.as_str().ends_with(template.as_str()));
            // A prefix contributes its own variables, ahead of these.
            prop_assert!(prefixed.variables().ends_with(template.variables()));
        }
    }
}
