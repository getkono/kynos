//! `#[derive(QueryParams)]`.

pub(crate) mod flatten;

use quote::{quote, quote_spanned};
use syn::{DeriveInput, Expr, Ident, LitBool, spanned::Spanned};

use crate::derive::{
    common::{named_fields, wrapped},
    params::{
        List, Param, check_field, constraint_problems_body, decode_field, decode_list,
        describe::{collected, parameter_entry},
        group_problem, render,
    },
    query_params::flatten::{Flattened, Member},
};

/// How the field carries its items, where it is a `Vec<T>`: exploded unless
/// its `#[param(explode = false)]` says otherwise.
///
/// Refuses an `Option<Vec<T>>`, whose `Some` of an empty list no query string
/// carries, and `explode` on a field that is one value, where OpenAPI says it
/// has no effect and so would say nothing.
fn list(param: &Param<'_>, explode: Option<LitBool>) -> syn::Result<Option<List>> {
    if param
        .optional()
        .is_some_and(|inner| wrapped(inner, "Vec").is_some())
    {
        return Err(syn::Error::new(
            param.ty().span(),
            "a list parameter is absent exactly when it has no items, so an `Option` around it \
             holds a `Some(vec![])` no query string carries. Use the `Vec` alone, which is \
             empty when no pair names it",
        ));
    }
    if wrapped(param.ty(), "Vec").is_none() {
        return match explode {
            Some(flag) => Err(syn::Error::new(
                flag.span(),
                "`explode` says how a list's items are carried, and this field is one value. \
                 Only a `Vec<T>` field takes it",
            )),
            None => Ok(None),
        };
    }
    Ok(Some(match explode {
        Some(flag) if !flag.value => List::Delimited,
        _ => List::Exploded,
    }))
}

/// What the field binds when no pair names it, where its
/// `#[param(default = ...)]` says.
///
/// Refuses a default on an `Option<T>`, which would never be `None` and so
/// reads as optional while it is not, and on a list, whose absence is its
/// emptiness: a default would leave the empty list no query string could ask
/// for.
fn default(param: &Param<'_>, default: Option<Expr>) -> syn::Result<Option<Expr>> {
    let Some(value) = default else {
        return Ok(None);
    };
    if param.optional().is_some() {
        return Err(syn::Error::new(
            param.ty().span(),
            "a defaulted parameter is never absent once decoded, so an `Option` around it would \
             never be `None`. Use the inner type, which binds the default when no pair names it",
        ));
    }
    if wrapped(param.ty(), "Vec").is_some() {
        return Err(syn::Error::new(
            value.span(),
            "a list parameter is absent exactly when it has no items, so a default would leave \
             no query string able to send the empty list. Drop the `default`: no pair already \
             binds the empty list",
        ));
    }
    Ok(Some(value))
}

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "QueryParams")?;
    // The attribute the `Schema` derive reads too, so a group deriving both
    // answers as `QueryString` reads it.
    let group = group_problem(input)?;
    let (params, members) = flatten::members(input, fields, group.as_ref())?;
    let flattened: Vec<&Flattened<'_>> = members
        .iter()
        .filter_map(|member| match member {
            Member::Flattened(flattened) => Some(flattened),
            Member::Own(_) => None,
        })
        .collect();

    let name = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let [described, decoded, encoded] = ["QueryParams", "DecodeQuery", "EncodeQuery"]
        .map(|trait_name| flatten::bounded(&input.generics, &flattened, trait_name));

    // No `Schema` is emitted: beside a `Schema` derive it would conflict, and
    // a group is described by its parameters, never as a body schema.
    let rejection = quote!(::kynos::error::rejection::QueryRejection);
    let location = quote!(::kynos::openapi::ParameterIn::Query);

    let pairs = query_slots(&params, &rejection);
    let keep_whole = (!flattened.is_empty()).then(flatten::keep_whole);
    let mut reads = Vec::with_capacity(members.len());
    let mut entries = Vec::with_capacity(members.len());
    let mut writes = Vec::with_capacity(members.len());
    let mut idents = Vec::with_capacity(members.len());
    for member in &members {
        let (read, entry, encode, ident) = match member {
            Member::Own(index) => {
                let param = &params[*index];
                (
                    read(param, &query_slot(*index), &rejection),
                    parameter_entry(param, &location, false),
                    encode_entry(param),
                    param.ident(),
                )
            }
            Member::Flattened(flattened) => (
                flattened.decode(),
                flattened.parameters(),
                flattened.encode(),
                flattened.ident(),
            ),
        };
        reads.push(read);
        writes.push(encode);
        idents.push(ident);
        entries.push(entry);
    }

    let own_problems = constraint_problems_body(&params);
    let (parameters, constraint_problems) = if flattened.is_empty() {
        (collected(entries.into_iter(), &quote!()), own_problems)
    } else {
        flatten::composed(entries, &own_problems, &flattened)
    };

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::query::QueryParams
            for #name #ty_generics #described
        {
            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }

            fn constraint_problems(
            ) -> ::std::vec::Vec<::kynos::schema::constraints::ConstraintProblem> {
                #constraint_problems
            }
        }

        impl #impl_generics ::kynos::extract::params::query::DecodeQuery
            for #name #ty_generics #decoded
        {
            fn decode(
                query: ::core::option::Option<&str>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::QueryRejection> {
                #keep_whole
                #pairs
                #(#reads)*
                ::core::result::Result::Ok(Self { #(#idents),* })
            }
        }

        impl #impl_generics ::kynos::extract::params::query::EncodeQuery
            for #name #ty_generics #encoded
        {
            fn encode(&self) -> ::std::string::String {
                let mut query = ::std::string::String::new();
                #(#writes)*
                query
            }
        }
    })
}

