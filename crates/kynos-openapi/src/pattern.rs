//! A Schema Object's `pattern`, an ECMA-262 regular expression, as the
//! [`regex`](https://docs.rs/regex) engine reads it.
//!
//! JSON Schema reads `pattern` in the ECMA-262 dialect with Unicode support,
//! and the engine reads its own. The two agree on most of the syntax and part
//! of the meaning, so a pattern is translated rather than handed over:
//!
//! - Where both read a construct and give it different meanings, it is
//!   rewritten to the ECMA-262 one. `\d`, `\w` and `\b` are ASCII in ECMA-262
//!   and Unicode-aware in the engine; `\s` and `.` differ in which characters
//!   they count as space and as a line terminator.
//! - Where the engine reads a construct ECMA-262 refuses or reads otherwise —
//!   inline flags, `\A`, `\z`, `(?P<name>`, a group name that is no
//!   identifier, a quantified quantifier or assertion, a POSIX class, a nested
//!   class, a leading `]` in a class, a set operation, or a Unicode property
//!   spelled other than as ECMA-262 spells it (`\p{greek}`, `\p{Greek}` for a
//!   script) — it is refused, since the document and the check would disagree
//!   about it.
//! - Where ECMA-262 reads a construct the engine cannot — lookaround and
//!   backreferences, which need a backtracking search — it is refused, since
//!   no check could enforce it.
//! - Where ECMA-262 reads an escape the engine does not parse, it is rewritten
//!   when it stands alone (`\0`, NUL) and otherwise refused by its name with
//!   the escape to write instead (`[\b]`, `\cX`).
//!
//! A pattern [`translate`] returns has been compiled, so the engine runs it.
//! Kynos translates a derived field's pattern when the derive expands, and a
//! map key's when the router is built.

use regex_syntax::ast::{
    self, Assertion, AssertionKind, Ast, ClassPerl, ClassPerlKind, ClassSetBinaryOp, ClassSetItem,
    ClassUnicode, ClassUnicodeKind, ClassUnicodeOpKind, ErrorKind, GroupKind, HexLiteralKind,
    Literal, LiteralKind, Span, SpecialLiteralKind, parse::ParserBuilder,
};

use std::sync::LazyLock;

/// `\d`: the ASCII digits.
const DIGIT: &str = "[0-9]";
const NOT_DIGIT: &str = "[^0-9]";
/// `\w`: the ASCII letters and digits, and `_`.
const WORD: &str = "[0-9A-Za-z_]";
const NOT_WORD: &str = "[^0-9A-Za-z_]";
/// `\s`: ECMA-262's `WhiteSpace` and `LineTerminator`, which is not Unicode's
/// `White_Space`: it holds U+FEFF and leaves out U+0085.
const SPACE: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
const NOT_SPACE: &str = r"[^\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// `.`: any character but ECMA-262's four line terminators.
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
/// `\b` and `\B`, between ASCII word characters.
const WORD_BOUNDARY: &str = r"(?-u:\b)";
const NOT_WORD_BOUNDARY: &str = r"(?-u:\B)";
/// `\0` with no digit after it: NUL, which the engine reads only as octal.
const NUL: &str = r"\x00";

/// Why lookaround and backreferences are refused.
const BACKTRACKING: &str = "lookaround and backreferences are ECMA-262, but the `regex` engine \
    the check runs with does not implement them: both need a backtracking search, and it matches \
    in time linear in the input instead. Express the bound without them, or check it in the \
    handler and describe it in prose";

/// An ECMA-262 group name: an identifier. The engine's names are a letter or
/// `_` and then letters, digits, `_`, `.`, `[` and `]`, so this refuses what
/// the engine reads and ECMA-262 does not; its `$` and joiners never reach it.
static GROUP_NAME: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^[\p{ID_Start}_]\p{ID_Continue}*$").expect("a valid pattern")
});

/// What ECMA-262 lets a backslash escape to the character itself, outside a
/// class: its syntax characters and `/`. Inside one, `-` as well.
const IDENTITY_ESCAPES: &str = r"^$\.*+?()[]{}|/";

