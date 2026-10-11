//! What the four parameter derives share: decoding, encoding and describing a
//! field; each derive supplies only its own lookup.
//!
//! A value becomes a field through [`FromStr`](std::str::FromStr) alone, never
//! serde. Each field's type is bounded by `kynos::schema::ParamValue`, so its
//! schema describes the one value `FromStr` reads. An `Option<T>` field is
//! optional; the recognition is syntactic, so an alias for it reads as required.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Field, FieldsNamed, GenericArgument, Ident, PathArguments, Type, spanned::Spanned};

use crate::derive::{
    common::doc_string,
    schema::{attributes::constraints, check::value_checks},
};

/// One field of a parameter group, paired with the wire name it occupies.
pub(crate) struct Param<'a> {
    field: &'a Field,
    name: String,
}

impl<'a> Param<'a> {
    /// Pairs each field with the wire name already resolved for it.
    pub(crate) fn pair(fields: &'a FieldsNamed, names: &[String]) -> Vec<Self> {
        fields
            .named
            .iter()
            .zip(names)
            .map(|(field, name)| Self {
                field,
                name: name.clone(),
            })
            .collect()
    }

    /// The name this parameter is carried under.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The identifier the struct literal binds.
    fn ident(&self) -> &Ident {
        self.field
            .ident
            .as_ref()
            .expect("a parameter group is a struct with named fields")
    }

    fn ty(&self) -> &Type {
        &self.field.ty
    }

    /// The `T` of an `Option<T>` field, which is what makes it optional.
    fn optional(&self) -> Option<&Type> {
        let Type::Path(path) = self.ty() else {
            return None;
        };
        if path.qself.is_some() {
            return None;
        }

        let segment = path.path.segments.last()?;
        if segment.ident != "Option" {
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
}

/// Binds one field from an already-located value, or returns a rejection.
///
/// `found` is an expression of type `Option<&str>`, `rejection` the type the
/// derive's `decode` returns, and `missing` what a required parameter says when
/// nothing carried it. The `ParamValue` assertion is spanned at the field and
/// lives here because every derive's `decode` calls this.
pub(crate) fn decode_field(
    param: &Param<'_>,
    rejection: &TokenStream2,
    found: &TokenStream2,
    missing: &str,
) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = &param.name;
    let span = param.field.ty.span();

    let parse = |inner: &Type| {
        quote_spanned! {span=>
            match <#inner as ::core::str::FromStr>::from_str(raw) {
                ::core::result::Result::Ok(value) => value,
                ::core::result::Result::Err(error) => {
                    return ::core::result::Result::Err(#rejection::Invalid {
                        name: ::std::string::String::from(#name),
                        detail: ::std::string::ToString::to_string(&error),
                    });
                }
            }
        }
    };

    // An `Option<T>` field bounds its `T`.
    let carried = param.optional().unwrap_or(ty);
    let bounded = quote_spanned! {carried.span()=>
        {
            fn carried_by_one_parameter<T: ::kynos::schema::ParamValue>() {}
            carried_by_one_parameter::<#carried>();
        }
    };

    let read = if let Some(inner) = param.optional() {
        let parsed = parse(inner);
        quote! {
            match found {
                ::core::option::Option::Some(raw) => ::core::option::Option::Some(#parsed),
                ::core::option::Option::None => ::core::option::Option::None,
            }
        }
    } else {
        let parsed = parse(ty);
        quote! {
            match found {
                ::core::option::Option::Some(raw) => #parsed,
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(#rejection::Invalid {
                        name: ::std::string::String::from(#name),
                        detail: ::std::string::String::from(#missing),
                    });
                }
            }
        }
    };

    quote! {
        #bounded
        let #ident: #ty = {
            let found: ::core::option::Option<&str> = #found;
            #read
        };
    }
}

/// Refuses a decoded field that breaks one of its `#[schema(...)]` bounds, or
/// one its type declares, as `Schema`, naming the parameter, each failure at a
/// pointer into its value. Runs after [`decode_field`] has bound the field.
pub(crate) fn check_field(param: &Param<'_>, rejection: &TokenStream2) -> TokenStream2 {
    let ident = param.ident();
    let ty = param.ty();
    let name = &param.name;
    let checks = value_checks(param.field);

    quote! {
        {
            let value: &#ty = &#ident;
            let at = ::kynos::schema::constraints::Pointer::root();
            let mut __kynos_violations = ::kynos::schema::constraints::Violations::new();
            {
                let violations = &mut __kynos_violations;
                #checks
            }
            if !__kynos_violations.is_empty() {
                return ::core::result::Result::Err(#rejection::Schema {
                    name: ::std::string::String::from(#name),
                    failures: __kynos_violations.into_failures(),
                });
            }
        }
    }
}

