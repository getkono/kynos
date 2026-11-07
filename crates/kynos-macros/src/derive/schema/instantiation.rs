//! `name_instantiation!`: a type alias for one generic instantiation, and the
//! `NamedInstantiation` implementation publishing it under the alias's name.
//!
//! ```text
//! name_instantiation!( <entry> [; <entry>]* [;] )
//!
//! entry := <doc comment>* <visibility> <identifier> = <type>
//! ```

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Ident, Token, Type, Visibility,
    ext::IdentExt,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::doc::doc_string;

/// Every alias one invocation declares.
pub(crate) struct Instantiations(Punctuated<Entry, Token![;]>);

/// One alias: its prose, visibility, name and the instantiation it names.
struct Entry {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    ty: Type,
}

impl Parse for Instantiations {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Punctuated::parse_terminated(input).map(Self)
    }
}

impl Parse for Entry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis = input.parse()?;
        let ident = input.call(Ident::parse_any)?;
        // Read here, so `=` is not the only thing a generic alias is told.
        let generics: Option<Token![<]> = input.parse()?;
        if let Some(open) = generics {
            return Err(syn::Error::new(
                open.span(),
                "`name_instantiation!` names one instantiation, so its alias takes no generic \
                 parameters; write each instantiation as its own alias",
            ));
        }
        input.parse::<Token![=]>()?;
        let ty = input.parse()?;
        Ok(Self {
            attrs,
            vis,
            ident,
            ty,
        })
    }
}

pub(crate) fn expand_inner(input: &Instantiations) -> syn::Result<TokenStream2> {
    input.0.iter().map(expand_entry).collect()
}

fn expand_entry(entry: &Entry) -> syn::Result<TokenStream2> {
    let Entry {
        attrs,
        vis,
        ident,
        ty,
    } = entry;

    // The doc comment becomes the description; anything else would apply to
    // the alias alone, and a `cfg` would leave the implementation behind.
    if let Some(attribute) = attrs
        .iter()
        .find(|attribute| !attribute.path().is_ident("doc"))
    {
        return Err(syn::Error::new(
            attribute.span(),
            "`name_instantiation!` accepts only a doc comment on an alias, which becomes the \
             component's description",
        ));
    }

    // An identifier is a component name exactly when it is ASCII: letters,
    // digits and `_` are all `^[a-zA-Z0-9._-]+$` admits.
    let name = ident.unraw().to_string();
    if !name.is_ascii() {
        return Err(syn::Error::new(
            ident.span(),
            "a component name is ASCII letters, digits, `.`, `-` and `_`, so the alias naming \
             one must be spelled in ASCII",
        ));
    }

    let description = match doc_string(attrs) {
        Some(doc) => quote!(::core::option::Option::Some(#doc)),
        None => quote!(::core::option::Option::None),
    };

    Ok(quote! {
        #(#attrs)*
        #vis type #ident = #ty;

        impl ::kynos::schema::NamedInstantiation for #ident {
            const NAME: &'static str = #name;
            const DESCRIPTION: ::core::option::Option<&'static str> = #description;
        }
    })
}
