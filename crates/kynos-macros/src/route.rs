//! Expansion of the route attributes and the `routes!` / `path!` macros.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Expr, ExprLit, Ident, ItemFn, Lit, LitStr, Meta, Token, parse::Parser, parse_macro_input,
    punctuated::Punctuated, spanned::Spanned,
};

/// The methods that have a dedicated Path Item field in OpenAPI 3.1.
const STANDARD_METHODS: &[&str] = &[
    "GET", "PUT", "POST", "DELETE", "OPTIONS", "HEAD", "PATCH", "TRACE",
];

/// The arguments a route attribute accepts.
struct RouteArgs {
    path: LitStr,
    operation_id: Option<LitStr>,
    tag: Option<Ident>,
}

impl RouteArgs {
    /// Parses `("/users/{id}", operation_id = "getUser", tag = Users)`.
    fn parse(tokens: TokenStream2) -> syn::Result<Self> {
        let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
        let items = parser.parse2(tokens)?;

        let mut path = None;
        let mut operation_id = None;
        let mut tag = None;

        for item in items {
            match &item {
                // The bare path literal, which must come first.
                Meta::Path(bare) => {
                    return Err(syn::Error::new(
                        bare.span(),
                        "expected a path string literal, `operation_id = \"...\"`, or `tag = Tag`",
                    ));
                }
                Meta::NameValue(pair) => {
                    let name = pair
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    match name.as_str() {
                        "operation_id" => operation_id = Some(expect_str(&pair.value)?),
                        "tag" => tag = Some(expect_ident(&pair.value)?),
                        "path" => path = Some(expect_str(&pair.value)?),
                        _ => {
                            return Err(syn::Error::new(
                                pair.path.span(),
                                format!("unknown route argument `{name}`"),
                            ));
                        }
                    }
                }
                Meta::List(list) => {
                    return Err(syn::Error::new(
                        list.span(),
                        "expected `name = value`, not a list",
                    ));
                }
            }
        }

        let path = path.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "a route attribute needs a path, as in `#[kynos::get(\"/users/{id}\")]`",
            )
        })?;

        Ok(Self {
            path,
            operation_id,
            tag,
        })
    }
}

fn expect_str(expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Ok(value.clone()),
        other => Err(syn::Error::new(other.span(), "expected a string literal")),
    }
}

fn expect_ident(expr: &Expr) -> syn::Result<Ident> {
    match expr {
        Expr::Path(path) => path.path.get_ident().cloned().ok_or_else(|| {
            syn::Error::new(path.span(), "expected the name of a type deriving `Tag`")
        }),
        other => Err(syn::Error::new(
            other.span(),
            "expected the name of a type deriving `Tag`",
        )),
    }
}

/// Splits a doc comment into its summary and description.
///
/// The first paragraph becomes the operation's `summary` and the remainder its
/// `description`, matching how the specification distinguishes the two.
pub(crate) fn split_doc(lines: &[String]) -> (Option<String>, Option<String>) {
    let trimmed: Vec<&str> = lines.iter().map(|line| line.trim()).collect();
    let first_blank = trimmed.iter().position(|line| line.is_empty());

    match first_blank {
        None if trimmed.is_empty() => (None, None),
        None => (Some(trimmed.join(" ")), None),
        Some(index) => {
            let summary = trimmed[..index].join(" ");
            let rest = trimmed[index + 1..].join("\n");
            let rest = rest.trim().to_owned();
            (
                (!summary.is_empty()).then_some(summary),
                (!rest.is_empty()).then_some(rest),
            )
        }
    }
}

/// Collects the text of every `#[doc]` attribute on an item.
fn doc_lines(function: &ItemFn) -> Vec<String> {
    function
        .attrs
        .iter()
        .filter_map(|attribute| {
            let Meta::NameValue(pair) = &attribute.meta else {
                return None;
            };
            if !pair.path.is_ident("doc") {
                return None;
            }
            expect_str(&pair.value).ok().map(|literal| literal.value())
        })
        .collect()
}

/// Whether the item carries `#[deprecated]`, which becomes
/// `Operation.deprecated`.
fn is_deprecated(function: &ItemFn) -> bool {
    function
        .attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("deprecated"))
}

/// Expands a route attribute for a method with a dedicated Path Item field.
pub(crate) fn expand(method: &str, attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = match RouteArgs::parse(prepend_path_name(attribute.into())) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(method, &args, &function).into()
}

