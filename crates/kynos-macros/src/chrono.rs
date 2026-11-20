//! The `time-chrono04` deprecation, emitted where a chrono type is written.
//!
//! The match is syntactic: a path through `chrono` or `chrono04`, a
//! `NaiveDate`, `NaiveTime` or `NaiveDateTime`, or a `DateTime` with type
//! arguments, which jiff's `civil::DateTime` never takes. The marker each
//! match names is deprecated only when `kynos` has the feature, so the same
//! expansion is silent in every other build.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote_spanned;
use syn::{
    Data, DeriveInput, Fields, FnArg, GenericArgument, PathArguments, ReturnType, Signature, Type,
    TypePath, spanned::Spanned,
};

/// One marker per chrono type among a derive input's fields.
pub(crate) fn input_markers(input: &DeriveInput) -> TokenStream2 {
    match &input.data {
        Data::Struct(data) => fields_markers(&data.fields),
        Data::Enum(data) => data
            .variants
            .iter()
            .map(|variant| fields_markers(&variant.fields))
            .collect(),
        Data::Union(data) => data
            .fields
            .named
            .iter()
            .map(|field| type_markers(&field.ty))
            .collect(),
    }
}

/// One marker per chrono type among a handler's parameters and return type.
pub(crate) fn signature_markers(signature: &Signature) -> TokenStream2 {
    let inputs = signature.inputs.iter().filter_map(|input| match input {
        FnArg::Typed(typed) => Some(type_markers(&typed.ty)),
        FnArg::Receiver(_) => None,
    });
    let output = match &signature.output {
        ReturnType::Type(_, ty) => type_markers(ty),
        ReturnType::Default => TokenStream2::new(),
    };
    inputs.chain([output]).collect()
}

fn fields_markers(fields: &Fields) -> TokenStream2 {
    fields.iter().map(|field| type_markers(&field.ty)).collect()
}

/// One marker per chrono type within `ty`, each spanned at that type.
pub(crate) fn type_markers(ty: &Type) -> TokenStream2 {
    let mut found = Vec::new();
    collect(ty, &mut found);
    found
        .into_iter()
        .map(|ty| {
            quote_spanned! { ty.span() =>
                const _: () = {
                    // Not `#[allow(deprecated)]`: this use is the warning.
                    let _ = ::kynos::__private::temporal::chrono_is_unmaintained;
                };
            }
        })
        .collect()
}

fn collect<'a>(ty: &'a Type, found: &mut Vec<&'a Type>) {
    match ty {
        Type::Path(path) if is_chrono(path) => found.push(ty),
        Type::Path(path) => {
            if let Some(qself) = &path.qself {
                collect(&qself.ty, found);
            }
            for segment in &path.path.segments {
                arguments(&segment.arguments, found);
            }
        }
        Type::Reference(reference) => collect(&reference.elem, found),
        Type::Paren(paren) => collect(&paren.elem, found),
        Type::Group(group) => collect(&group.elem, found),
        Type::Array(array) => collect(&array.elem, found),
        Type::Slice(slice) => collect(&slice.elem, found),
        Type::Ptr(pointer) => collect(&pointer.elem, found),
        Type::Tuple(tuple) => tuple.elems.iter().for_each(|elem| collect(elem, found)),
        _ => {}
    }
}

fn arguments<'a>(arguments: &'a PathArguments, found: &mut Vec<&'a Type>) {
    match arguments {
        PathArguments::AngleBracketed(angle) => {
            for argument in &angle.args {
                match argument {
                    GenericArgument::Type(ty) => collect(ty, found),
                    GenericArgument::AssocType(assoc) => collect(&assoc.ty, found),
                    _ => {}
                }
            }
        }
        PathArguments::Parenthesized(parenthesized) => {
            parenthesized
                .inputs
                .iter()
                .for_each(|input| collect(&input.ty, found));
            if let ReturnType::Type(_, ty) = &parenthesized.output {
                collect(ty, found);
            }
        }
        PathArguments::None => {}
    }
}

fn is_chrono(path: &TypePath) -> bool {
    let segments = &path.path.segments;
    if segments
        .iter()
        .any(|segment| segment.ident == "chrono" || segment.ident == "chrono04")
    {
        return true;
    }
    segments.last().is_some_and(|last| {
        matches!(
            last.ident.to_string().as_str(),
            "NaiveDate" | "NaiveTime" | "NaiveDateTime"
        ) || (last.ident == "DateTime"
            && matches!(last.arguments, PathArguments::AngleBracketed(_)))
    })
}

#[cfg(test)]
mod tests;
