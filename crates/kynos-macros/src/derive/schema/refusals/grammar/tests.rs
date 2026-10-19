//! [`is_media_type`] swept against RFC 9110's `media-type` written as an
//! automaton over character classes, rather than spot-checked by sample.

use super::is_media_type;

/// One representative of each class the grammar tells apart.
const CLASSES: [char; 11] = [
    'a',      // tchar
    '/',      // the type/subtype separator
    ';',      // the parameter separator
    '=',      // the name/value separator
    '"',      // DQUOTE
    '\\',     // the quoted-pair escape
    ' ',      // SP
    '\t',     // HTAB
    '\u{1}',  // CTL other than HTAB
    ',',      // a VCHAR that is no tchar and no separator above
    '\u{e9}', // obs-text, which only a quoted-string admits
];

/// The states of the automaton, named by what has been read.
#[derive(Clone, Copy)]
enum State {
    Start,
    Type,
    Slash,
    Subtype,
    /// OWS read after an element, which only a `;` may follow.
    Trailing,
    /// A `;` and any OWS after it, where a parameter is optional.
    Separated,
    Name,
    Equals,
    Token,
    Quoted,
    Escaped,
    Closed,
}

/// RFC 9110's `tchar` (section 5.6.2), spelt out rather than shared.
fn tchar(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

/// `type "/" subtype *( OWS ";" OWS [ token "=" ( token / quoted-string ) ] )`
/// (sections 8.3.1, 5.6.6 and 5.6.4), one character at a time.
fn grammar(value: &str) -> bool {
    use State::{
        Closed, Equals, Escaped, Name, Quoted, Separated, Slash, Start, Subtype, Token, Trailing,
        Type,
    };

    let mut state = Start;
    for character in value.chars() {
        let ows = matches!(character, ' ' | '\t');
        // qdtext and quoted-pair's operand, less what each excludes.
        let text = ows || ('\u{21}'..='\u{7e}').contains(&character) || !character.is_ascii();
        state = match state {
            Start | Type if tchar(character) => Type,
            Type if character == '/' => Slash,
            Slash | Subtype if tchar(character) => Subtype,
            Subtype | Token | Closed | Trailing if ows => Trailing,
            Subtype | Token | Closed | Trailing | Separated if character == ';' => Separated,
            Separated if ows => Separated,
            Separated | Name if tchar(character) => Name,
            Name if character == '=' => Equals,
            Equals | Token if tchar(character) => Token,
            Equals if character == '"' => Quoted,
            Quoted if character == '"' => Closed,
            Quoted if character == '\\' => Escaped,
            Quoted | Escaped if text => Quoted,
            _ => return false,
        };
    }
    matches!(state, Subtype | Separated | Token | Closed)
}

/// Every string of `length` characters drawn from [`CLASSES`], after `prefix`.
fn strings(prefix: &str, length: u32) -> impl Iterator<Item = String> + '_ {
    let base = CLASSES.len();
    (0..base.pow(length)).map(move |mut index| {
        let mut value = prefix.to_owned();
        for _ in 0..length {
            value.push(CLASSES[index % base]);
            index /= base;
        }
        value
    })
}

/// Every class sequence up to five long, and five more after each prefix that
/// reaches a parameter, its value, and a quoted string's interior.
#[test]
fn is_media_type_is_the_rfc_9110_grammar() {
    let mut accepted = 0;
    for prefix in ["", "a/a", "a/a;a=", "a/a;a=\""] {
        for length in 0..=5 {
            for value in strings(prefix, length) {
                let expected = grammar(&value);
                assert_eq!(is_media_type(&value), expected, "{value:?}");
                accepted += usize::from(expected);
            }
        }
    }
    // The control: a sweep accepting nothing would agree with a grammar
    // accepting nothing.
    assert!(accepted > 1000, "the sweep accepted only {accepted}");
}

/// Each character in each class position, so a class's representative is not
/// the only member held to it.
#[test]
fn every_character_is_classified_as_the_grammar_classifies_it() {
    let characters = ('\u{0}'..='\u{7f}').chain(['\u{e9}', '\u{2028}', '\u{1f600}']);
    for character in characters {
        for template in [
            "a/#",
            "#/a",
            "a/a;#=a",
            "a/a;a=#",
            "a/a;a=\"#\"",
            "a/a;a=\"\\#\"",
        ] {
            let value = template.replace('#', &character.to_string());
            assert_eq!(is_media_type(&value), grammar(&value), "{value:?}");
        }
    }
}

/// What the derive raises for a value the grammar refuses is the
/// `content_media_type` refusal, and a value it admits raises nothing.
#[test]
fn a_refused_media_type_raises_the_media_type_refusal() {
    for length in 0..=4 {
        for value in strings("", length).chain(strings("a/a;a=\"", length)) {
            let input = syn::parse2(quote::quote!(
                struct Upload {
                    #[schema(content_media_type = #value)]
                    torrent: String,
                }
            ))
            .expect("the declaration parses");

            match crate::derive::schema::expand_inner(&input) {
                Ok(_) => assert!(grammar(&value), "the derive accepted {value:?}"),
                Err(error) => {
                    assert!(!grammar(&value), "the derive refused {value:?}: {error}");
                    assert!(
                        error
                            .to_string()
                            .starts_with(&format!("{value:?} is not a media type,")),
                        "{value:?} raised another refusal: {error}"
                    );
                }
            }
        }
    }
}
