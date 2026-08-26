//! The arguments a route attribute accepts, and how they parse.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, ExprLit, Ident, Lit, LitStr, Meta, Token, parse::Parser, punctuated::Punctuated,
    spanned::Spanned,
};

/// The arguments a route attribute accepts.
pub(crate) struct RouteArgs {
    pub(crate) path: LitStr,
    pub(crate) operation_id: Option<LitStr>,
    pub(crate) tag: Option<Ident>,
    pub(crate) catch_panics: bool,
}

impl RouteArgs {
    /// Parses `("/users/{id}", operation_id = "getUser", tag = Users)`.
    pub(crate) fn parse(tokens: TokenStream2) -> syn::Result<Self> {
        let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
        let items = parser.parse2(tokens)?;

        let mut path = None;
        let mut operation_id = None;
        let mut tag = None;
        let mut catch_panics = false;

        for item in items {
            match &item {
                // The bare path literal, which must come first.
                Meta::Path(bare) => {
                    if bare.is_ident("catch_panics") {
                        catch_panics = true;
                    } else {
                        return Err(syn::Error::new(
                            bare.span(),
                            "expected a path string literal, `operation_id = \"...\"`, `tag = Tag`, or `catch_panics`",
                        ));
                    }
                }
                Meta::NameValue(pair) => {
                    let name = pair
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    match name.as_str() {
                        "operation_id" => operation_id = Some(expect_str(&pair.value)?),
                        // Overwriting would discard the first silently, which is
                        // the same defect as never reading it at all.
                        "tag" if tag.is_some() => {
                            return Err(syn::Error::new(
                                pair.span(),
                                "this route already names a tag, and it can name one. Apply the \
                                 others with `Router::tag`, `Group::tag` or `EndpointBuilder::tag`",
                            ));
                        }
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
            catch_panics,
        })
    }
}

pub(crate) fn expect_str(expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Ok(value.clone()),
        other => Err(syn::Error::new(other.span(), "expected a string literal")),
    }
}

pub(crate) fn expect_ident(expr: &Expr) -> syn::Result<Ident> {
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

/// Turns a leading bare string literal into `path = "..."`.
///
/// Lets `#[kynos::get("/users")]` and `#[kynos::operation(path = "/users")]`
/// share one argument parser.
pub(crate) fn prepend_path_name(tokens: TokenStream2) -> TokenStream2 {
    let mut iter = tokens.clone().into_iter().peekable();
    match iter.peek() {
        Some(proc_macro2::TokenTree::Literal(_)) => quote!(path = #tokens),
        _ => tokens,
    }
}
