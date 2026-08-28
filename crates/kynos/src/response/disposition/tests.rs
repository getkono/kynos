use kynos_openapi::model::schema::types::SchemaType;

use super::{ContentDisposition, Disposition, EncodeHeaders};
use crate::{
    extract::params::header::HeaderParams,
    extract::{body::binary::Binary, media::Pdf},
    http::header,
    response::{IntoResponse, Responses, headers::WithHeaders, status::Created},
    schema::registry::Registry,
};

/// The one field the group sends, read back off a response.
fn sent(group: &ContentDisposition) -> String {
    let response = WithHeaders::new((), group.clone()).into_response();
    response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .expect("the group always sends one")
        .to_str()
        .expect("a printable field")
        .to_owned()
}

/// A closed table: the `match` has no wildcard arm and `ALL` has a fixed
/// length, so a variant added without a token fails to compile twice.
#[test]
fn every_disposition_sends_its_registered_token() {
    for disposition in Disposition::ALL {
        let token = match disposition {
            Disposition::Attachment => "attachment",
            Disposition::Inline => "inline",
        };

        assert_eq!(disposition.token(), token);
        assert_eq!(
            sent(&ContentDisposition {
                disposition,
                filename: None
            }),
            token
        );
    }
}

/// The exact bytes, which is the whole of what RFC 6266 section 4.1 and
/// RFC 8187 section 3.2.1 constrain.
#[test]
fn a_filename_is_written_as_the_two_rfcs_spell_it() {
    let cases: [(Option<&str>, &str); 9] = [
        (None, "attachment"),
        (Some("report.pdf"), "attachment; filename=\"report.pdf\""),
        (
            Some("quarterly report.pdf"),
            "attachment; filename=\"quarterly report.pdf\"",
        ),
        (Some("a;b,c.txt"), "attachment; filename=\"a;b,c.txt\""),
        (
            Some("résumé.pdf"),
            "attachment; filename=\"r_sum_.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf",
        ),
        // Appendix D: *avoid including the percent character followed by
        // two hexadecimal characters (e.g., %A9) in the filename parameter,
        // since some existing implementations consider it to be an escape
        // character, while others will pass it through unchanged.* Left
        // intact the fallback would be the whole name, which suppresses
        // `filename*` and leaves nothing to disambiguate `50 off.pdf` from
        // `50%20off.pdf`.
        (
            Some("50%20off.pdf"),
            "attachment; filename=\"50_20off.pdf\"; filename*=UTF-8''50%2520off.pdf",
        ),
        (
            Some("a\"b.txt"),
            "attachment; filename=\"a_b.txt\"; filename*=UTF-8''a%22b.txt",
        ),
        (
            Some("a\r\nX-Injected: yes"),
            "attachment; filename=\"a__X-Injected: yes\"; \
             filename*=UTF-8''a%0D%0AX-Injected%3A%20yes",
        ),
        (Some(""), "attachment; filename=\"\""),
    ];

    for (filename, expected) in cases {
        let group = filename.map_or_else(ContentDisposition::attachment, |name| {
            ContentDisposition::attachment().filename(name)
        });
        assert_eq!(sent(&group), expected, "for {filename:?}");
    }
}

/// No filename can end the field early, whichever half of the value it
/// reaches.
#[test]
fn no_filename_can_split_the_field_it_is_written_into() {
    let long = "n".repeat(300);
    let fixtures = [
        "report.pdf",
        "résumé.pdf",
        "\"quoted\".txt",
        "back\\slash.txt",
        "a;b,c.txt",
        "a\r\nX-Injected: yes",
        "📄.pdf",
        "trailing\\",
        "",
        "nul\0byte.txt",
        long.as_str(),
    ];

    for fixture in fixtures {
        let group = ContentDisposition::inline().filename(fixture);
        let (_, value) = group
            .encode()
            .pop()
            .expect("the group always sends one field");

        assert!(
            !value
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b'\r' | b'\n' | 0)),
            "`{fixture}` reached the wire with a field-ending byte"
        );
    }
}

/// What the group sends is what the group declares.
#[test]
fn the_group_describes_the_field_it_sends() {
    assert!(std::hint::black_box(
        <ContentDisposition as HeaderParams>::DESCRIBED
    ));

    let declared = ContentDisposition::response_headers(&mut Registry::default());
    let kynos_openapi::RefOr::Item(header) = declared
        .get("Content-Disposition")
        .expect("the canonical spelling")
    else {
        panic!("described inline rather than as a `$ref`");
    };

    assert_eq!(header.required, Some(true));
    assert!(
        header
            .description
            .as_deref()
            .is_some_and(|description| !description.is_empty())
    );
    assert_eq!(
        header.schema(),
        Some(&kynos_openapi::Schema::of_type(SchemaType::String))
    );
}

/// The disposition rides the status the body declares, not a 200 the
/// wrapper invented.
#[test]
fn a_disposition_rides_every_status_the_body_declares() {
    let value = "attachment; filename=\"r_sum_.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf";

    let response = WithHeaders::new(
        Binary::<Pdf>::new(&b"%PDF-1.7"[..]),
        ContentDisposition::attachment().filename("résumé.pdf"),
    )
    .into_response();

    assert_eq!(response.status(), crate::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .expect("the group always sends one"),
        value
    );

    let described = <WithHeaders<Created<Binary<Pdf>>, ContentDisposition> as Responses>::responses(
        &mut Registry::default(),
    );

    assert!(!described.responses.contains_key("200"));
    let kynos_openapi::RefOr::Item(created) =
        described.responses.get("201").expect("the body's status")
    else {
        panic!("described as a `$ref`");
    };
    assert!(created.headers.contains_key("Content-Disposition"));
}
