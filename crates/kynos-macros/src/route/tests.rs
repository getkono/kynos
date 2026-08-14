use quote::{ToTokens, quote};

use crate::route::{args::RouteArgs, attrs::split_doc, uri::endpoint_uri_impl};

fn lines(input: &[&str]) -> Vec<String> {
    input.iter().map(|line| (*line).to_owned()).collect()
}

/// Splitting a doc comment into its summary and its description.
///
/// The sweep below proves the split is total over every arrangement of blank
/// lines; the cases after it pin the characters the two halves are joined with.
/// Neither restates the other: the sweep reads words and never looks at the
/// separators, and the cases fix separators the sweep cannot see.
mod doc_comments {
    use super::{lines, split_doc};

    /// The whitespace-separated words of a half, absent or not.
    ///
    /// Blind to the join character on purpose, so that this and the cases below
    /// divide the work rather than share it.
    fn words(text: Option<&str>) -> Vec<String> {
        text.map(|text| text.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Every arrangement of blank and non-blank lines, up to five lines.
    ///
    /// The expectation is built from the pattern rather than from `split_doc`:
    /// each non-blank line carries a word naming its own index, so where a word
    /// comes out says which half claimed it. Five lines is enough to hold a
    /// leading blank, a trailing blank, and a doubled blank between two
    /// paragraphs at the same time, which are the arrangements that differ.
    ///
    /// A sweep rather than a sampled property: the space is 2^5 patterns, and
    /// enumerating it is total where drawing from it would not be.
    #[test]
    fn every_arrangement_of_blank_lines_splits_without_losing_a_word() {
        for width in 0..=5usize {
            for pattern in 0..(1usize << width) {
                let blank: Vec<bool> = (0..width).map(|bit| pattern & (1 << bit) != 0).collect();
                let rendered: Vec<String> = blank
                    .iter()
                    .enumerate()
                    .map(|(index, &is_blank)| {
                        // Whitespace rather than "", so that a line that only
                        // looks blank is swept too.
                        if is_blank {
                            "  ".to_owned()
                        } else {
                            format!(" w{index}")
                        }
                    })
                    .collect();

                let (summary, description) = split_doc(&rendered);

                // The first paragraph is whatever precedes the first blank line,
                // so every line before it is non-blank by construction.
                let boundary = blank.iter().position(|&is_blank| is_blank).unwrap_or(width);
                let expected_summary: Vec<String> =
                    (0..boundary).map(|index| format!("w{index}")).collect();
                let expected_description: Vec<String> = ((boundary + 1)..width)
                    .filter(|index| !blank[*index])
                    .map(|index| format!("w{index}"))
                    .collect();

                assert_eq!(
                    words(summary.as_deref()),
                    expected_summary,
                    "summary for {blank:?}"
                );
                assert_eq!(
                    words(description.as_deref()),
                    expected_description,
                    "description for {blank:?}"
                );

                // An empty half is absent, never present and blank: `Some("")`
                // would reach the description as a field that renders as an
                // empty string rather than one that is omitted.
                assert_ne!(summary.as_deref(), Some(""), "summary for {blank:?}");
                assert_ne!(
                    description.as_deref(),
                    Some(""),
                    "description for {blank:?}"
                );
            }
        }
    }

    #[test]
    fn a_single_line_doc_comment_is_all_summary() {
        let (summary, description) = split_doc(&lines(&[" Fetch a single user."]));
        assert_eq!(summary.as_deref(), Some("Fetch a single user."));
        assert_eq!(description, None);
    }

    #[test]
    fn the_first_paragraph_is_the_summary_and_the_rest_the_description() {
        let (summary, description) = split_doc(&lines(&[
            " Fetch a single user.",
            "",
            " Includes soft-deleted accounts.",
            " Requires the `users:read` scope.",
        ]));
        assert_eq!(summary.as_deref(), Some("Fetch a single user."));
        assert_eq!(
            description.as_deref(),
            Some("Includes soft-deleted accounts.\nRequires the `users:read` scope.")
        );
    }

    #[test]
    fn a_wrapped_first_paragraph_joins_into_one_summary() {
        let (summary, description) = split_doc(&lines(&[
            " Fetch a single user by",
            " its identifier.",
            "",
            " More detail.",
        ]));
        assert_eq!(
            summary.as_deref(),
            Some("Fetch a single user by its identifier.")
        );
        assert_eq!(description.as_deref(), Some("More detail."));
    }

    /// A blank line inside the description is kept, because it separates
    /// paragraphs the specification renders as written.
    #[test]
    fn a_paragraph_break_survives_into_the_description() {
        let (_, description) = split_doc(&lines(&[" Summary.", "", " First.", "", " Second."]));
        assert_eq!(description.as_deref(), Some("First.\n\nSecond."));
    }

    #[test]
    fn an_absent_doc_comment_yields_neither() {
        let (summary, description) = split_doc(&[]);
        assert_eq!(summary, None);
        assert_eq!(description, None);
    }

    #[test]
    fn trailing_blank_lines_do_not_become_an_empty_description() {
        let (summary, description) = split_doc(&lines(&[" Fetch a user.", "", "  "]));
        assert_eq!(summary.as_deref(), Some("Fetch a user."));
        assert_eq!(description, None);
    }
}

#[test]
fn endpoint_uri_uses_the_exact_extracted_parameter_types() {
    let function: syn::ItemFn = syn::parse_quote! {
        async fn report(Path(path): Path<ReportPath>, Query(query): Query<ReportQuery>) {}
    };
    let expansion = endpoint_uri_impl(&function, "/reports/{name}", &["name".to_owned()])
        .expect("valid endpoint")
        .into_token_stream()
        .to_string();

    assert!(expansion.contains("pub fn relative_uri (path : ReportPath , query : ReportQuery ,)"));
}

#[test]
fn endpoint_uri_rejects_a_template_without_a_path_extractor() {
    let function: syn::ItemFn = syn::parse_quote! {
        async fn report() {}
    };
    let error = endpoint_uri_impl(&function, "/reports/{name}", &["name".to_owned()])
        .expect_err("missing Path<T> must fail");

    assert!(error.to_string().contains("no Path<T> extractor"));
}

#[test]
fn catch_panics_is_a_bare_route_option() {
    let args =
        RouteArgs::parse(quote!(path = "/health", catch_panics)).expect("valid route arguments");

    assert!(args.catch_panics);
}

/// `method` belongs to `#[kynos::operation]` alone. The shared parser must
/// keep rejecting it, so that `#[kynos::get("/x", method = "POST")]` cannot
/// serve one method while the description names another.
#[test]
fn a_per_method_attribute_rejects_a_method_argument() {
    let Err(error) = RouteArgs::parse(quote!(path = "/health", method = "POST")) else {
        panic!("a per-method attribute must not accept `method`")
    };

    assert!(error.to_string().contains("unknown route argument"));
}

#[test]
fn an_unknown_route_argument_is_still_rejected() {
    let Err(error) = RouteArgs::parse(quote!(path = "/health", nonsense = "x")) else {
        panic!("an argument no attribute reads must not be silently ignored")
    };

    assert!(error.to_string().contains("unknown route argument"));
}
