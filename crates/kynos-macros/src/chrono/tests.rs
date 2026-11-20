use syn::{DeriveInput, ItemFn, Type, parse_quote};

use crate::chrono::{input_markers, signature_markers, type_markers};

const MARKER: &str = ":: kynos :: __private :: temporal :: chrono_is_unmaintained";

fn count(tokens: &proc_macro2::TokenStream) -> usize {
    tokens.to_string().matches(MARKER).count()
}

/// Every spelling chrono reaches a field by, nested or not, is one marker.
#[test]
fn a_chrono_type_is_marked_once_wherever_it_is_written() {
    let marked: [Type; 8] = [
        parse_quote!(chrono::NaiveDate),
        parse_quote!(chrono04::TimeDelta),
        parse_quote!(NaiveDateTime),
        parse_quote!(NaiveTime),
        parse_quote!(Option<DateTime<Utc>>),
        parse_quote!(::chrono::DateTime<::chrono::FixedOffset>),
        parse_quote!(Query<Vec<&'static [NaiveDate]>>),
        parse_quote!(<Wrapper<NaiveDate> as Trait>::Output),
    ];
    for ty in &marked {
        let tokens = type_markers(ty);
        assert_eq!(count(&tokens), 1, "{}", quote::quote!(#ty));
        assert!(!tokens.to_string().contains("allow"), "{tokens}");
    }
}

/// jiff's `civil::DateTime` takes no type arguments, which is what tells the
/// two backends' `DateTime` apart.
#[test]
fn types_outside_chrono_are_not_marked() {
    let unmarked: [Type; 6] = [
        parse_quote!(jiff::civil::DateTime),
        parse_quote!(DateTime),
        parse_quote!(jiff::Timestamp),
        parse_quote!(Option<String>),
        parse_quote!(uuid::Uuid),
        parse_quote!((u8, [i32; 4])),
    ];
    for ty in &unmarked {
        assert!(type_markers(ty).is_empty(), "{}", quote::quote!(#ty));
    }
}

#[test]
fn every_variant_and_tuple_field_is_read() {
    let input: DeriveInput = parse_quote! {
        enum Event {
            Booked { at: chrono::DateTime<chrono::Utc>, by: String },
            Moved(NaiveDate, NaiveDate),
            Cancelled,
        }
    };
    assert_eq!(count(&input_markers(&input)), 3);
}

#[test]
fn a_handler_is_read_across_its_parameters_and_its_return_type() {
    let function: ItemFn = parse_quote! {
        async fn on(Query(day): Query<NaiveDate>, id: Path<u64>) -> Json<Vec<NaiveDateTime>> {}
    };
    assert_eq!(count(&signature_markers(&function.sig)), 2);
}
