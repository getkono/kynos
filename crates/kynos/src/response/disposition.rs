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
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    schema::registry::Registry,
};

/// How a recipient should present the representation.
///
/// RFC 6266 section 4.2. An unknown type is to be treated as `attachment`, so
/// the two here are the whole of what a sender gains by choosing.
///
/// `#[non_exhaustive]`, because section 4.1 leaves the set open:
/// `disp-ext-type = token` makes a third disposition type an extension the
/// specification sanctions rather than one Kynos would be inventing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
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
/// It implements `EncodeHeaders` and not `DecodeHeaders`, which
/// `RateLimitHeaders` also does. The consequence is worth stating plainly:
/// `Headers<ContentDisposition>` as a *handler argument* does not compile. It
/// is a response header; ask for it in a return type.
///
/// That used to be a panic on the first request, because both directions were
/// defaulted methods on one trait.
///
/// # This group can grow
///
/// `#[non_exhaustive]`, for the reason [`Disposition`] is: RFC 6266 section 4.1
/// admits any `disp-ext-parm`, and Appendix B names four already written down
/// elsewhere — `creation-date`, `modification-date`, `quoted-date-time` and
/// `size`, omitted from this profile only because *the majority of user agents
/// do not implement these*. A change of mind about one of them is then a field
/// rather than a new type, and nothing is lost by reserving the room: the two
/// constructors plus [`filename`](Self::filename) were already the way a value
/// is built.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
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
/// `%` does not survive either, and for a reason that is about the *pair* of
/// parameters rather than about the grammar. Appendix D: *avoid including the
/// percent character followed by two hexadecimal characters (e.g., %A9) in the
/// filename parameter, since some existing implementations consider it to be an
/// escape character, while others will pass it through unchanged.* Leaving it
/// intact makes the fallback equal to an otherwise-ASCII name, which suppresses
/// `filename*` — so the two readings of `50%20off.pdf` have nothing to be
/// settled by. Replacing it costs one substituted character and buys the
/// starred parameter that says exactly which name was meant.
///
/// One `_` per `char` rather than per byte, so a name of non-ASCII characters
/// keeps its length rather than tripling it.
fn ascii_fallback(filename: &str) -> String {
    filename
        .chars()
        .map(|character| match character {
            '"' | '\\' | '%' => '_',
            ' ' => ' ',
            _ if character.is_ascii_graphic() => character,
            _ => '_',
        })
        .collect()
}

impl HeaderParams for ContentDisposition {
    const NAMES: &'static [&'static str] = &["content-disposition"];

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

impl EncodeHeaders for ContentDisposition {
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
}

#[cfg(test)]
mod tests;