/// Expands `#[kynos::operation(method = "...", path = "...")]`.
pub(crate) fn expand_generic(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let tokens: TokenStream2 = attribute.into();

    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let items = match parser.parse2(tokens.clone()) {
        Ok(items) => items,
        Err(error) => return error.to_compile_error().into(),
    };

    let mut method = None;
    for entry in &items {
        if let Meta::NameValue(pair) = entry {
            if pair.path.is_ident("method") {
                match expect_str(&pair.value) {
                    Ok(value) => method = Some(value),
                    Err(error) => return error.to_compile_error().into(),
                }
            }
        }
    }

    let Some(method) = method else {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[kynos::operation]` needs `method = \"...\"`; the per-method attributes such as \
             `#[kynos::get]` are the usual way to declare an operation",
        )
        .to_compile_error()
        .into();
    };

    let method_value = method.value();
    if !STANDARD_METHODS.contains(&method_value.as_str()) && !cfg!(feature = "openapi32") {
        return syn::Error::new(
            method.span(),
            format!(
                "`{method_value}` has no Path Item field in OpenAPI 3.1, so it can only be \
                 described through `additionalOperations`; enable the `openapi32` feature"
            ),
        )
        .to_compile_error()
        .into();
    }

    let args = match RouteArgs::parse(tokens) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error().into(),
    };
    emit(&method_value, &args, &function).into()
}

/// Turns a leading bare string literal into `path = "..."`.
///
/// Lets `#[kynos::get("/users")]` and `#[kynos::operation(path = "/users")]`
/// share one argument parser.
fn prepend_path_name(tokens: TokenStream2) -> TokenStream2 {
    let mut iter = tokens.clone().into_iter().peekable();
    match iter.peek() {
        Some(proc_macro2::TokenTree::Literal(_)) => quote!(path = #tokens),
        _ => tokens,
    }
}

/// Emits the endpoint type alongside the original handler.
fn emit(method: &str, args: &RouteArgs, function: &ItemFn) -> TokenStream2 {
    let raw_path = args.path.value();

    // Reuse the document model's parser rather than reimplementing it here: two
    // notions of "valid path template" that could disagree is exactly the kind
    // of drift this framework exists to prevent.
    let variables = match kynos_openapi::PathTemplate::parse(raw_path.clone()) {
        Ok(template) => template
            .variables()
            .iter()
            .map(String::clone)
            .collect::<Vec<_>>(),
        Err(error) => {
            return syn::Error::new(args.path.span(), error.to_string()).to_compile_error();
        }
    };

    let name = &function.sig.ident;
    let visibility = &function.vis;
    let (summary, description) = split_doc(&doc_lines(function));
    let deprecated = is_deprecated(function);

    let operation_id = args
        .operation_id
        .as_ref()
        .map_or_else(|| name.to_string(), LitStr::value);

    let summary = option_str(summary.as_deref());
    let description = option_str(description.as_deref());
    let variables = variables.iter().map(String::as_str);

    // A braced struct occupies only the type namespace, so it can share a name
    // with the handler function rather than shadowing it. `routes!` refers to
    // the type; callers and unit tests keep calling the function.
    let endpoint = format_ident!("{name}");
    let tag_note = args.tag.as_ref().map(|tag| {
        quote! {
            const _: fn() = || {
                fn assert_tag<T: ::kynos::router::Tag>() {}
                assert_tag::<#tag>();
            };
        }
    });

    quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, Default)]
        #visibility struct #endpoint {}

        impl ::kynos::router::EndpointMeta for #endpoint {
            const METHOD: &'static str = #method;
            const PATH: &'static str = #raw_path;
            const PATH_VARIABLES: &'static [&'static str] = &[#(#variables),*];
            const OPERATION_ID: &'static str = #operation_id;
            const SUMMARY: ::core::option::Option<&'static str> = #summary;
            const DESCRIPTION: ::core::option::Option<&'static str> = #description;
            const DEPRECATED: bool = #deprecated;
        }

        #tag_note
    }
}

fn option_str(value: Option<&str>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |text| quote!(::core::option::Option::Some(#text)),
    )
}

/// Expands `routes![a, b, c]`.
pub(crate) fn expand_routes(input: TokenStream) -> TokenStream {
    let parser = Punctuated::<syn::Path, Token![,]>::parse_terminated;
    let paths = match parser.parse(input) {
        Ok(paths) => paths,
        Err(error) => return error.to_compile_error().into(),
    };

    if paths.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "`routes!` needs at least one operation",
        )
        .to_compile_error()
        .into();
    }

    let _ = paths;
    todo!()
}

/// Expands `path!("/users/{id}")`.
pub(crate) fn expand_path(input: TokenStream) -> TokenStream {
    let literal = parse_macro_input!(input as LitStr);

    match kynos_openapi::PathTemplate::parse(literal.value()) {
        Ok(_) => {}
        Err(error) => {
            return syn::Error::new(literal.span(), error.to_string())
                .to_compile_error()
                .into();
        }
    }

    let raw = literal.value();
    quote! {
        ::kynos::openapi::PathTemplate::parse(#raw)
            .expect("the macro validated this template at compile time")
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::split_doc;

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
    fn trailing_blank_lines_do_not_become_an_empty_description() {
        let (summary, description) = split_doc(&lines(&[" Fetch a user.", "", "  "]));
        assert_eq!(summary.as_deref(), Some("Fetch a user."));
        assert_eq!(description, None);
    }
}
