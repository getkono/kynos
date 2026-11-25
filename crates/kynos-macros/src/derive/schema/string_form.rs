//! The expansion of a `#[schema(string)]` type: `type: string`, whatever the
//! type declares, bounded and checked on the text its `Display` writes.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::{DeriveInput, LitStr};

use super::{
    attributes::{bounds, constraints},
    check::value::value_checks,
    container::StringForm,
    deprecate, described,
    shape::typed,
};
use crate::doc::{doc_string, is_deprecated};

/// The `Schema` impl of a `#[schema(string)]` type, carrying
/// `constraint_problem`, the type's own `CONSTRAINT_PROBLEM` item if it names
/// one.
///
/// `schema` records on the registry each example `FromStr` refuses or whose
/// value breaks the text's bounds, which the router reports as a violation
/// when it is built, so a document never shows an example the type refuses.
pub(super) fn expand(
    input: &DeriveInput,
    form: &StringForm,
    constraint_problem: Option<&TokenStream2>,
) -> TokenStream2 {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let component = LitStr::new(&name.to_string(), name.span());

    let keywords = typed("keywords", "String");
    let examples = &form.examples;
    let listed = (!examples.is_empty()).then(|| {
        quote! {
            keywords.examples = ::core::option::Option::Some(::std::vec![
                #(::core::convert::Into::into(#examples)),*
            ]);
        }
    });
    let constrained =
        constraints(&form.text).map(|constraints| quote!(let schema = #constraints.apply(schema);));
    let schema = quote! {
        {
            #keywords
            #listed
            let schema = ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords));
            #constrained
            schema
        }
    };
    let schema = deprecate(
        described(schema, doc_string(&input.attrs).as_deref()),
        is_deprecated(&input.attrs),
    );

    // Spanned at `string`, so a type `FromStr` or `Display` does not carry
    // is reported at the key that requires them.
    let carried = quote_spanned! {form.span=>
        let _: fn(&::core::primitive::str) -> _ = <Self as ::core::str::FromStr>::from_str;
        let _ = <Self as ::core::fmt::Display>::fmt;
    };
    let admitted = (!examples.is_empty()).then(|| {
        quote! {
            ::kynos::__private::string_form::admit_examples::<Self>(
                registry,
                &[#(#examples),*],
            );
        }
    });

    // The text is written only where a bound reads it.
    let check = (!bounds(&form.text).is_empty()).then(|| {
        let checks = value_checks(&form.text);
        quote! {
            let text = ::std::string::ToString::to_string(self);
            let value: &::std::string::String = &text;
            #checks
        }
    });

    quote! {
        // The impl names the type, so a `#[deprecated]` type would otherwise warn
        // at its own definition (as serde's derives also allow).
        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::Schema for #name #ty_generics #where_clause {
            #constraint_problem

            // The string describes itself, so it resolves nothing.
            #[allow(unused_variables)]
            fn schema(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Schema {
                #carried
                #admitted
                #schema
            }

            fn name() -> ::core::option::Option<::kynos::openapi::ComponentName> {
                ::kynos::openapi::ComponentName::sanitized(#component).ok()
            }

            // A type with no bound reads neither argument.
            #[allow(unused_variables)]
            fn check_constraints(
                &self,
                at: ::kynos::schema::constraints::Pointer<'_>,
                violations: &mut ::kynos::schema::constraints::Violations,
            ) {
                #check
            }
        }
    }
}
