//! `#[derive(MultipartForm)]`.
//!
//! No attribute of its own. Both directions come from one declaration: a field
//! is read from the part carrying its name and written back under the same one,
//! so the names, media types and encodings a body accepts are the ones it
//! produces.
//!
//! The names themselves come from [`schema`](super::schema), which is what
//! keeps the part a body carries and the property the description names from
//! being two rules that agree until a `rename_all` is added.
//!
//! # How a part becomes a field
//!
//! Through `FromPart` and `IntoPart`, the way a parameter travels through
//! `FromStr` and `Display`. Multiplicity is read from the field's type, exactly
//! as the parameter derives read an `Option`: a `Vec<T>` field is one part per
//! element, an `Option<T>` field is a part that need not have been sent, and
//! anything else is a part that must have been. The recognition is syntactic,
//! as serde's own is, so an alias for `Option<T>` reads as required and
//! spelling the type out is the remedy.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    DeriveInput, Field, GenericArgument, Ident, PathArguments, Type, parse_macro_input,
    spanned::Spanned,
};

use crate::derive::{
    common::{named_fields, reject_duplicate_names},
    schema::property_names,
};

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let fields = named_fields(input, "MultipartForm")?;
    let names = property_names(input, fields);
    reject_duplicate_names(fields, &names, "part")?;

    let declared: Vec<Declared<'_>> = fields
        .named
        .iter()
        .zip(&names)
        .enumerate()
        .map(|(index, (field, name))| Declared::new(index, field, name))
        .collect();

    // One binding per field, so the parts are sorted into their fields in a
    // single pass over the body rather than one scan per declared name.
    let bindings = declared.iter().map(Declared::binding);
    let dispatch = dispatch(&declared);
    let reads = declared.iter().map(Declared::read);
    let idents = declared.iter().map(Declared::ident);
    let writes = declared.iter().map(Declared::write);
    let moved = declared.iter().map(Declared::ident);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::extract::body::multipart::FromMultipart
            for #name #ty_generics #where_clause
        {
            fn from_parts(
                parts: ::std::vec::Vec<::kynos::extract::body::multipart::Part>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::BodyRejection> {
                #(#bindings)*

                // A part naming no declared field is ignored, the way an
                // undeclared query parameter is: a form may carry what the
                // agent that rendered it added, and refusing that would make
                // every such body a 422.
                for part in parts {
                    #dispatch
                }

                #(#reads)*
                ::core::result::Result::Ok(Self { #(#idents),* })
            }
        }

        impl #impl_generics ::kynos::response::codec::multipart::IntoMultipart
            for #name #ty_generics #where_clause
        {
            fn into_parts(self) -> ::std::vec::Vec<::kynos::extract::body::multipart::Part> {
                let Self { #(#moved),* } = self;
                let mut parts: ::std::vec::Vec<::kynos::extract::body::multipart::Part> =
                    ::std::vec::Vec::new();
                #(#writes)*
                parts
            }
        }
    })
}

/// Sorts one part into the field that declared its name, or drops it.
///
/// A chain rather than a `match`, because an arm of a `match` on `part.name`
/// cannot move `part`: the scrutinee's borrow outlives the arm bodies.
fn dispatch(declared: &[Declared<'_>]) -> TokenStream2 {
    declared.iter().rev().fold(quote!({}), |rest, field| {
        let gathered = field.gathered();
        let name = field.name;
        quote! {
            if part.name == #name {
                #gathered.push(part);
            } else #rest
        }
    })
}

/// How many parts one field is carried by, read from the field's type.
#[derive(Clone, Copy)]
enum Arity {
    /// Exactly one part, which the body must have carried.
    One,
    /// One part or none, which is what an `Option<T>` field declares.
    Optional,
    /// One part per element, which is what a `Vec<T>` field declares.
    Repeated,
}

/// One declared field, paired with the part name it occupies.
struct Declared<'a> {
    index: usize,
    field: &'a Field,
    name: &'a str,
    arity: Arity,
    /// The type one part becomes: the `T` of `Option<T>` or `Vec<T>`, else the
    /// field's own type.
    element: &'a Type,
}