/// The spellings ECMA-262 reads in `\p{...}`, each value on a line beside its
/// aliases: its tables of `General_Category` values and of binary properties,
/// and the Script values of the Unicode version the engine carries (16.0),
/// which `Script_Extensions` shares.
const GENERAL_CATEGORY: &str = include_str!("pattern/general_category.txt");
const BINARY_PROPERTY: &str = include_str!("pattern/binary_property.txt");
const SCRIPT: &str = include_str!("pattern/script.txt");

/// A `pattern` that no check could enforce as ECMA-262 reads it.
///
/// It says why, and what to write instead where there is an equivalent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct UntranslatablePattern {
    reason: String,
}

/// `source`, an ECMA-262 pattern, as the pattern the `regex` engine runs.
///
/// The translation is unanchored, as JSON Schema's `pattern` is: it matches a
/// string holding a match anywhere.
///
/// # Errors
///
/// [`UntranslatablePattern`] where `source` is no regular expression, writes a
/// construct the engine cannot run (lookaround, backreferences) or an escape
/// it does not parse (`[\b]`, `\cX`), writes one the two dialects read
/// apart, or translates to a pattern the engine cannot compile.
pub fn translate(source: &str) -> Result<String, UntranslatablePattern> {
    translated(source).map_err(|reason| UntranslatablePattern { reason })
}

/// [`translate`], refusing with the reason alone.
fn translated(source: &str) -> Result<String, String> {
    // Octal makes `\0` a literal the translator rewrites, and `\1` one it
    // refuses as the backreference ECMA-262 reads it as; no octal escape
    // reaches the engine.
    let ast = ParserBuilder::new()
        .octal(true)
        .build()
        .parse(source)
        .map_err(|error| unreadable(&error))?;
    let mut replacements = ast::visit(
        &ast,
        Translator {
            source,
            replacements: Vec::new(),
        },
    )?;
    replacements.sort_by_key(|(span, _)| span.start.offset);

    let mut translated = String::with_capacity(source.len());
    let mut copied = 0;
    for (span, replacement) in replacements {
        translated.push_str(&source[copied..span.start.offset]);
        translated.push_str(replacement);
        copied = span.end.offset;
    }
    translated.push_str(&source[copied..]);

    regex::Regex::new(&translated)
        .map_err(|error| format!("the `regex` engine cannot compile this pattern: {error}"))?;
    Ok(translated)
}

/// Why the parser refused its pattern, naming an escape ECMA-262 reads and
/// the parser does not.
fn unreadable(error: &ast::Error) -> String {
    let pattern = error.pattern();
    let span = error.span();
    let written = &pattern[span.start.offset..span.end.offset];
    match error.kind() {
        ErrorKind::UnsupportedLookAround => BACKTRACKING.to_owned(),
        // With octal on, the parser reads `\1` to `\7` as octal and refuses
        // `\8` and `\9`, which ECMA-262 reads as backreferences too.
        ErrorKind::EscapeUnrecognized if matches!(written, r"\8" | r"\9") => {
            BACKTRACKING.to_owned()
        }
        ErrorKind::ClassEscapeInvalid if written == r"\b" => {
            "this pattern writes `[\\b]`, which ECMA-262 reads as U+0008 BACKSPACE and the \
             `regex` engine refuses; write `\\x08`"
                .to_owned()
        }
        ErrorKind::EscapeUnrecognized if written == r"\c" => {
            match pattern[span.end.offset..].chars().next() {
                Some(letter) if letter.is_ascii_alphabetic() => format!(
                    "this pattern writes `\\c{letter}`, ECMA-262's control escape, which the \
                     `regex` engine does not read; write `\\x{:02X}`",
                    u32::from(letter) % 32
                ),
                _ => format!(
                    "this is not a regular expression: `\\c` takes an ASCII letter, at byte {}",
                    span.start.offset
                ),
            }
        }
        kind => format!(
            "this is not a regular expression: {kind}, at byte {}",
            error.span().start.offset
        ),
    }
}

