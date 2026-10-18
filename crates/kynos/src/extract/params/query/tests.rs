use super::{DecodeQuery, Query, QueryParams};
use crate::{
    error::rejection::QueryRejection,
    extract::FromRequestParts,
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
/// request with no `?` at all, `Some("")` for a bare one. Driven through
/// `Query`'s extractor, so what is proved is what a request hands the
/// decoder rather than what the fixture's own `decode` returns.
#[tokio::test]
async fn a_group_that_declares_a_decoder_sees_the_query_it_was_given() {
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

    for (uri, expected) in [
        ("/reports", None),
        ("/reports?", Some("")),
        ("/reports?a=1", Some("a=1")),
    ] {
        let (mut parts, ()) = http::Request::get(uri)
            .body(())
            .expect("a well-formed request")
            .into_parts();
        let Query(recorded) = Query::<Recorded>::from_request_parts(&mut parts, &())
            .await
            .expect("decoded");

        assert_eq!(recorded, Recorded(expected.map(str::to_owned)), "{uri}");
    }
}
