//! The `check_constraints` body: the runtime projection of a type's
//! `#[schema(...)]` bounds.
//!
//! Each described member is checked against its own bounds and then descended
//! into, under the JSON Pointer serde reads it at: a named field under the name
//! serde reads it by, a tuple member under its position on the wire, and a
//! flattened field, a newtype's member or an internally tagged payload where
//! its parent is, since their members are the parent's own. A variant's
//! payload sits under its name when externally tagged, under the content
//! member when adjacently tagged, and beside the tag when internally tagged.
//!
//! `pattern` is described and not checked; `kynos::schema::constraints` says
//! why.

use super::{
    Container, DataEnum, DeriveInput, Field, Fields, TokenStream2, Variant,
    attributes::{Bound, bounds, field_read_name, is_described, is_phantom, is_unit_like},
    described_variants, is_flattened, positional_members, quote, transparent_member,
    variant_read_name,
};

use proc_macro2::Span;
use syn::{Data, Ident, Index, LitStr};

/// The body of `check_constraints`, which reads `self`, `at` and `violations`.
pub(super) fn body(input: &DeriveInput, container: &Container) -> TokenStream2 {
    match &input.data {
        Data::Struct(data) => struct_check(&data.fields, container),
        Data::Enum(data) => enum_check(data, container),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => TokenStream2::new(),
    }
}

/// The constraint kinds a newtype takes from its member.
///
/// A newtype is its member on the wire, so a bound a field of the newtype's
/// type declares applies to that member: `#[schema(max_length = 8)] label:
/// Label` bounds the string `Label` wraps. Each kind is implemented under a
/// bound on the member's type, written under a `for<'__kynos>` binder so that
/// a member of another kind leaves the implementation inapplicable rather than
/// failing the derive: the bound then names no parameter, which rustc would
/// otherwise refuse as trivially false.
///
/// Nothing for any other shape, which is not one value on the wire.
pub(super) fn kinds(
    input: &DeriveInput,
    container: &Container,
    generics: &syn::Generics,
) -> TokenStream2 {
    let Data::Struct(data) = &input.data else {
        return TokenStream2::new();
    };
    let member = match &data.fields {
        fields if container.transparent => transparent_member(fields),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => unnamed.unnamed.first(),
        _ => None,
    };
    let Some(member) = member.filter(|member| is_checked(member)) else {
        return TokenStream2::new();
    };

    let name = &input.ident;
    let access = member_access(&data.fields, member);
    let ty = &member.ty;
    let kinds = [
        (quote!(Numeric), quote!(number), quote!(f64)),
        (quote!(Text), quote!(text), quote!(&str)),
        (quote!(Items), quote!(item_count), quote!(usize)),
        (quote!(UniqueItems), quote!(has_unique_items), quote!(bool)),
    ];

    kinds
        .into_iter()
        .map(|(kind, method, output)| {
            let kind = quote!(::kynos::schema::constraints::#kind);
            let mut generics = generics.clone();
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(for<'__kynos> #ty: #kind));
            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
            quote! {
                #[allow(deprecated)]
                impl #impl_generics #kind for #name #ty_generics #where_clause {
                    fn #method(&self) -> ::core::option::Option<#output> {
                        <#ty as #kind>::#method(&self.#access)
                    }
                }
            }
        })
        .collect()
}

/// Where a member sits relative to its parent's pointer.
enum Step {
    /// Where its parent is.
    Here,
    /// Under a member name.
    Member(String),
    /// At an array position.
    Index(usize),
}

