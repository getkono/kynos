//! The constraint kinds a newtype takes from its member.

use super::{
    Container, DeriveInput, Fields, TokenStream2,
    attributes::described_as,
    check::{is_checked, member_access},
    quote, transparent_member,
};

use syn::Data;

/// The constraint kinds a newtype takes from its member, so
/// `#[schema(max_length = 8)] label: Label` bounds the string `Label` wraps.
///
/// The `for<'__kynos>` binder keeps a bound naming no parameter from being
/// refused as trivially false; a member of another kind just leaves the impl
/// inapplicable. A member under `#[schema(as = T)]` lends none: its value is
/// not the `T` it is described as, and a kind lends what it reads by reference.
pub(super) fn newtype(
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
    let Some(member) = member.filter(|member| is_checked(member) && described_as(member).is_none())
    else {
        return TokenStream2::new();
    };

    let name = &input.ident;
    let access = member_access(&data.fields, member);
    let ty = &member.ty;
    let kinds = [
        (quote!(Numeric), quote!(number), quote!(f64)),
        (quote!(Textual), quote!(text), quote!(&str)),
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
