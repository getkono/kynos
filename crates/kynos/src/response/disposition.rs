//! Saying whether a representation is meant to be saved or shown.
//!
//! # The grammar
//!
//! RFC 6266 section 4.1, which is the profile of the field HTTP uses:
//!
//! ```text
//! content-disposition = "Content-Disposition" ":"
//!                        disposition-type *( ";" disposition-parm )
//!
//! disposition-type    = "inline" | "attachment" | disp-ext-type
//!                     ; case-insensitive
//! disp-ext-type       = token
//!
//! disposition-parm    = filename-parm | disp-ext-parm
//!
//! filename-parm       = "filename" "=" value
//!                     | "filename*" "=" ext-value
//!
//! disp-ext-parm       = token "=" value
//!                     | ext-token "=" ext-value
//! ext-token           = <the characters in token, followed by "*">
//!
//! value               = token | quoted-string
//! ```
//!
//! and RFC 8187 section 3.2.1 for what the starred spelling carries:
//!
//! ```text
//! ext-value     = charset  "'" [ language ] "'" value-chars
//! charset       = "UTF-8" / mime-charset
//! value-chars   = *( pct-encoded / attr-char )
//! pct-encoded   = "%" HEXDIG HEXDIG
//! attr-char     = ALPHA / DIGIT
//!               / "!" / "#" / "$" / "&" / "+" / "-" / "."
//!               / "^" / "_" / "`" / "|" / "~"
//!               ; token except ( "*" / "'" / "%" )
//! ```
//!
//! The grammar is transcribed here rather than cited, so a reviewer can check
//! the encoder against it without leaving the file.
//!
//! # What the encoder does with it
//!
//! Only the `quoted-string` half of `value` is ever produced, because the
//! `token` half is a subset of it and choosing between them per filename buys
//! nothing. The unstarred parameter always comes first, and the starred one
//! only when the unstarred one lost something — both of which RFC 6266
//! Appendix D asks for: a recipient understanding only `filename` must find
//! one, and a recipient understanding `filename*` prefers it where it appears.
//!
//! Encoding is total. There is no filename this module refuses, and no
//! `Result` for a caller to handle, because every byte outside the two
//! grammars above has an escape in one of them.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue, header},
    schema::registry::Registry,
};

/// How a recipient should present the representation.
///
/// RFC 6266 section 4.2. An unknown type is to be treated as `attachment`, so
/// the two here are the whole of what a sender gains by choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Disposition {
    /// Prompt to save the representation rather than process it.
    Attachment,
    /// Process the representation as its media type says, which is what a
    /// recipient does anyway — so this is worth sending only alongside a
    /// filename, to be remembered for a later save.
    Inline,
}

impl Disposition {
    /// Every disposition, for the table test that keeps the token map closed.
    ///
    /// Test-only because nothing on the sending path iterates the set: a value
    /// arrives already chosen.
    #[cfg(test)]
    const ALL: [Self; 2] = [Self::Attachment, Self::Inline];

    /// The `disposition-type` token this variant is named by on the wire.
    const fn token(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::Inline => "inline",
        }
    }
}

/// A `Content-Disposition` a response carries.
///
/// A header group like any other, composed through
/// [`WithHeaders`](crate::response::headers::WithHeaders). There is no
/// `Attachment<T>` wrapper, because the media type of a download is already the
/// body's own type — `Binary<Pdf>` says `application/pdf` — and what is left
/// once the media type is spoken for is the disposition, which is a header.
///
/// ```
/// use kynos::{
///     extract::{body::binary::Binary, media::Pdf},
///     response::{disposition::ContentDisposition, headers::WithHeaders},
/// };
///
/// # fn statement() -> Vec<u8> { Vec::new() }
/// fn download() -> WithHeaders<Binary<Pdf>, ContentDisposition> {
///     WithHeaders::new(
///         Binary::new(statement()),
///         ContentDisposition::attachment().filename("statement.pdf"),
///     )
/// }
/// ```
///
/// There is deliberately no `Default`: a response that has not said whether it
/// is to be saved or shown has not made the choice this type exists to record.
///
/// # This group is described
///
/// [`DESCRIBED`](HeaderParams::DESCRIBED) stays `true`, where `Vary` and
/// `Content-Encoding` set it `false`. Those two change how a response is
/// *transferred*, and every client already handles them without being told.
/// This one changes what a consumer *does* with the response — save it under a
/// name, or render it — so a generated client that has not been told about it
/// cannot offer the download.
///
/// # This group is written, not read
///
/// [`decode`](HeaderParams::decode) is left at its panicking default, which the
/// trait sanctions for a response-direction group and which
/// `RateLimitHeaders` does the same with. The consequence is worth stating
/// plainly: `Headers<ContentDisposition>` as a *handler argument* panics. It is
/// a response header; ask for it in a return type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDisposition {
    /// How the recipient should present the representation.
    pub disposition: Disposition,
    /// The filename to suggest, if any.
    pub filename: Option<String>,
}

