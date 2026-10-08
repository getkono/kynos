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
//! A member serde reads under an `alias` as well is reported at the object
//! holding it instead, since which name the document used is gone once it is
//! read. A member serde fills from a `default` when the document leaves it
//! out is reported only where the value it would be filled with meets its
//! bounds: where that value breaks them, a value breaking them may be one the
//! document never sent, which the emitted schema, not listing the member in
//! `required`, admits.
//!
//! `pattern` is described and not checked; `kynos::schema::constraints` says
//! why.

use super::{
    Container, DataEnum, DeriveInput, Field, Fields, TokenStream2, Variant,
    aliases::{read_names, variants_read_names},
    attributes::{
        Bound, DefaultFrom, bounds, is_described, is_phantom, is_unit_like, serde_default,
    },
    described_variants, is_flattened, positional_members, quote, transparent_member,
};

use proc_macro2::Span;
use syn::{Data, Ident, Index, LitStr};

/// The body of `check_constraints`, which reads `self`, `at` and `violations`.
pub(super) fn body(input: &DeriveInput, container: &Container) -> TokenStream2 {
    match &input.data {
        Data::Struct(data) => {
            let default = container
                .default
                .then(|| serde_default(&input.attrs))
                .flatten();
            struct_check(&data.fields, container, default.as_ref())
        }
        Data::Enum(data) => enum_check(data, container),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => TokenStream2::new(),
    }
}

/// Where a member sits relative to its parent's pointer.
enum Step {
    /// Where its parent is.
    Here,
    /// Under a member name, or under any of several, the read name first.
    Member(Vec<String>),
    /// At an array position.
    Index(usize),
}

/// The value serde would fill a missing member with, as an expression.
struct Filled {
    /// An `Option` of the value the default is taken from: `None` where the
    /// expansion cannot name it.
    value: TokenStream2,
    /// The member's value, reached from `__kynos_filled`, a reference to the
    /// unwrapped [`value`](Self::value).
    project: TokenStream2,
}

