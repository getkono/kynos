//! `#[derive(HeaderParams)]`.

use kynos_openapi::model::parameter::header::is_ignored_header_parameter;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Field, Type, spanned::Spanned};

use crate::derive::{
    attribute::{field_args, group_args},
    common::{
        NameCase, named_fields, names_const, reject_duplicate_names, reject_non_token_names,
        wire_names,
    },
    params::{
        Param, check_field, construct, decode_field, parameters_body, render, response_headers_body,
    },
};

/// What to reach for instead of each header [`is_ignored_header_parameter`]
/// refuses; the OpenAPI Parameter Object says such definitions *shall be
/// ignored*. Which names are refused is the model's to say, not this table's.
const ADVICE: &[(&str, &str)] = &[
    (
        "accept",
        "content negotiation decides this; return `Negotiated<T>` and let the description say \
         which representations exist",
    ),
    (
        "content-type",
        "the content map decides this; it follows from the body type rather than being declared \
         beside it",
    ),
    (
        "authorization",
        "declare the credential instead, with `#[derive(SecurityScheme)]`, so that enforcing it \
         and describing it are one act",
    ),
];

/// The [`ADVICE`] for `name`, matched as HTTP matches field names: ASCII
/// case-insensitively.
pub(super) fn advice(name: &str) -> Option<&'static str> {
    ADVICE
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, remedy)| *remedy)
}

/// What a required header nothing carried is refused with.
const MISSING: &str = "the header is required";