impl<'a> Declared<'a> {
    fn new(index: usize, field: &'a Field, name: &'a str) -> Self {
        let (arity, element) = wrapper(&field.ty, "Option")
            .map(|inner| (Arity::Optional, inner))
            .or_else(|| wrapper(&field.ty, "Vec").map(|inner| (Arity::Repeated, inner)))
            .unwrap_or((Arity::One, &field.ty));

        Self {
            index,
            field,
            name,
            arity,
            element,
        }
    }

    fn ident(&self) -> &Ident {
        self.field
            .ident
            .as_ref()
            .expect("a multipart form is a struct with named fields")
    }

    /// The local the parts naming this field are gathered into.
    ///
    /// Numbered rather than named after the field, since a field may be a raw
    /// identifier and `r#type` does not concatenate into one.
    fn gathered(&self) -> Ident {
        format_ident!("__kynos_parts_{}", self.index)
    }

    fn binding(&self) -> TokenStream2 {
        let gathered = self.gathered();
        quote! {
            let mut #gathered: ::std::vec::Vec<::kynos::extract::body::multipart::Part> =
                ::std::vec::Vec::new();
        }
    }

    /// Binds this field from the parts gathered for it.
    ///
    /// The conversion carries the field's own span, so a type with no
    /// `FromPart` is reported against the field a user wrote rather than
    /// against code they never saw.
    fn read(&self) -> TokenStream2 {
        let ident = self.ident();
        let gathered = self.gathered();
        let element = self.element;
        let span = self.field.ty.span();
        let pointer = format!("/{}", self.name);

        let one = quote_spanned! {span=>
            <#element as ::kynos::extract::body::multipart::FromPart>::from_part(part)?
        };

        match self.arity {
            // Only the first part under this name is read, matching how a
            // header parameter reads only the first value: a field declared
            // once is one value, and the parts after it were never described.
            Arity::One => quote! {
                let ::core::option::Option::Some(part) =
                    ::core::iter::IntoIterator::into_iter(#gathered).next()
                else {
                    return ::core::result::Result::Err(
                        ::kynos::error::rejection::BodyRejection::Schema {
                            failures: ::std::collections::BTreeMap::from([(
                                ::std::string::String::from(#pointer),
                                ::std::string::String::from("the part is required"),
                            )]),
                        },
                    );
                };
                let #ident = #one;
            },
            Arity::Optional => quote! {
                let #ident = match ::core::iter::IntoIterator::into_iter(#gathered).next() {
                    ::core::option::Option::Some(part) => ::core::option::Option::Some(#one),
                    ::core::option::Option::None => ::core::option::Option::None,
                };
            },
            Arity::Repeated => quote! {
                let mut #ident = ::std::vec::Vec::with_capacity(#gathered.len());
                for part in #gathered {
                    #ident.push(#one);
                }
            },
        }
    }

    /// Writes this field back as the parts it was read from.
    fn write(&self) -> TokenStream2 {
        let ident = self.ident();
        let name = self.name;
        let span = self.field.ty.span();
        let element = self.element;

        let push = |value: TokenStream2| {
            quote_spanned! {span=>
                parts.push(
                    <#element as ::kynos::response::codec::multipart::IntoPart>::into_part(
                        #value, #name,
                    ),
                );
            }
        };

        match self.arity {
            Arity::One => push(quote!(#ident)),
            Arity::Optional => {
                let push = push(quote!(value));
                quote! {
                    if let ::core::option::Option::Some(value) = #ident {
                        #push
                    }
                }
            }
            Arity::Repeated => {
                let push = push(quote!(value));
                quote! {
                    for value in #ident {
                        #push
                    }
                }
            }
        }
    }
}

/// The `T` of a `Wrapper<T>` field, which is what declares its multiplicity.
fn wrapper<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }

    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }

    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    match arguments.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}
