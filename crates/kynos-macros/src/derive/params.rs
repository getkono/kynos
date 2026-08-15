//! What the four parameter derives share.
//!
//! A path variable, a query parameter, a header and a cookie differ in *where*
//! a value is found and in almost nothing else: each is a string that becomes a
//! typed field, a typed field that becomes a string, and a schema in the
//! description. Those three acts live here, and each derive supplies only its
//! own lookup.
//!
//! # How a value becomes a field
//!
//! Through [`FromStr`](std::str::FromStr), and through it alone. A parameter
//! arrives as text whatever carried it, so the conversion a Rust program
//! already has for text is the one that applies — no serde `Deserializer` is
//! interposed, which would make the wire form of a parameter depend on
//! attributes that describe a JSON body.
//!
//! An `Option<T>` field is what makes a parameter optional, matching how an
//! `Option` field makes an object property optional. The recognition is
//! syntactic, as serde's own is: an alias for `Option<T>` reads as required,
//! and spelling the type out is the remedy.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{Field, FieldsNamed, GenericArgument, Ident, PathArguments, Type, spanned::Spanned};

use crate::derive::common::doc_string;

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
/// nothing carried it. The conversion carries the field's own span, so a type
/// with no `FromStr` is reported against the field a user wrote rather than
/// against code they never saw.
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
        let #ident: #ty = {
            let found: ::core::option::Option<&str> = #found;
            #read
        };
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
/// order.
///
/// Each field's own type supplies the schema through the registry rather than
/// through `Schema::schema`, so a named parameter type is registered once and
/// referenced rather than inlined at every operation that reads it.
///
/// `always_required` is the path location's, where the specification requires
/// it whatever the Rust type says: a template variable a request omits does not
/// match the template at all, so an `Option` there is optional in a sense no
/// description can express.
pub(crate) fn parameters_body(
    params: &[Param<'_>],
    location: &TokenStream2,
    always_required: bool,
) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let ty = param.ty();
        let name = &param.name;
        let required = always_required || param.optional().is_none();
        let described = doc_string(&param.field.attrs).map(|text| {
            quote!(parameter.description = ::core::option::Option::Some(
                ::std::string::String::from(#text)
            );)
        });

        quote! {
            parameters.push({
                let schema = registry.resolve::<#ty>();
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
/// One entry per declared name whether or not the field held a value, because
/// a path template has a slot for each and leaving one unfilled would emit the
/// brace-delimited variable itself. Percent-encoding happens where the template
/// is rendered, so the strings here are the values as they are.
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
/// The name is folded here rather than at run time: `HeaderName::from_static`
/// asks for lower case, and a field name is case-insensitive, so the fold costs
/// nothing and the construction is infallible. A *value* that could not be a
/// field value is dropped instead — writing a control character out would let
/// data end the message early, and omitting one field is the safe half of that
/// trade.
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
/// An absent optional parameter is omitted rather than written empty: `?after=`
/// and no `after` at all are different requests, and only the second means
/// "unset".
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

/// A form-encoder for one query string component.
///
/// Emitted into the body rather than called through the facade because
/// `percent-encoding` is contained to one module there and a parameter group
/// lives in the application's crate; the unreserved set is RFC 3986's, so a
/// value carrying `&`, `=` or a space survives the round trip.
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

/// The reverse: the pairs a raw query string carries, decoded.
///
/// `+` is a space, which is what `application/x-www-form-urlencoded` says and
/// what every client that builds a query string does. A malformed escape is
/// kept as the literal `%` rather than rejected: a query parameter this group
/// does not declare is none of its business, and a value it does declare fails
/// where the field is parsed, with the field's name in the diagnostic.
pub(crate) fn query_pairs() -> TokenStream2 {
    quote! {
        fn decode(raw: &str) -> ::std::string::String {
            fn digit(byte: u8) -> ::core::option::Option<u8> {
                match byte {
                    b'0'..=b'9' => ::core::option::Option::Some(byte - b'0'),
                    b'a'..=b'f' => ::core::option::Option::Some(byte - b'a' + 10),
                    b'A'..=b'F' => ::core::option::Option::Some(byte - b'A' + 10),
                    _ => ::core::option::Option::None,
                }
            }

            let bytes = raw.as_bytes();
            let mut decoded = ::std::vec::Vec::with_capacity(bytes.len());
            let mut index = 0;
            while index < bytes.len() {
                match bytes[index] {
                    b'+' => {
                        decoded.push(b' ');
                        index += 1;
                    }
                    b'%' if index + 2 < bytes.len() => {
                        match (digit(bytes[index + 1]), digit(bytes[index + 2])) {
                            (
                                ::core::option::Option::Some(high),
                                ::core::option::Option::Some(low),
                            ) => {
                                decoded.push(high * 16 + low);
                                index += 3;
                            }
                            _ => {
                                decoded.push(b'%');
                                index += 1;
                            }
                        }
                    }
                    byte => {
                        decoded.push(byte);
                        index += 1;
                    }
                }
            }
            ::std::string::String::from_utf8_lossy(&decoded).into_owned()
        }

        let mut pairs: ::std::vec::Vec<(
            ::std::string::String,
            ::std::string::String,
        )> = ::std::vec::Vec::new();
        for pair in ::core::option::Option::unwrap_or_default(query).split('&') {
            if pair.is_empty() {
                continue;
            }
            let (name, value) = match pair.split_once('=') {
                ::core::option::Option::Some(split) => split,
                ::core::option::Option::None => (pair, ""),
            };
            pairs.push((decode(name), decode(value)));
        }
    }
}