/// Binds a fixed header's field: present means equal to `value`, octet for
/// octet, as its schema's `const` says, and anything else is refused. `found`
/// is the `Option<&str>` the header lookup produced.
fn decode_fixed(param: &Param<'_>, value: &str, found: &TokenStream2) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = param.name();
    let differs = format!("the value must be `{value}`");

    let refuse = |detail: &str| {
        quote! {
            return ::core::result::Result::Err(
                ::kynos::error::rejection::HeaderRejection::Invalid {
                    name: <::std::string::String as ::core::convert::From<_>>::from(#name),
                    detail: <::std::string::String as ::core::convert::From<_>>::from(#detail),
                },
            );
        }
    };
    let (differs, missing) = (refuse(&differs), refuse(MISSING));
    let absent = if param.optional().is_some() {
        quote!(::core::option::Option::None)
    } else {
        missing
    };
    let present = if param.optional().is_some() {
        quote!(::core::option::Option::Some(()))
    } else {
        quote!(())
    };

    quote! {
        let #ident: #ty = match #found {
            ::core::option::Option::Some(raw) if raw == #value => #present,
            ::core::option::Option::Some(_) => { #differs }
            ::core::option::Option::None => { #absent }
        };
    }
}

/// The `HeaderParams::encode` body.
///
/// The name is lower-cased at expansion for `HeaderName::from_static`; a value
/// that is not a valid field value refuses the whole group, since every field
/// is one the description declares. A fixed value was checked at expansion, so
/// it is written as it stands.
fn encode_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let folded = param.name().to_ascii_lowercase();
        if let Some(value) = param.fixed_value() {
            let ident = param.ident();
            let field = quote! {
                fields.push((
                    ::kynos::http::HeaderName::from_static(#folded),
                    ::kynos::http::HeaderValue::from_static(#value),
                ));
            };
            return if param.optional().is_some() {
                quote!(if ::core::option::Option::is_some(&self.#ident) { #field })
            } else {
                field
            };
        }
        let rendered = render(param);
        quote! {
            if let ::core::option::Option::Some(rendered) = #rendered {
                let name = ::kynos::http::HeaderName::from_static(#folded);
                match ::kynos::http::HeaderValue::from_str(&rendered) {
                    ::core::result::Result::Ok(value) => fields.push((name, value)),
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::kynos::extract::params::header::UnencodableHeader::new(name),
                        );
                    }
                }
            }
        }
    });

    quote! {
        let mut fields = ::std::vec::Vec::new();
        #(#entries)*
        ::core::result::Result::Ok(fields)
    }
}

/// Whether the field is `()` or `Option<()>`, the types that carry no value of
/// their own; recognised syntactically, as `Option` is.
fn carries_unit(param: &Param<'_>) -> bool {
    matches!(
        param.optional().unwrap_or(param.ty()),
        Type::Tuple(tuple) if tuple.elems.is_empty()
    )
}

/// The field's `#[header(value = "...")]`, checked: a fixed value goes on a
/// field carrying nothing else, and every such field names one.
///
/// `unit` says whether the field is `()` or `Option<()>`.
fn fixed_value(field: &Field, unit: bool) -> syn::Result<Option<String>> {
    let Some(literal) = field_args(field, "header")?.value else {
        if unit {
            return Err(syn::Error::new(
                field.ty.span(),
                "`()` carries no value of its own, so this header needs the one it always \
                 carries: `#[header(value = \"...\")]`",
            ));
        }
        return Ok(None);
    };
    if !unit {
        return Err(syn::Error::new(
            field.ty.span(),
            "a header with a fixed `value` carries nothing beside it, so its field is `()`, or \
             `Option<()>` where it may be absent",
        ));
    }

    let value = literal.value();
    if let Some(message) = non_field_value_message(&value) {
        return Err(syn::Error::new(literal.span(), message));
    }
    if let Some(bounded) = field
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("schema"))
    {
        return Err(syn::Error::new(
            bounded.span(),
            "a fixed `value` is the whole of this header's schema, so `#[schema(...)]` has \
             nothing left to bound",
        ));
    }
    Ok(Some(value))
}

/// Says why `value` cannot be a fixed header value, or `None` where it can.
///
/// RFC 9110 §5.5's `field-value`, less `obs-text`, which the decoder refuses as
/// not printable ASCII and so could never compare equal.
fn non_field_value_message(value: &str) -> Option<String> {
    if let Some(character) = value
        .chars()
        .find(|&character| !(character == ' ' || character == '\t' || character.is_ascii_graphic()))
    {
        return Some(format!(
            "the header value `{}` contains {character:?}, and a fixed value is printable \
             ASCII: a decoded header is compared as text",
            value.escape_debug()
        ));
    }
    if value.starts_with([' ', '\t']) || value.ends_with([' ', '\t']) {
        return Some(format!(
            "the header value {value:?} starts or ends with whitespace, which a recipient strips \
             (RFC 9110 §5.5), so no request could carry it"
        ));
    }
    None
}

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = named_fields(input, "Headers")?;
    // Only a written `described` overrides the trait's default.
    let described = group_args(input)?
        .described
        .map(|flag| quote!(const DESCRIBED: bool = #flag;));

    let names = wire_names(input, fields, "header")?;
    reject_non_token_names(fields, &names, "header", "an RFC 9110 field name")?;
    for (field, name) in fields.named.iter().zip(&names) {
        if is_ignored_header_parameter(name) {
            let remedy = advice(name).expect("`ADVICE` names every header the model ignores");
            return Err(syn::Error::new(
                field.span(),
                format!("`{name}` must not be declared as a header parameter: {remedy}"),
            ));
        }
    }
    reject_duplicate_names(fields, &names, "header", NameCase::Insensitive)?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let names_item = names_const(&names);

    let params = Param::pair(fields, &names)
        .into_iter()
        .zip(&fields.named)
        .map(|(param, field)| {
            let value = fixed_value(field, carries_unit(&param))?;
            Ok(param.fixed(value))
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let decode = decode_body(&params);
    let parameters = parameters_body(
        &params,
        &quote!(::kynos::openapi::ParameterIn::Header),
        false,
    );
    let response_headers = response_headers_body(&params);
    let encode = encode_body(&params);

    Ok(quote! {
        impl #impl_generics ::kynos::extract::params::header::DecodeHeaders
            for #name #ty_generics #where_clause
        {
            fn decode(
                headers: &::kynos::http::HeaderMap,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::HeaderRejection> {
                #decode
            }
        }

        impl #impl_generics ::kynos::extract::params::header::EncodeHeaders
            for #name #ty_generics #where_clause
        {
            fn encode(
                &self,
            ) -> ::core::result::Result<
                ::std::vec::Vec<(::kynos::http::HeaderName, ::kynos::http::HeaderValue)>,
                ::kynos::extract::params::header::UnencodableHeader,
            > {
                #encode
            }
        }

        impl #impl_generics ::kynos::extract::params::header::HeaderParams
            for #name #ty_generics #where_clause
        {
            #names_item
            #described

            fn parameters(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::std::vec::Vec<::kynos::openapi::Parameter> {
                #parameters
            }

            fn response_headers(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Map<::kynos::openapi::RefOr<::kynos::openapi::Header>> {
                #response_headers
            }
        }
    })
}

/// The `DecodeHeaders::decode` body, reading `headers`.
///
/// Only the first value of a repeated field is read: a field is one value.
fn decode_body(params: &[Param<'_>]) -> TokenStream2 {
    let rejection = quote!(::kynos::error::rejection::HeaderRejection);
    let reads = params.iter().map(|param| {
        let wire = param.name();
        let found = quote! {
            match headers.get(#wire) {
                ::core::option::Option::Some(value) => match value.to_str() {
                    ::core::result::Result::Ok(text) => ::core::option::Option::Some(text),
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::kynos::error::rejection::HeaderRejection::Invalid {
                                name: <::std::string::String as ::core::convert::From<_>>::from(
                                    #wire,
                                ),
                                detail: <::std::string::String as ::core::convert::From<_>>::from(
                                    "the value is not printable ASCII",
                                ),
                            },
                        );
                    }
                },
                ::core::option::Option::None => ::core::option::Option::None,
            }
        };
        if let Some(fixed) = param.fixed_value() {
            return decode_fixed(param, fixed, &found);
        }
        let decoded = decode_field(param, &rejection, &found, MISSING);
        let checked = check_field(param, &rejection);
        quote!(#decoded #checked)
    });
    let value = construct(params);

    quote! {
        #(#reads)*
        #value
    }
}
