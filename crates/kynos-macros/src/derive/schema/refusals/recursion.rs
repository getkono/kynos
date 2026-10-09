//! Types whose schema would never end.

use proc_macro2::Span;
use syn::{DeriveInput, GenericArgument, Ident, Path, PathArguments, Type};

use crate::derive::schema::{
    attributes::{is_described, is_phantom},
    described_groups,
};

/// A generic type naming itself in a member its schema describes is refused.
///
/// A generic type has no component name, so the registry inlines it, and an
/// inlined body cannot be stood in for while it is being built: the
/// description would recurse without end. A concrete type is named, and
/// `$ref`s itself. Only the direct case is visible here; generic types
/// reaching each other are refused by `Registry::resolve` when the document is
/// built.
///
/// The type is named by its bare identifier or by `Self`. A path qualified to
/// it, `crate::Node`, is left alone, since it may name another type of the
/// same identifier.
pub(super) fn reject_recursive_generic(input: &DeriveInput) -> syn::Result<()> {
    if input.generics.type_params().next().is_none() {
        return Ok(());
    }

    let site = described_groups(input)
        .into_iter()
        .flatten()
        .filter(|field| is_described(field) && !is_phantom(&field.ty))
        .find_map(|field| self_reference(&field.ty, &input.ident));
    let Some(span) = site else {
        return Ok(());
    };
    Err(syn::Error::new(
        span,
        format!(
            "`{name}` refers to itself, but a generic type has no component name and is \
             inlined, so its schema would never end. Make the recursive type concrete, or \
             implement `Schema` for it by hand returning a `name()`",
            name = input.ident,
        ),
    ))
}

/// Where `ty` names `name`: by its identifier at the head of a path, or as
/// `Self`.
///
/// A qualified path, `<Self as Tr>::Out`, is skipped whole: it is described
/// by the associated type's schema, which this derive cannot see, so any
/// cycle through it is left to `Registry::resolve`.
fn self_reference(ty: &Type, name: &Ident) -> Option<Span> {
    match ty {
        Type::Array(array) => self_reference(&array.elem, name),
        Type::Group(group) => self_reference(&group.elem, name),
        Type::Paren(paren) => self_reference(&paren.elem, name),
        Type::Ptr(pointer) => self_reference(&pointer.elem, name),
        Type::Reference(reference) => self_reference(&reference.elem, name),
        Type::Slice(slice) => self_reference(&slice.elem, name),
        Type::Tuple(tuple) => tuple
            .elems
            .iter()
            .find_map(|elem| self_reference(elem, name)),
        Type::Path(path) if path.qself.is_none() => path_reference(&path.path, name),
        _ => None,
    }
}

/// Where `path` names `name` at its head, or in any segment's type arguments.
fn path_reference(path: &Path, name: &Ident) -> Option<Span> {
    let head = path
        .segments
        .first()
        .map(|segment| &segment.ident)
        .filter(|head| *head == "Self" || (path.leading_colon.is_none() && *head == name));
    if let Some(head) = head {
        return Some(head.span());
    }
    path.segments.iter().find_map(|segment| {
        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return None;
        };
        arguments.args.iter().find_map(|argument| match argument {
            GenericArgument::Type(ty) => self_reference(ty, name),
            GenericArgument::AssocType(binding) => self_reference(&binding.ty, name),
            _ => None,
        })
    })
}
