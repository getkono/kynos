use quote::{ToTokens, quote};

use crate::route::{args::RouteArgs, attrs::split_doc, uri::endpoint_uri_impl};

fn lines(input: &[&str]) -> Vec<String> {
    input.iter().map(|line| (*line).to_owned()).collect()
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

#[test]
fn an_absent_doc_comment_yields_neither() {
    let (summary, description) = split_doc(&[]);
    assert_eq!(summary, None);
    assert_eq!(description, None);
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

    assert!(expansion.contains("pub fn uri (path : ReportPath , query : ReportQuery)"));
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
fn trailing_blank_lines_do_not_become_an_empty_description() {
    let (summary, description) = split_doc(&lines(&[" Fetch a user.", "", "  "]));
    assert_eq!(summary.as_deref(), Some("Fetch a user."));
    assert_eq!(description, None);
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