impl ContentDisposition {
    /// A disposition asking the recipient to save the representation.
    #[must_use]
    pub fn attachment() -> Self {
        Self {
            disposition: Disposition::Attachment,
            filename: None,
        }
    }

    /// A disposition asking the recipient to present the representation as its
    /// media type says.
    #[must_use]
    pub fn inline() -> Self {
        Self {
            disposition: Disposition::Inline,
            filename: None,
        }
    }

    /// Suggests a filename.
    ///
    /// Validated for nothing, and sanitised for nothing beyond what the two
    /// grammars require. RFC 6266 section 4.3 makes the filename advisory and
    /// puts the handling of it on the recipient — which MUST NOT write outside
    /// a location it is entitled to, and is told in so many words to strip path
    /// segments delimited by `/` and `\`. So `/` and `\` survive this encoder
    /// rather than being rewritten: mangling them here would make a legitimate
    /// name wrong without making a hostile one safe, since the recipient
    /// performs that strip either way. Section 5's own example carries a space,
    /// for the same reason.
    ///
    /// What the encoder *does* guarantee is that the field value is one field
    /// value: a name carrying CR, LF or NUL cannot end the field early, because
    /// neither the quoted-string form nor the extended form has a spelling for
    /// those bytes that leaves them intact.
    #[must_use]
    pub fn filename(mut self, name: impl Into<String>) -> Self {
        self.filename = Some(name.into());
        self
    }

    /// The field value, per the grammar in the module documentation.
    fn field_value(&self) -> String {
        let mut value = self.disposition.token().to_owned();

        let Some(filename) = &self.filename else {
            return value;
        };

        // Unstarred first and unconditionally: RFC 6266 Appendix D, so a
        // recipient that knows only this spelling finds a name, and one that
        // knows both prefers the starred one it meets afterwards.
        let fallback = ascii_fallback(filename);
        value.push_str("; filename=\"");
        value.push_str(&fallback);
        value.push('"');

        // And the starred spelling only where the fallback lost something.
        // Sending it for a name it cannot improve on is two parameters saying
        // one thing, which Appendix D warns costs more than it buys.
        if fallback != *filename {
            value.push_str("; filename*=UTF-8''");
            value.push_str(&crate::__private::uri::encode_ext_value(filename));
        }

        value
    }
}

/// The `quoted-string` fallback: what survives, and one `_` for what does not.
///
/// A space and every `qdtext` graphic survive, which keeps `;` and `,` —
/// neither can end a quoted-string, so replacing them would mangle an ordinary
/// name for nothing. `"` and `\` do not survive: the first ends the string and
/// the second is the escape Appendix D says some recipients mishandle, so
/// neither is worth spelling.
///
/// One `_` per `char` rather than per byte, so a name of non-ASCII characters
/// keeps its length rather than tripling it.
fn ascii_fallback(filename: &str) -> String {
    filename
        .chars()
        .map(|character| match character {
            '"' | '\\' => '_',
            ' ' => ' ',
            _ if character.is_ascii_graphic() => character,
            _ => '_',
        })
        .collect()
}

impl HeaderParams for ContentDisposition {
    const NAMES: &'static [&'static str] = &["content-disposition"];

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        let value = self.field_value();
        vec![(
            header::CONTENT_DISPOSITION,
            // Infallible by construction: `field_value` emits a token, the
            // `qdtext` `ascii_fallback` admits, and `attr-char` and percent
            // triplets — every one of which is printable ASCII.
            HeaderValue::from_str(&value).expect("a field value of printable ASCII"),
        )]
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // A bare string, with no `pattern`. The grammar above is long enough
        // that a regular expression transcribing it would be wrong at the
        // edges, and a wrong constraint in the description is worse than none
        // -- which is the call `status.rs` makes for `Location` too.
        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "Content-Disposition".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description(
                        "Whether to save or show the representation, and the filename to suggest",
                    )
                    .required(true),
            ),
        );
        headers
    }
}

#[cfg(test)]
mod tests {
    use kynos_openapi::model::schema::types::SchemaType;

    use super::{ContentDisposition, Disposition};
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
        let cases: [(Option<&str>, &str); 8] = [
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

        let described =
            <WithHeaders<Created<Binary<Pdf>>, ContentDisposition> as Responses>::responses(
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
}