/// Collects each rewrite, in source order, and refuses what the two dialects
/// read differently.
struct Translator<'a> {
    /// The pattern the visited AST was parsed from.
    source: &'a str,
    replacements: Vec<(Span, &'static str)>,
}

impl ast::Visitor for Translator<'_> {
    type Output = Vec<(Span, &'static str)>;
    type Err = String;

    fn finish(self) -> Result<Self::Output, String> {
        Ok(self.replacements)
    }

    fn visit_pre(&mut self, ast: &Ast) -> Result<(), String> {
        match ast {
            Ast::Empty(_) | Ast::Alternation(_) | Ast::Concat(_) | Ast::ClassBracketed(_) => Ok(()),
            // ECMA-262 with Unicode support quantifies only an atom, so neither
            // a quantifier nor an assertion can be quantified.
            Ast::Repetition(repetition) => match &*repetition.ast {
                Ast::Repetition(_) => Err(not_ecma(
                    "a quantifier on a quantifier, such as `a**` or `x{2}{3}`, which ECMA-262 \
                     refuses; group the inner one as `(?:x{2}){3}`",
                )),
                Ast::Assertion(_) => Err(not_ecma(
                    "a quantified assertion, such as `^*` or `\\b+`, which ECMA-262 refuses",
                )),
                _ => Ok(()),
            },
            Ast::Flags(_) => Err(not_ecma("an inline flag group such as `(?i)`")),
            Ast::Literal(literal) => self.literal(literal, false),
            Ast::Dot(span) => {
                self.replacements.push((**span, DOT));
                Ok(())
            }
            Ast::Assertion(assertion) => self.assertion(assertion),
            Ast::ClassUnicode(class) => unicode_class(class),
            Ast::ClassPerl(class) => {
                self.perl(class);
                Ok(())
            }
            Ast::Group(group) => match &group.kind {
                GroupKind::CaptureIndex(_) => Ok(()),
                GroupKind::CaptureName {
                    starts_with_p: false,
                    name,
                } => {
                    if GROUP_NAME.is_match(&name.name) {
                        Ok(())
                    } else {
                        Err(not_ecma(
                            "a group name that is not an ECMA-262 identifier, such as \
                             `(?<a.b>...)`; it starts with a letter or `_` and holds no `.`, \
                             `[` or `]`",
                        ))
                    }
                }
                GroupKind::CaptureName {
                    starts_with_p: true,
                    ..
                } => Err(not_ecma(
                    "`(?P<name>...)`; ECMA-262 writes a named group `(?<name>...)`",
                )),
                GroupKind::NonCapturing(flags) if flags.items.is_empty() => Ok(()),
                GroupKind::NonCapturing(_) => {
                    Err(not_ecma("a group setting flags, such as `(?i:...)`"))
                }
            },
        }
    }

    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), String> {
        match item {
            ClassSetItem::Empty(_) | ClassSetItem::Union(_) => Ok(()),
            ClassSetItem::Literal(literal) => self.literal(literal, true),
            ClassSetItem::Range(range) => {
                self.literal(&range.start, true)?;
                self.literal(&range.end, true)
            }
            ClassSetItem::Ascii(_) => Err(not_ecma(
                "a POSIX class such as `[[:alpha:]]`, which ECMA-262 reads as a class of the \
                 characters `[`, `:` and the letters, followed by `]`",
            )),
            ClassSetItem::Unicode(class) => unicode_class(class),
            ClassSetItem::Perl(class) => {
                self.perl(class);
                Ok(())
            }
            ClassSetItem::Bracketed(_) => Err(not_ecma(
                "a class nested in a class; ECMA-262 reads `[` inside a class as the character \
                 itself, so escape it as `\\[`",
            )),
        }
    }

    fn visit_class_set_binary_op_pre(&mut self, _: &ClassSetBinaryOp) -> Result<(), String> {
        Err(not_ecma(
            "a class set operation (`&&`, `--` or `~~`); ECMA-262 reads each as two characters",
        ))
    }
}