/// Binds one field from what its [`query_slot`] holds once [`query_slots`]
/// has run, then checks its bounds.
fn read(
    param: &Param<'_>,
    slot: &Ident,
    rejection: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let wire = param.name();
    let checked = check_field(param, rejection);
    let decoded = match param.list() {
        Some(List::Exploded) => decode_list(param, rejection, &quote!(#slot)),
        Some(List::Delimited) => {
            // Split before decoding, so an escaped `%2C` stays inside its
            // item (OpenAPI 3.1.1 and 3.2.0, "Delimiters in Parameter Values").
            let items = quote! {
                ::core::iter::Iterator::map(
                    ::core::iter::Iterator::flat_map(
                        ::core::iter::IntoIterator::into_iter(#slot),
                        |raw: &str| raw.split(','),
                    ),
                    ::kynos::__private::uri::decode_query_component,
                )
            };
            decode_list(param, rejection, &items)
        }
        None => {
            // Only a declared parameter's value is required to be UTF-8.
            let found = quote! {
                match &#slot {
                    ::core::option::Option::Some(octets) => match ::core::str::from_utf8(octets) {
                        ::core::result::Result::Ok(text) => ::core::option::Option::Some(text),
                        ::core::result::Result::Err(_) => {
                            return ::core::result::Result::Err(
                                #rejection::Invalid {
                                    name: <::std::string::String as ::core::convert::From<_>>::from(
                                        #wire,
                                    ),
                                    detail: <::std::string::String as ::core::convert::From<_>>::from(
                                        "the percent-decoded value is not UTF-8",
                                    ),
                                },
                            );
                        }
                    },
                    ::core::option::Option::None => ::core::option::Option::None,
                }
            };
            decode_field(param, rejection, &found, "the parameter is required")
        }
    };
    quote!(#decoded #checked)
}

/// One pass over the pairs a raw query string carries, binding each declared
/// parameter's [`query_slot`] to what the pairs naming it carry: the octets of
/// the one pair, every exploded list pair's octets in wire order, or a
/// delimited list's one value still escaped, for its items to be split before
/// they are decoded.
///
/// A second pair naming a parameter that is one pair is refused rather than
/// either value kept: the parameter carries one value, and which one a client,
/// a proxy or another framework would read is not this decoder's to guess.
///
/// A name is compared as octets once decoded, and only a declared parameter's
/// value is decoded at all, so a pair no field reads costs its name's decoding
/// alone and nothing is collected. Split by `kynos::__private::uri::query_names`,
/// which shares its decoding with query API keys.
fn query_slots(
    params: &[Param<'_>],
    rejection: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if params.is_empty() {
        return quote!(let _ = query;);
    }

    let mut slots = Vec::with_capacity(params.len());
    let mut arms = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let slot = query_slot(index);
        let name = param.name();
        let wire = proc_macro2::Literal::byte_string(name.as_bytes());
        let once = quote! {
            if #slot.is_some() {
                return ::core::result::Result::Err(#rejection::Invalid {
                    name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                    detail: <::std::string::String as ::core::convert::From<_>>::from(
                        "the parameter is given more than once, and it carries one value",
                    ),
                });
            }
        };
        let (declared, arm) = match param.list() {
            Some(List::Exploded) => (
                quote! {
                    let mut #slot: ::std::vec::Vec<::std::borrow::Cow<'_, [u8]>> =
                        ::std::vec::Vec::new();
                },
                quote! {
                    #slot.push(::kynos::__private::uri::decode_query_component(value));
                },
            ),
            Some(List::Delimited) => (
                quote! {
                    let mut #slot: ::core::option::Option<&str> = ::core::option::Option::None;
                },
                quote! {
                    #once
                    #slot = ::core::option::Option::Some(value);
                },
            ),
            None => (
                quote! {
                    let mut #slot: ::core::option::Option<::std::borrow::Cow<'_, [u8]>> =
                        ::core::option::Option::None;
                },
                quote! {
                    #once
                    #slot = ::core::option::Option::Some(
                        ::kynos::__private::uri::decode_query_component(value),
                    );
                },
            ),
        };
        slots.push(declared);
        arms.push(quote!(#wire => { #arm }));
    }

    quote! {
        #(#slots)*
        for (name, value) in ::kynos::__private::uri::query_names(query) {
            match &*name {
                #(#arms)*
                _ => {}
            }
        }
    }
}

/// The local [`query_slots`] binds the `index`th declared parameter's octets
/// to.
fn query_slot(index: usize) -> Ident {
    quote::format_ident!("__kynos_query_{index}")
}

/// What the `EncodeQuery::encode` body appends to `query` for one field.
///
/// An absent optional parameter is omitted rather than written empty, and so
/// is an empty list, which RFC 6570 treats as undefined. Each name and item is
/// encoded by `kynos::__private::uri::encode_query_component`, which escapes a
/// `,`, so it round-trips through the decoder [`query_slots`] writes; a
/// delimited list's items are joined by a literal one.
fn encode_entry(param: &Param<'_>) -> proc_macro2::TokenStream {
    let name = param.name();
    let ident = param.ident();
    // Only the path is spanned to the field, so an item that does not
    // display is reported there.
    let to_string = quote_spanned!(param.ty().span()=> ::std::string::ToString::to_string);
    let pair = quote! {
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str(&::kynos::__private::uri::encode_query_component(#name));
        query.push('=');
    };
    match param.list() {
        Some(List::Exploded) => {
            return quote! {
                for item in &self.#ident {
                    #pair
                    query.push_str(&::kynos::__private::uri::encode_query_component(
                        &#to_string(item),
                    ));
                }
            };
        }
        Some(List::Delimited) => {
            return quote! {
                for (index, item) in ::core::iter::Iterator::enumerate(self.#ident.iter()) {
                    if index == 0 {
                        #pair
                    } else {
                        query.push(',');
                    }
                    query.push_str(&::kynos::__private::uri::encode_query_component(
                        &#to_string(item),
                    ));
                }
            };
        }
        None => {}
    }
    let rendered = render(param);
    quote! {
        if let ::core::option::Option::Some(value) = #rendered {
            #pair
            query.push_str(&::kynos::__private::uri::encode_query_component(&value));
        }
    }
}
