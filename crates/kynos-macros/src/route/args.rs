//! The arguments a route attribute accepts, and how they parse.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, ExprLit, Ident, Lit, LitStr, Meta, Token, parse::Parser, punctuated::Punctuated,
    spanned::Spanned,
};

use crate::derive::attribute::Keys;

/// The arguments a route attribute accepts.
pub(crate) struct RouteArgs {
    pub(crate) path: LitStr,
    pub(crate) operation_id: Option<LitStr>,
    pub(crate) tag: Option<Ident>,
    pub(crate) catch_panics: bool,
    pub(crate) head: Option<Head>,
}

/// The `head` argument: the `HEAD` a `GET` describes beside itself, under an
/// `operationId` derived from the handler's name or given outright.
pub(crate) enum Head {
    /// `head`, located for the diagnostic refusing it.
    Derived(proc_macro2::Span),
    /// `head = "..."`.
    Named(LitStr),
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
        let mut head = None;
        // Each argument is read once; a second `tag` has its own word.
        let mut keys =
            Keys::new("is already given, and a route attribute takes each argument once")
                .except(|key| key == "tag");

        for item in items {
            keys.read(item.path())?;

            match &item {
                // A bare flag: `catch_panics`, or `head` with a derived name.
                Meta::Path(bare) => {
                    if bare.is_ident("catch_panics") {
                        catch_panics = true;
                    } else if bare.is_ident("head") {
                        head = Some(Head::Derived(bare.span()));
                    } else {
                        return Err(syn::Error::new(
                            bare.span(),
                            "expected a path string literal, `operation_id = \"...\"`, `tag = Tag`, `catch_panics`, or `head`",
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
                        "head" => head = Some(Head::Named(expect_str(&pair.value)?)),
                        // A second tag is refused rather than silently overwriting.
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
            head,
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
