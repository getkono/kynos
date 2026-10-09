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
//! # What the encoder does with it
//!
//! `filename` is always sent, as a `quoted-string`; `filename*` follows only
//! when the quoted fallback lost something (RFC 6266 Appendix D).
//!
//! Encoding is total: no filename is refused, so there is no `Result`.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, header},
    schema::registry::Registry,
};

/// How a recipient should present the representation.
///
/// RFC 6266 section 4.2. `#[non_exhaustive]`, because section 4.1's
/// `disp-ext-type` leaves the set open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Disposition {
    /// Prompt to save the representation rather than process it.
    Attachment,
    /// Process the representation as its media type says. Worth sending only
    /// with a filename, for a later save.
    Inline,
}

impl Disposition {
    /// Every disposition, for the table test that keeps the token map closed.
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
/// A header group composed through
/// [`WithHeaders`](crate::response::headers::WithHeaders); the body's own type
/// already states the media type.
///
/// ```
/// use kynos::{
///     extract::body::binary::Binary,
///     http::media::Pdf,
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
/// There is deliberately no `Default`: the disposition is the choice this type
/// records.
///
/// The group is [`DESCRIBED`](HeaderParams::DESCRIBED), since it changes what a
/// consumer does with the response. It is a response header only:
/// `Headers<ContentDisposition>` as a handler argument does not compile.
///
/// `#[non_exhaustive]`, because RFC 6266 section 4.1 admits any
/// `disp-ext-parm`; build a value through the constructors and
/// [`filename`](Self::filename).
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
    /// Not validated or sanitised beyond what the grammars require: RFC 6266
    /// section 4.3 makes stripping path segments the recipient's job, so `/`
    /// and `\` reach the field (escaped where the grammar needs it). A name
    /// carrying CR, LF or NUL still cannot end the field early.
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

        // Unstarred first and unconditionally (RFC 6266 Appendix D).
        let fallback = ascii_fallback(filename);
        value.push_str("; filename=\"");
        value.push_str(&fallback);
        value.push('"');

        // The starred spelling only where the fallback lost something.
        if fallback != *filename {
            value.push_str("; filename*=UTF-8''");
            value.push_str(&crate::__private::uri::encode_ext_value(filename));
        }

        value
    }
}

/// The `quoted-string` fallback: what survives, and one `_` for what does not.
///
/// A space and every ASCII graphic survive except `"`, `\` (which recipients
/// mishandle) and `%` (which some read as an escape, RFC 6266 Appendix D);
/// replacing `%` also forces `filename*` to state the exact name. One `_` per
/// `char`, not per byte.
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

        // No `pattern`: a regex for this grammar would be wrong at the edges,
        // and a wrong constraint is worse than none.
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
            // `field_value` emits printable ASCII only.
            HeaderValue::from_str(&value).expect("a field value of printable ASCII"),
        )]
    }
}

#[cfg(test)]
mod tests;
