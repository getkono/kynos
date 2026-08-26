//! What `assets!` was given.

use syn::{Attribute, Ident, LitStr, Token, Visibility, parse::Parse};

/// The threshold past which an embedded set warns, unless told otherwise.
///
/// Two mebibytes: where a set stops being a bundle and a favicon. Being wrong
/// about it costs one attribute, which is why it is a default rather than a
/// rule.
pub(super) const DEFAULT_WARN_OVER: usize = 2 * 1024 * 1024;

/// The parsed invocation.
pub(crate) struct AssetArgs {
    pub(super) docs: Vec<Attribute>,
    pub(super) visibility: Visibility,
    pub(super) name: Ident,
    pub(super) dir: LitStr,
    pub(super) exclude: Vec<LitStr>,
    /// `None` where the check was turned off.
    pub(super) warn_over: Option<usize>,
}

impl Parse for AssetArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let docs = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = input.parse()?;

        input.parse::<Token![struct]>().map_err(|error| {
            syn::Error::new(
                error.span(),
                "an asset set needs a unit struct to name it: `pub struct Site;`",
            )
        })?;
        let name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;

        let mut dir: Option<LitStr> = None;
        let mut exclude = Vec::new();
        let mut warn_over = Some(DEFAULT_WARN_OVER);

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "dir" => {
                    input.parse::<Token![=]>()?;
                    dir = Some(input.parse()?);
                }
                "exclude" => {
                    input.parse::<Token![=]>()?;
                    let content;
                    syn::bracketed!(content in input);
                    for entry in content.parse_terminated(<LitStr as Parse>::parse, Token![,])? {
                        if entry.value().is_empty() {
                            return Err(syn::Error::new(
                                entry.span(),
                                "an `exclude` entry is a file name or an extension beginning with \
                                 a dot, and an empty string is neither",
                            ));
                        }
                        exclude.push(entry);
                    }
                }
                "warn_over" => {
                    input.parse::<Token![=]>()?;
                    let value: LitStr = input.parse()?;
                    warn_over = parse_size(&value)?;
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "`{other}` is not part of the `assets!` grammar, which takes `dir`, \
                             `exclude` and `warn_over`"
                        ),
                    ));
                }
            }

            // A trailing comma is allowed, which is what makes the last option
            // look like every other one.
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        let Some(dir) = dir else {
            return Err(syn::Error::new(
                name.span(),
                "an asset set needs `dir = \"...\"`, relative to the crate root",
            ));
        };

        Ok(Self {
            docs,
            visibility,
            name,
            dir,
            exclude,
            warn_over,
        })
    }
}

/// A size in IEC units, or `None` for the word `none`.
///
/// IEC only. `KB` is ambiguous — a thousand bytes to a disk vendor and 1024 to
/// everyone else — and a threshold that means one thing to the writer and
/// another to the reader is worse than no threshold.
fn parse_size(value: &LitStr) -> syn::Result<Option<usize>> {
    let text = value.value();
    if text == "none" {
        return Ok(None);
    }

    let malformed = || {
        syn::Error::new(
            value.span(),
            "`warn_over` takes a size such as \"4MiB\" or the word \"none\"; the units are B, \
             KiB, MiB and GiB",
        )
    };

    for (suffix, scale) in [
        ("GiB", 1024 * 1024 * 1024),
        ("MiB", 1024 * 1024),
        ("KiB", 1024),
        ("B", 1),
    ] {
        if let Some(number) = text.strip_suffix(suffix) {
            let number: usize = number.trim().parse().map_err(|_| malformed())?;
            return Ok(Some(number * scale));
        }
    }

    Err(malformed())
}