impl Translator<'_> {
    /// Rewrites `\0`, and refuses the other octal escapes and what
    /// [`literal_escape`] refuses.
    fn literal(&mut self, literal: &Literal, in_class: bool) -> Result<(), String> {
        if literal.kind != LiteralKind::Octal {
            return literal_escape(literal, in_class);
        }
        let escape = &self.source[literal.span.start.offset..literal.span.end.offset];
        let digit_follows =
            self.source[literal.span.end.offset..].starts_with(|c: char| c.is_ascii_digit());
        if escape == r"\0" && !digit_follows {
            self.replacements.push((literal.span, NUL));
            Ok(())
        } else if escape.starts_with(r"\0") {
            Err(not_ecma(
                "`\\0` followed by a digit, such as `\\01`, which ECMA-262 refuses; write NUL \
                 as `\\x00` and the digit after it",
            ))
        } else if in_class {
            Err(not_ecma(
                "a decimal escape such as `\\1` in a class, which ECMA-262 refuses; write the \
                 character as `\\u{...}`",
            ))
        } else {
            Err(BACKTRACKING.to_owned())
        }
    }

    fn perl(&mut self, class: &ClassPerl) {
        let replacement = match (&class.kind, class.negated) {
            (ClassPerlKind::Digit, false) => DIGIT,
            (ClassPerlKind::Digit, true) => NOT_DIGIT,
            (ClassPerlKind::Word, false) => WORD,
            (ClassPerlKind::Word, true) => NOT_WORD,
            (ClassPerlKind::Space, false) => SPACE,
            (ClassPerlKind::Space, true) => NOT_SPACE,
        };
        self.replacements.push((class.span, replacement));
    }

    fn assertion(&mut self, assertion: &Assertion) -> Result<(), String> {
        let replacement = match assertion.kind {
            // Without the `m` flag, which ECMA-262 sets nowhere here, both
            // read `^` and `$` as the ends of the input.
            AssertionKind::StartLine | AssertionKind::EndLine => return Ok(()),
            AssertionKind::WordBoundary => WORD_BOUNDARY,
            AssertionKind::NotWordBoundary => NOT_WORD_BOUNDARY,
            AssertionKind::StartText
            | AssertionKind::EndText
            | AssertionKind::WordBoundaryStart
            | AssertionKind::WordBoundaryEnd
            | AssertionKind::WordBoundaryStartAngle
            | AssertionKind::WordBoundaryEndAngle
            | AssertionKind::WordBoundaryStartHalf
            | AssertionKind::WordBoundaryEndHalf => {
                return Err(not_ecma(
                    "an assertion such as `\\A`, `\\z` or `\\b{start}`; ECMA-262 writes the ends \
                     of the input as `^` and `$`",
                ));
            }
        };
        self.replacements.push((assertion.span, replacement));
        Ok(())
    }
}