/// What serde fills a member with when the document leaves it out: its own
/// `default`, else the container's, read through `access` from the value the
/// container's default gives. `None` where serde fills nothing.
fn filled(
    field: &Field,
    container: Option<&DefaultFrom>,
    access: Option<&TokenStream2>,
) -> Option<Filled> {
    let value = |from: &DefaultFrom, ty: TokenStream2| match from {
        DefaultFrom::Default => quote! {
            (&&::kynos::__private::constraints::Filled::<#ty>::new()).filled()
        },
        DefaultFrom::Path(path) => quote!(::core::option::Option::Some(#path())),
    };

    if let Some(own) = serde_default(&field.attrs) {
        let ty = &field.ty;
        return Some(Filled {
            value: value(&own, quote!(#ty)),
            project: quote!(__kynos_filled),
        });
    }
    let (from, access) = container.zip(access)?;
    Some(Filled {
        value: value(from, quote!(Self)),
        project: quote!(&__kynos_filled.#access),
    })
}

/// A struct's members, read through `self`.
///
/// A transparent struct and a newtype are their member on the wire, which a
/// document therefore cannot leave out, so serde fills nothing there.
fn struct_check(
    fields: &Fields,
    container: &Container,
    default: Option<&DefaultFrom>,
) -> TokenStream2 {
    if container.transparent {
        return transparent_member(fields)
            .map(|field| {
                let access = member_access(fields, field);
                member(&quote!(&self.#access), field, &Step::Here, None)
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
                let filled = filled(field, default, Some(&access));
                member(
                    &quote!(&self.#access),
                    field,
                    &named_step(field, container),
                    filled.as_ref(),
                )
            })
            .collect(),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let field = &unnamed.unnamed[0];
            if is_checked(field) {
                member(&quote!(&self.0), field, &Step::Here, None)
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
                let filled = filled(field, default, Some(&access));
                member(
                    &quote!(&self.#access),
                    field,
                    &Step::Index(position),
                    filled.as_ref(),
                )
            })
            .collect(),
        Fields::Unit => TokenStream2::new(),
    }
}

/// An enum's members, read through a `match` on `self` with one arm per
/// variant that carries any.
fn enum_check(data: &DataEnum, container: &Container) -> TokenStream2 {
    let variants = described_variants(data);
    let names = variants_read_names(&variants, container);
    let arms: Vec<TokenStream2> = variants
        .into_iter()
        .zip(names)
        .filter(|(variant, _)| !is_unit_like(&variant.fields))
        .map(|(variant, names)| arm(variant, &names, container))
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
///
/// `read_as` are the names serde reads the variant under, its read name first.
fn arm(variant: &Variant, read_as: &[String], container: &Container) -> TokenStream2 {
    let ident = &variant.ident;
    let fields = container.fields_of(variant);

    // Where the payload sits, relative to the enum's own pointer.
    let payload = match (&container.tag, &container.content) {
        (Some(_), Some(content)) => Step::Member(vec![content.clone()]),
        (Some(_), None) => Step::Here,
        (None, _) => Step::Member(read_as.to_vec()),
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
                    filled(field, None, None).as_ref(),
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
                        let (step, filled) = if newtype {
                            (Step::Here, None)
                        } else {
                            (Step::Index(index), filled(field, None, None))
                        };
                        checks.push(member(&quote!(#binding), field, &step, filled.as_ref()));
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

    let checks = located(&payload, &quote!({ #(#checks)* }));
    quote! {
        #pattern => #checks
    }
}

/// Whether a member is on the wire serde reads and has a schema to check.
///
/// A `PhantomData` is `null` on the wire and has no `Schema`, so there is
/// nothing to descend into.
pub(super) fn is_checked(field: &Field) -> bool {
    is_described(field) && !is_phantom(&field.ty)
}

/// Where a named field sits: under the name serde reads it by, or where its
/// parent is when flattened.
fn named_step(field: &Field, container: &Container) -> Step {
    if is_flattened(field) {
        Step::Here
    } else {
        Step::Member(read_names(field, container))
    }
}

/// The binding a variant's member is matched into, by its declared position,
/// so it cannot collide with `at`, `violations` or another member.
fn binding(position: usize) -> Ident {
    Ident::new(&format!("__kynos_member_{position}"), Span::call_site())
}

/// How `self` reaches a struct field: its name, or its declared index.
pub(super) fn member_access(fields: &Fields, field: &Field) -> TokenStream2 {
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

/// `checks`, a block reading `at` and `violations`, run with `at` moved by
/// `step`.
///
/// A member under several names is checked from the member itself and
/// reported at its parent, which is where `at` already is.
fn located(step: &Step, checks: &TokenStream2) -> TokenStream2 {
    match step {
        Step::Here => checks.clone(),
        Step::Member(names) if names.len() > 1 => {
            let names = names
                .iter()
                .map(|name| LitStr::new(name, Span::call_site()));
            quote! {
                {
                    ::kynos::__private::constraints::aliased(
                        at,
                        &[#(#names),*],
                        violations,
                        |at, violations| #checks,
                    );
                }
            }
        }
        Step::Member(names) => {
            let locate = names.first().map(|name| {
                let name = LitStr::new(name, Span::call_site());
                quote!(let at = at.member(#name);)
            });
            quote!({ #locate #checks })
        }
        Step::Index(index) => quote!({ let at = at.index(#index); #checks }),
    }
}

/// One member's checks: each of its own bounds, then its value's.
///
/// A member serde fills when the document leaves it out is checked a second
/// time, on the value it would be filled with, only where its own value broke
/// something; what its own value broke is reported only where the filled
/// value breaks nothing. The success path pays for neither.
fn member(
    value: &TokenStream2,
    field: &Field,
    step: &Step,
    filled: Option<&Filled>,
) -> TokenStream2 {
    let ty = &field.ty;
    let keywords = bounds(field)
        .into_iter()
        .filter(|bound| bound.key != "pattern")
        .map(|Bound { key, value: bound }| {
            let bound = bound.map(|bound| quote!(#bound,));
            quote! {
                ::kynos::__private::constraints::#key::<#ty>(value, #bound at, violations);
            }
        });
    let check = quote! {
        #(#keywords)*
        ::kynos::schema::Schema::check_constraints(value, at, violations);
    };

    let checks = match filled {
        None => quote! {
            {
                let value: &#ty = #value;
                #check
            }
        },
        Some(Filled {
            value: filled,
            project,
        }) => quote! {
            {
                #[allow(unused_imports)]
                use ::kynos::__private::constraints::{ByDefault as _, Unfilled as _};

                let __kynos_check = |
                    value: &#ty,
                    violations: &mut ::kynos::schema::constraints::Violations,
                | {
                    #check
                };
                let mut __kynos_sent = ::kynos::schema::constraints::Violations::new();
                __kynos_check(#value, &mut __kynos_sent);
                if !__kynos_sent.is_empty() {
                    if let ::core::option::Option::Some(__kynos_filled) = &#filled {
                        let mut __kynos_own = ::kynos::schema::constraints::Violations::new();
                        __kynos_check(#project, &mut __kynos_own);
                        if __kynos_own.is_empty() {
                            ::kynos::__private::constraints::absorb(violations, __kynos_sent);
                        }
                    }
                }
            }
        },
    };

    located(step, &checks)
}
