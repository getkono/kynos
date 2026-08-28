use super::{DecodeQuery, QueryParams};
use crate::{
    error::rejection::QueryRejection,
    schema::{Schema, registry::Registry},
};

// Two `#[should_panic]` cases stood here, asserting that a group declaring
// neither direction said so at run time. `DecodeQuery` and `EncodeQuery`
// are separate traits now, so such a group does not compile and there is
// nothing left to provoke. The control below stays: it is what proves the
// decoder is reached, which is the half a compile-time guarantee cannot
// say anything about.

/// The control: a group that declares a decoder is not touched by the
/// default, and sees the distinction the signature draws — `None` for a
/// request with no `?` at all, `Some("")` for a bare one.
#[test]
fn a_group_that_declares_a_decoder_sees_the_query_it_was_given() {
    #[derive(Debug, PartialEq)]
    struct Recorded(Option<String>);

    impl Schema for Recorded {
        fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
            let _ = registry;
            kynos_openapi::Schema::any()
        }
    }

    impl QueryParams for Recorded {}

    impl DecodeQuery for Recorded {
        fn decode(query: Option<&str>) -> Result<Self, QueryRejection> {
            Ok(Self(query.map(str::to_owned)))
        }
    }

    assert_eq!(Recorded::decode(None).expect("decoded"), Recorded(None));
    assert_eq!(
        Recorded::decode(Some("")).expect("decoded"),
        Recorded(Some(String::new()))
    );
    assert_eq!(
        Recorded::decode(Some("a=1")).expect("decoded"),
        Recorded(Some("a=1".to_owned()))
    );
}

/// A structured syntax suffix is JSON, which is what lets a vendor media
/// type be decoded as the JSON it is.
#[cfg(feature = "openapi32")]
#[test]
fn a_json_suffixed_media_type_is_read_as_json() {
    use super::is_json;

    assert!(is_json("application/json"));
    assert!(is_json("application/json; charset=utf-8"));
    assert!(is_json("APPLICATION/JSON"));
    assert!(is_json("application/vnd.acme.filter+json"));

    assert!(!is_json("application/xml"));
    assert!(!is_json("text/plain"));
    // A suffix is a suffix of the base type, not of the parameters.
    assert!(!is_json("application/xml; note=+json"));
}