/// A struct's members, read through `self`.
fn struct_check(fields: &Fields, container: &Container) -> TokenStream2 {
    if container.transparent {
        return transparent_member(fields)
            .map(|field| {
                let access = member_access(fields, field);
                member(&quote!(&self.#access), field, &Step::Here)
            })
            .unwrap_or_default();
    }

    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter(|field| is_checked(field))
            .map(|field| {
                let access = member_access(fields, field);
                member(&quote!(&self.#access), field, &named_step(field, container))
            })
            .collect(),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let field = &unnamed.unnamed[0];
            if is_checked(field) {
                member(&quote!(&self.0), field, &Step::Here)
            } else {
                TokenStream2::new()
            }
        }
        Fields::Unnamed(unnamed) => positional_members(&unnamed.unnamed)
            .into_iter()
            .enumerate()
            .filter(|(_, field)| is_checked(field))
            .map(|(position, field)| {
                let access = member_access(fields, field);
                member(&quote!(&self.#access), field, &Step::Index(position))
            })
            .collect(),
        Fields::Unit => TokenStream2::new(),
    }
}

/// An enum's members, read through a `match` on `self` with one arm per
/// variant that carries any.
fn enum_check(data: &DataEnum, container: &Container) -> TokenStream2 {
    let arms: Vec<TokenStream2> = described_variants(data)
        .into_iter()
        .filter(|variant| !is_unit_like(&variant.fields))
        .map(|variant| arm(variant, container))
        .collect();

    if arms.is_empty() {
        return TokenStream2::new();
    }

    quote! {
        #[allow(unreachable_patterns)]
        match self {
            #(#arms)*
            _ => {}
        }
    }
}

/// One variant's arm: a pattern binding each checked member, and the checks.
fn arm(variant: &Variant, container: &Container) -> TokenStream2 {
    let ident = &variant.ident;
    let fields = container.fields_of(variant);

    // Where the payload sits, relative to the enum's own pointer.
    let payload = match (&container.tag, &container.content) {
        (Some(_), Some(content)) => {
            let content = LitStr::new(content, Span::call_site());
            quote!(let at = at.member(#content);)
        }
        (Some(_), None) => TokenStream2::new(),
        (None, _) => {
            let name = LitStr::new(&variant_read_name(variant, container), Span::call_site());
            quote!(let at = at.member(#name);)
        }
    };

    let (pattern, checks) = match &variant.fields {
        Fields::Named(named) => {
            let mut bindings = Vec::new();
            let mut checks = Vec::new();
            for (position, field) in named.named.iter().enumerate() {
                if !is_checked(field) {
                    continue;
                }
                let binding = binding(position);
                let name = &field.ident;
                bindings.push(quote!(#name: #binding));
                checks.push(member(
                    &quote!(#binding),
                    field,
                    &named_step(field, &fields),
                ));
            }
            (quote!(Self::#ident { #(#bindings,)* .. }), checks)
        }
        Fields::Unnamed(unnamed) => {
            let newtype = unnamed.unnamed.len() == 1;
            let wire: Vec<&Field> = positional_members(&unnamed.unnamed);
            let mut bindings = Vec::new();
            let mut checks = Vec::new();
            for (position, field) in unnamed.unnamed.iter().enumerate() {
                let on_wire = wire.iter().position(|member| std::ptr::eq(*member, field));
                match on_wire.filter(|_| is_checked(field)) {
                    Some(index) => {
                        let binding = binding(position);
                        let step = if newtype {
                            Step::Here
                        } else {
                            Step::Index(index)
                        };
                        checks.push(member(&quote!(#binding), field, &step));
                        bindings.push(quote!(#binding));
                    }
                    None => bindings.push(quote!(_)),
                }
            }
            (quote!(Self::#ident(#(#bindings),*)), checks)
        }
        Fields::Unit => return TokenStream2::new(),
    };

    if checks.is_empty() {
        return TokenStream2::new();
    }

    quote! {
        #pattern => {
            #payload
            #(#checks)*
        }
    }
}

/// Whether a member is on the wire serde reads and has a schema to check.
///
/// A `PhantomData` is `null` on the wire and has no `Schema`, so there is
/// nothing to descend into.
fn is_checked(field: &Field) -> bool {
    is_described(field) && !is_phantom(&field.ty)
}

/// Where a named field sits: under the name serde reads it by, or where its
/// parent is when flattened.
fn named_step(field: &Field, container: &Container) -> Step {
    if is_flattened(field) {
        Step::Here
    } else {
        Step::Member(field_read_name(field, container))
    }
}

/// The binding a variant's member is matched into, by its declared position,
/// so it cannot collide with `at`, `violations` or another member.
fn binding(position: usize) -> Ident {
    Ident::new(&format!("__kynos_member_{position}"), Span::call_site())
}

/// How `self` reaches a struct field: its name, or its declared index.
fn member_access(fields: &Fields, field: &Field) -> TokenStream2 {
    if let Some(name) = &field.ident {
        return quote!(#name);
    }
    let position = fields
        .iter()
        .position(|candidate| std::ptr::eq(candidate, field))
        .unwrap_or_default();
    let index = Index::from(position);
    quote!(#index)
}

/// One member's checks: each of its own bounds, then its value's.
fn member(value: &TokenStream2, field: &Field, step: &Step) -> TokenStream2 {
    let ty = &field.ty;
    let locate = match step {
        Step::Here => TokenStream2::new(),
        Step::Member(name) => {
            let name = LitStr::new(name, Span::call_site());
            quote!(let at = at.member(#name);)
        }
        Step::Index(index) => quote!(let at = at.index(#index);),
    };

    let keywords = bounds(field)
        .into_iter()
        .filter(|bound| bound.key != "pattern")
        .map(|Bound { key, value: bound }| {
            let bound = bound.map(|bound| quote!(#bound,));
            quote! {
                ::kynos::__private::constraints::#key::<#ty>(value, #bound at, violations);
            }
        });

    quote! {
        {
            let value: &#ty = #value;
            #locate
            #(#keywords)*
            ::kynos::schema::Schema::check_constraints(value, at, violations);
        }
    }
}
