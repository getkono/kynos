//! A failure's type URI: the `&'static str` expression the expansion writes
//! wherever the URI is read, and what the derive publishes of it.
//!
//! A struct naming one is a `ProblemType`, so an interceptor's refusal can
//! name the type the application's own error does. A struct naming none
//! sends `about:blank`, which RFC 9457 section 4.2.1 says carries no meaning
//! beyond the status, so it is no `ProblemType`: `None` there leaves a
//! refusal to the problem policy, which never rewrites an `ApiError`'s, and
//! `Some("about:blank")` would declare a refusal the policy then renames.
//!
//! An enum's variants are no types, so each publishes its URI as a hidden
//! `const` among the enum's handles, which `type_uri` reads per value and a
//! subset or a `variant_of` variant reads through the wider enum. A variant
//! handing over the error it holds publishes `None`: that error's URI may be
//! chosen at run time, as an `AuthRejection`'s is.

use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{Data, DeriveInput, ExprPath, Ident, parse_quote, spanned::Spanned};

use super::{Failure, Transparent};

/// The URI a declared failure's problem carries, as a `&'static str`
/// expression: its own, or `about:blank`.
pub(super) fn carried(uri: Option<&TypeUri>) -> TokenStream2 {
    uri.map_or_else(|| quote!("about:blank"), |uri| quote!(#uri))
}

/// The hidden `const` holding the type URI `variant` answers with, spanned
/// to the variant, as its statuses' is.
pub(super) fn type_uri_of(variant: &Ident) -> Ident {
    format_ident!(
        "__KYNOS_PROBLEM_TYPE_URI_{}",
        variant,
        span = variant.span()
    )
}

/// A declared failure's entry for [`type_uri_of`]: the URI it carries.
pub(super) fn declared(failure: &Failure) -> TokenStream2 {
    let uri = carried(failure.type_uri.as_ref());
    quote!(::core::option::Option::Some(#uri))
}

impl Transparent {
    /// Its entry for [`type_uri_of`]: the wider variant's, or `None` for a
    /// field, whose URI is the held error's to say.
    pub(super) fn type_uri(&self) -> TokenStream2 {
        match &self.of {
            None => quote!(::core::option::Option::None),
            Some(of) => {
                let ty = &self.ty;
                let item = type_uri_of(of);
                quote!(<#ty>::#item)
            }
        }
    }
}

/// The enum's `type_uri`, reading each variant's [`type_uri_of`].
pub(super) fn method<'a>(variants: impl Iterator<Item = &'a Ident>) -> TokenStream2 {
    let arms = variants.map(|variant| {
        let item = type_uri_of(variant);
        quote!(Self::#variant { .. } => Self::#item,)
    });
    quote! {
        /// The RFC 9457 `type` this value's problem carries, `about:blank`
        /// where its failure names none, or `None` where it answers as the
        /// error it holds, whose own problem names it.
        #[must_use]
        pub const fn type_uri(&self) -> ::core::option::Option<&'static str> {
            match *self {
                #(#arms)*
            }
        }
    }
}

/// A struct's `ProblemType`, where it names a type URI; nothing otherwise.
pub(super) fn problem_type(input: &DeriveInput, failures: &[&Failure]) -> TokenStream2 {
    let (Data::Struct(_), [failure]) = (&input.data, failures) else {
        return TokenStream2::new();
    };
    let Some(uri) = &failure.type_uri else {
        return TokenStream2::new();
    };

    let name = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    // `ProblemType` is `'static`, which a generic struct is only where its
    // parameters are.
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#name #ty_generics: 'static));
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    quote! {
        #[automatically_derived]
        impl #impl_generics ::kynos::error::problem::ProblemType
            for #name #ty_generics #where_clause
        {
            const TYPE_URI: ::core::option::Option<&'static str> =
                ::core::option::Option::Some(#uri);
        }
    }
}

/// A failure's type URI, as the `&'static str` expression the expansion
/// writes.
pub(super) enum TypeUri {
    /// Known here, so emitted as a literal.
    Literal(String),

    /// A `const` prefix and a slug, joined at compile time by `const` items
    /// the expansion declares, so the URI stays a borrowed `&'static str`.
    Joined { base: ExprPath, slug: String },
}

impl ToTokens for TypeUri {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            Self::Literal(uri) => uri.to_tokens(tokens),
            Self::Joined { base, slug } => {
                // Spanned to the path, so a `const` that is not a `&str`
                // is reported where `base` names it. The names are prefixed
                // because these items shadow the module's: a user `const
                // BASE` would otherwise be defined as itself.
                let prefix =
                    quote_spanned!(base.span()=> const __KYNOS_PROBLEM_BASE: &str = #base;);
                tokens.extend(quote! {{
                    #prefix
                    const __KYNOS_PROBLEM_LEN: usize = __KYNOS_PROBLEM_BASE.len() + #slug.len();
                    const __KYNOS_PROBLEM_BYTES: &[u8] = &::kynos::__private::problem::join::<
                        __KYNOS_PROBLEM_LEN,
                    >(__KYNOS_PROBLEM_BASE, #slug);
                    // A `const`, so the UTF-8 check runs once, at compile time.
                    const __KYNOS_PROBLEM_URI: &str =
                        ::kynos::__private::problem::utf8(__KYNOS_PROBLEM_BYTES);
                    __KYNOS_PROBLEM_URI
                }});
            }
        }
    }
}
