//! A part's `Content-Disposition`, read for the field it names.
//!
//! Kynos's own reader rather than `multer`'s, whose scan is not quote-aware
//! and never checks the disposition type.

/// The disposition RFC 7578 section 4.2 requires of every part.
const FORM_DATA: &str = "form-data";

/// What a part's `Content-Disposition` says about the field it carries.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Disposition {
    /// The `name` parameter.
    pub(crate) name: String,
    /// The `filename` parameter, if any.
    pub(crate) file_name: Option<String>,
}

/// Reads a part's `Content-Disposition` value, or says why it names no field.
///
/// RFC 7578 section 4.2 under RFC 6266 section 4.1's grammar: the type is
/// `form-data`, in any case, followed by `;`-separated parameters whose names
/// are case-insensitive and whose values are a `token` or a `quoted-string`
/// (RFC 9110 sections 5.6.2 and 5.6.4), so a `;` or `name=` inside quotes is
/// part of the value. Whitespace is skipped around `;` and `=`, and an empty
/// parameter between two `;` is skipped. `name` is required; `name` and
/// `filename` may each appear once, since RFC 6266 makes a repeated parameter
/// invalid. Any other parameter is read past and ignored, `filename*`
/// included, which RFC 7578 section 4.2 tells senders not to use. A value is
/// returned as sent, unescaped but not percent-decoded: RFC 7578 section
/// 5.1.3 leaves a percent-encoded name indistinguishable from a literal one.
pub(crate) fn parse(field: &str) -> Result<Disposition, String> {
    let (kind, mut rest) = token(skip_ows(field));
    if !kind.eq_ignore_ascii_case(FORM_DATA) {
        return Err(format!("a part's disposition is not `{FORM_DATA}`"));
    }

    let malformed = || format!("a part's `Content-Disposition` is malformed: `{field}`");
    let (mut name, mut file_name) = (None, None);
    while !skip_ows(rest).is_empty() {
        rest = skip_ows(skip_ows(rest).strip_prefix(';').ok_or_else(malformed)?);
        if rest.is_empty() || rest.starts_with(';') {
            continue;
        }

        let (parameter, after) = token(rest);
        let after = (skip_ows(after).strip_prefix('=').map(skip_ows))
            .filter(|_| crate::http::is_token(parameter))
            .ok_or_else(malformed)?;
        let value;
        (value, rest) = if after.starts_with('"') {
            quoted_string(after)
        } else {
            let (value, after) = token(after);
            crate::http::is_token(value).then(|| (value.to_owned(), after))
        }
        .ok_or_else(malformed)?;

        let slot = if parameter.eq_ignore_ascii_case("name") {
            &mut name
        } else if parameter.eq_ignore_ascii_case("filename") {
            &mut file_name
        } else {
            continue;
        };
        if slot.replace(value).is_some() {
            return Err(format!(
                "a part's `Content-Disposition` repeats `{}`",
                parameter.to_ascii_lowercase()
            ));
        }
    }

    let name = name.ok_or("a part declares no field name in its `Content-Disposition`")?;
    Ok(Disposition { name, file_name })
}

/// `input` past its leading RFC 9110 `OWS`.
fn skip_ows(input: &str) -> &str {
    input.trim_start_matches([' ', '\t'])
}

/// `input` split where a `token` would have to end; whether the head is one
/// is the caller's to check.
fn token(input: &str) -> (&str, &str) {
    let end = input
        .find([' ', '\t', ';', '=', '"'])
        .unwrap_or(input.len());
    input.split_at(end)
}

/// The leading RFC 9110 section 5.6.4 `quoted-string` of `input`, unescaped,
/// and what follows it; `None` when unterminated or holding a control
/// character. A non-ASCII character is `obs-text`, read as sent.
fn quoted_string(input: &str) -> Option<(String, &str)> {
    // HTAB, SP, VCHAR and obs-text: what `qdtext` and a `quoted-pair` admit.
    let admitted = |c: char| c == '\t' || (c >= ' ' && c != '\u{7f}');
    let mut value = String::new();
    let mut chars = input.strip_prefix('"')?.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((value, &input[1 + at + 1..])),
            '\\' => value.push(chars.next().map(|(_, c)| c).filter(|&c| admitted(c))?),
            c if admitted(c) => value.push(c),
            _ => return None,
        }
    }
    None
}