/// The struct literal a `decode` body ends with.
pub(crate) fn construct(params: &[Param<'_>]) -> TokenStream2 {
    let idents = params.iter().map(Param::ident);
    quote!(::core::result::Result::Ok(Self { #(#idents),* }))
}

/// This field's value as an `Option<String>`, absent only when the field is.
fn render(param: &Param<'_>) -> TokenStream2 {
    let ident = param.ident();
    let span = param.field.ty.span();

    if param.optional().is_some() {
        quote_spanned! {span=>
            ::core::option::Option::map(
                ::core::option::Option::as_ref(&self.#ident),
                ::std::string::ToString::to_string,
            )
        }
    } else {
        quote_spanned! {span=>
            ::core::option::Option::Some(::std::string::ToString::to_string(&self.#ident))
        }
    }
}

/// The `parameters` body: one OpenAPI parameter per field, in declaration
/// order, each schema resolved through the registry.
///
/// `always_required` is the path location's, which the OpenAPI Parameter Object
/// requires to be `required: true` whatever the Rust type says. `bounded` puts
/// each field's `#[schema(...)]` bounds on its schema, as the `Schema` derive
/// puts them on a property, for a derive whose decoder runs [`check_field`].
pub(crate) fn parameters_body(
    params: &[Param<'_>],
    location: &TokenStream2,
    always_required: bool,
    bounded: bool,
) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let ty = param.ty();
        let name = &param.name;
        let required = always_required || param.optional().is_none();
        let constrained = bounded
            .then(|| constraints(param.field))
            .flatten()
            .map(|constraints| quote!(let schema = #constraints.apply(schema);));
        let described = doc_string(&param.field.attrs).map(|text| {
            quote!(parameter.description = ::core::option::Option::Some(
                ::std::string::String::from(#text)
            );)
        });

        quote! {
            parameters.push({
                let schema = registry.resolve::<#ty>();
                #constrained
                let mut parameter =
                    ::kynos::openapi::Parameter::new(#name, #location, schema);
                parameter.required = ::core::option::Option::Some(#required);
                #described
                parameter
            });
        }
    });

    quote! {
        let mut parameters = ::std::vec::Vec::new();
        #(#entries)*
        parameters
    }
}

/// The `response_headers` body: the same fields, as a `headers` map.
pub(crate) fn response_headers_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let ty = param.ty();
        let name = &param.name;
        let required = param.optional().is_none();
        let described = doc_string(&param.field.attrs).map(|text| {
            quote!(header.description = ::core::option::Option::Some(
                ::std::string::String::from(#text)
            );)
        });

        quote! {
            headers.insert(::std::string::String::from(#name), {
                let schema = registry.resolve::<#ty>();
                let mut header = ::kynos::openapi::Header::new(schema);
                header.required = ::core::option::Option::Some(#required);
                #described
                ::kynos::openapi::RefOr::Item(header)
            });
        }
    });

    quote! {
        let mut headers = ::kynos::openapi::Map::new();
        #(#entries)*
        headers
    }
}

/// The `PathParams::encode` body.
///
/// One entry per declared name, so every template slot is filled; values are
/// percent-encoded where the template is rendered.
pub(crate) fn path_encode_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let name = &param.name;
        let rendered = render(param);
        quote! {
            (#name, ::core::option::Option::unwrap_or_default(#rendered))
        }
    });

    quote!(::std::vec![#(#entries),*])
}

/// The `HeaderParams::encode` body.
///
/// The name is lower-cased at expansion for `HeaderName::from_static`; a value
/// that is not a valid field value is dropped rather than written.
pub(crate) fn header_encode_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let folded = param.name.to_ascii_lowercase();
        let rendered = render(param);
        quote! {
            if let ::core::option::Option::Some(rendered) = #rendered {
                if let ::core::result::Result::Ok(value) =
                    ::kynos::http::HeaderValue::from_str(&rendered)
                {
                    fields.push((::kynos::http::HeaderName::from_static(#folded), value));
                }
            }
        }
    });

    quote! {
        let mut fields = ::std::vec::Vec::new();
        #(#entries)*
        fields
    }
}

/// The `QueryParams::encode` body.
///
/// An absent optional parameter is omitted rather than written empty.
pub(crate) fn query_encode_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let name = &param.name;
        let rendered = render(param);
        quote! {
            if let ::core::option::Option::Some(value) = #rendered {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&encode(#name));
                query.push('=');
                query.push_str(&encode(&value));
            }
        }
    });

    let encoder = query_encoder();
    quote! {
        #encoder
        let mut query = ::std::string::String::new();
        #(#entries)*
        query
    }
}

/// A form-encoder for one query string component, escaping everything outside
/// RFC 3986's unreserved set so a value round-trips through [`query_pairs`].
fn query_encoder() -> TokenStream2 {
    quote! {
        fn encode(raw: &str) -> ::std::string::String {
            let mut encoded = ::std::string::String::with_capacity(raw.len());
            for byte in raw.as_bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                        encoded.push(::core::primitive::char::from(*byte));
                    }
                    other => {
                        encoded.push('%');
                        for digit in [other >> 4, other & 0x0f] {
                            encoded.push(::core::primitive::char::from(match digit {
                                0..=9 => b'0' + digit,
                                _ => b'A' + digit - 10,
                            }));
                        }
                    }
                }
            }
            encoded
        }
    }
}

/// The reverse: the pairs a raw query string carries, each half decoded to
/// octets, so a name is compared as octets.
///
/// Decoded by `kynos::__private::uri::query_pairs`, shared with query API keys.
pub(crate) fn query_pairs() -> TokenStream2 {
    quote! {
        let pairs: ::std::vec::Vec<(
            ::std::borrow::Cow<'_, [u8]>,
            ::std::borrow::Cow<'_, [u8]>,
        )> = ::kynos::__private::uri::query_pairs(query).collect();
    }
}
