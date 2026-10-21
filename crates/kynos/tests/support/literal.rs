//! Reading a Rust string literal out of source text, for the suites that hold
//! a snapshot to what some source says: `ui.rs` over this crate's own, and
//! `ledger.rs` over `kynos-macros`'.
//!
//! Included with `#[path]` rather than depended on, for the reason
//! [`support/mod.rs`](mod.rs) gives.

/// The value of the string literal whose opening quote precedes `text`, with
/// its escapes and line continuations resolved as rustc resolves them, or
/// `None` where it is unclosed or holds an escape rustc refuses.
pub(crate) fn string_literal(text: &str) -> Option<String> {
    let mut value = String::new();
    let mut characters = text.chars().peekable();
    loop {
        match characters.next()? {
            '"' => return Some(value),
            '\\' => match characters.next()? {
                '\n' => while characters.next_if(|next| next.is_whitespace()).is_some() {},
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                '0' => value.push('\0'),
                'x' => {
                    let digits: String = characters.by_ref().take(2).collect();
                    let code = u8::from_str_radix(&digits, 16).ok().filter(u8::is_ascii)?;
                    value.push(char::from(code));
                }
                'u' => {
                    let digits: String = characters
                        .by_ref()
                        .skip(1)
                        .take_while(|next| *next != '}')
                        .filter(|next| *next != '_')
                        .collect();
                    let code = u32::from_str_radix(&digits, 16).ok()?;
                    value.push(char::from_u32(code)?);
                }
                escaped @ ('\\' | '\'' | '"') => value.push(escaped),
                _ => return None,
            },
            character => value.push(character),
        }
    }
}