/// Refuses an escape ECMA-262 does not read as the engine does.
///
/// `in_class` admits `\-`, which ECMA-262 allows only inside a class.
fn literal_escape(literal: &Literal, in_class: bool) -> Result<(), String> {
    match &literal.kind {
        LiteralKind::Verbatim => {
            // A bare `]` inside a class was its first character to the engine;
            // ECMA-262 reads `[]` as the empty class.
            if in_class && literal.c == ']' {
                Err(not_ecma(
                    "a `]` first in a class, such as `[]a]`; ECMA-262 reads `[]` as the empty \
                     class, so escape it as `\\]`",
                ))
            // ECMA-262 refuses a bare `]`, `{` or `}` outside a class under
            // Unicode support, and the engine reads one as the character.
            } else if !in_class && matches!(literal.c, ']' | '{' | '}') {
                Err(not_ecma(&format!(
                    "an unescaped `{}`, which ECMA-262 refuses; write `\\{}`",
                    literal.c, literal.c
                )))
            } else {
                Ok(())
            }
        }
        LiteralKind::Meta | LiteralKind::Superfluous => {
            if IDENTITY_ESCAPES.contains(literal.c) || (in_class && literal.c == '-') {
                Ok(())
            } else {
                Err(not_ecma(&format!(
                    "`\\{}`, an escape ECMA-262 refuses; write the character alone",
                    literal.c
                )))
            }
        }
        LiteralKind::HexFixed(HexLiteralKind::X | HexLiteralKind::UnicodeShort)
        | LiteralKind::HexBrace(HexLiteralKind::UnicodeShort)
        | LiteralKind::Special(
            SpecialLiteralKind::FormFeed
            | SpecialLiteralKind::Tab
            | SpecialLiteralKind::LineFeed
            | SpecialLiteralKind::CarriageReturn
            | SpecialLiteralKind::VerticalTab,
        ) => Ok(()),
        LiteralKind::HexFixed(HexLiteralKind::UnicodeLong)
        | LiteralKind::HexBrace(HexLiteralKind::X | HexLiteralKind::UnicodeLong) => Err(not_ecma(
            "a `\\U` or `\\x{...}` escape; ECMA-262 writes a code point as `\\u{...}`",
        )),
        // `Translator::literal` reads an octal escape before this does.
        LiteralKind::Octal
        | LiteralKind::Special(SpecialLiteralKind::Bell | SpecialLiteralKind::Space) => Err(
            not_ecma("an escape such as `\\a`; write the character as `\\u{...}`"),
        ),
    }
}

/// Refuses a Unicode class ECMA-262 does not write as the engine does.
///
/// The engine matches a property's name and value loosely, ignoring case,
/// spaces, `_` and `-`; ECMA-262 matches them exactly, and reads a lone name only
/// as a `General_Category` value or a binary property.
fn unicode_class(class: &ClassUnicode) -> Result<(), String> {
    let (spelled, written) = match &class.kind {
        ClassUnicodeKind::Named(name) => (
            spells(GENERAL_CATEGORY, name) || spells(BINARY_PROPERTY, name),
            name.clone(),
        ),
        ClassUnicodeKind::NamedValue {
            op: ClassUnicodeOpKind::Equal,
            name,
            value,
        } => (
            match name.as_str() {
                "General_Category" | "gc" => spells(GENERAL_CATEGORY, value),
                "Script" | "sc" | "Script_Extensions" | "scx" => spells(SCRIPT, value),
                _ => false,
            },
            format!("{name}={value}"),
        ),
        ClassUnicodeKind::OneLetter(_) => {
            return Err(not_ecma(
                "a one-letter Unicode class such as `\\pL`; ECMA-262 writes it `\\p{L}`",
            ));
        }
        ClassUnicodeKind::NamedValue { .. } => {
            return Err(not_ecma(
                "a Unicode class written with `:` or `!=`; ECMA-262 writes `\\p{name=value}`, \
                 and `\\P{...}` for its complement",
            ));
        }
    };
    if spelled {
        Ok(())
    } else {
        Err(not_ecma(&format!(
            "`\\{}{{{written}}}`, a property ECMA-262 does not spell. It spells a name and a \
             value exactly as Unicode does, case and `_` included, and reads `\\p{{Lu}}` as a \
             General_Category value or a binary property and `\\p{{Script=Greek}}` as a script",
            if class.negated { 'P' } else { 'p' },
        )))
    }
}

/// Whether `table`, a property's values one per line beside their aliases,
/// spells `name`.
fn spells(table: &str, name: &str) -> bool {
    table
        .split_ascii_whitespace()
        .any(|spelling| spelling == name)
}

/// The refusal of a construct ECMA-262 reads otherwise than the engine.
fn not_ecma(construct: &str) -> String {
    format!(
        "this pattern writes {construct}. `pattern` is an ECMA-262 regular expression, and the \
         check enforcing it reads only what ECMA-262 reads the same way"
    )
}

#[cfg(test)]
mod tests;
