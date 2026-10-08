//! The constraint kinds a newtype takes from its member.

use super::{
    Container, DeriveInput, Fields, TokenStream2,
    check::{is_checked, member_access},
    quote, transparent_member,
};

use syn::Data;

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
    let Some(member) = member.filter(|member| is_checked(member)) else {
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
