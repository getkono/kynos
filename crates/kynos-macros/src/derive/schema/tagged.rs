//! The `Tagged` implementation of a type serde writes under its own tag.

use super::{
    Container, DeriveInput, Fields, TokenStream2, attributes::variant_name, container::Visitor,
    members::is_written, quote,
};

use syn::Data;

/// The `Tagged` implementation of a type serde writes under its own tag, or
/// nothing for one it writes untagged.
///
/// Its names are the ones the schema gives the tag property: each variant's
/// [`variant_name`], and a tagged struct's `struct_name`. An identifier enum
/// is never written, and a transparent or unnamed struct carries no tag serde
/// writes.
pub(super) fn emit(
    input: &DeriveInput,
    container: &Container,
    struct_name: &str,
) -> Option<TokenStream2> {
    let tag = container.tag.as_deref()?;
    let (variants, variant) = match &input.data {
        Data::Enum(data) if container.visitor == Visitor::Enum => {
            let written = data
                .variants
                .iter()
                .filter(|variant| is_written(variant))
                .map(|variant| variant_name(variant, container));
            let arms = data.variants.iter().map(|variant| {
                let ident = &variant.ident;
                let name = variant_name(variant, container);
                quote!(Self::#ident { .. } => #name,)
            });
            (quote!(&[#(#written),*]), quote!(match self { #(#arms)* }))
        }
        Data::Struct(data)
            if matches!(data.fields, Fields::Named(_))
                && !container.transparent
                && container.content.is_none() =>
        {
            (quote!(&[#struct_name]), quote!(#struct_name))
        }
        _ => return None,
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Some(quote! {
        // The impl names the type, as the `Schema` impl does.
        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::Tagged for #name #ty_generics #where_clause {
            const TAG: &'static str = #tag;
            const VARIANTS: &'static [&'static str] = #variants;

            fn variant(&self) -> &'static str {
                #variant
            }
        }
    })
}
