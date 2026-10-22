//! Counts held against `kynos-macros`, which only this repository can make.
//!
//! Every other target here asserts something about `kynos` alone. These four
//! read the macro crate's source for what it declares, and compare that against
//! a set the suites next door witness — so a macro added without a witness
//! fails the build rather than expanding to whatever it likes, and a refusal
//! the `Schema` derive documents, or a diagnostic its ledgers assert, without a
//! snapshot fails it rather than going unchecked.
//!
//! That is why they are not in the files whose witnesses they count.
//! `crates/kynos/Cargo.toml` keeps this target out of the published archive:
//! `cargo package` carries a package directory and nothing beside it, so
//! `../../kynos-macros/` does not exist in a tarball and an assertion written
//! against it could only fail there. The claim is a property of the workspace,
//! and the workspace is where it is checked.

#[path = "support/literal.rs"]
mod literal;

use std::{collections::BTreeSet, fs, path::Path};

use literal::string_literal;

/// The macro crate's entry points, as text.
fn macro_crate() -> String {
    fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kynos-macros/src/lib.rs"
    ))
    .expect("the macro crate's entry points are readable from the workspace")
}

/// The derives, counted against the entry points that declare them.
///
/// `every_derive_implements_its_trait` in [`derives.rs`](derives.rs) witnesses
/// a set someone chose, and nothing tied that set to the macros the crate
/// actually exports. A derive added without a witness is one that could expand
/// to anything — and eight witnesses against ten entry points is the state this
/// test was written to end.
///
/// A count rather than a mapping: it catches a derive added without a witness,
/// and not a witness renamed to cover a different one.
#[test]
fn every_derive_has_a_witness() {
    /// Every derive witnessed in `derives.rs`. `Provider` is exercised by
    /// `the_provider_derive_supplies_every_field_it_was_not_told_to_skip`,
    /// `ApiError` and `Reply` through `implements_responses`,
    /// `MultipartForm` through `implements_multipart`, and the rest by
    /// `every_derive_implements_its_trait`.
    const WITNESSED: &[&str] = &[
        "ApiError",
        "CookieParams",
        "HeaderParams",
        "MultipartForm",
        "ParamValue",
        "PathParams",
        "Provider",
        "QueryParams",
        "Reply",
        "Schema",
        "SecurityScheme",
        "Tag",
    ];

    let declared = macro_crate().matches("#[proc_macro_derive(").count();
    assert_eq!(
        declared,
        WITNESSED.len(),
        "`kynos-macros` declares {declared} derive(s) and {} are witnessed; a derive added \
         without one is a derive nothing asks to implement its trait",
        WITNESSED.len()
    );
}

/// Each refusal the `Schema` derive's rustdoc lists, as a fragment only its own
/// entry carries and the snapshot of its refusal under `tests/ui/`.
///
/// Outside [`every_rejected_schema_attribute_has_a_case`], which holds the list
/// to it, so that the test stays within the length Clippy accepts.
const RECORDED: &[(&str, &str)] = &[
    (
        "`#[serde(with = ...)]`",
        "macros/schema_serialize_with.stderr",
    ),
    ("`#[serde(untagged)]`", "macros/schema_untagged_enum.stderr"),
    ("`#[serde(flatten)]`", "macros/schema_flatten_map.stderr"),
    (
        "`#[serde(other)]`",
        "macros/schema_catch_all_variant.stderr",
    ),
    (
        "`skip_serializing_if` on a non-`Option` field",
        "macros/schema_skip_serializing_if_without_default.stderr",
    ),
    (
        "`#[serde(into = ...)]`",
        "macros/schema_container_conversion.stderr",
    ),
    (
        "`#[serde(transparent)]`",
        "macros/schema_transparent_without_one_field.stderr",
    ),
    (
        "`skip_deserializing` alone",
        "macros/schema_tuple_member_skipped_one_way.stderr",
    ),
    (
        "its tag alone",
        "macros/schema_adjacent_skipped_payload.stderr",
    ),
    (
        "`#[serde(skip_deserializing)]` alone on a variant",
        "macros/schema_variant_skipped_on_read.stderr",
    ),
    (
        "`#[serde(skip_deserializing)]` without `skip_serializing`",
        "macros/schema_field_skipped_on_read_denying_unknown_fields.stderr",
    ),
    (
        "`#[schema(open)]` field beside `#[serde(deny_unknown_fields)]`",
        "macros/schema_open_map_denying_unknown_fields.stderr",
    ),
    (
        "A variant whose own name",
        "macros/schema_variant_name_collision.stderr",
    ),
    (
        "A flattened internally tagged enum",
        "macros/schema_flatten_internally_tagged_denying_unknown_fields.stderr",
    ),
    (
        "on a struct carrying `deny_unknown_fields`",
        "macros/schema_tagged_struct_denying_unknown_fields.stderr",
    ),
    (
        "under the name of its struct's own",
        "macros/schema_tagged_struct_field_named_as_tag.stderr",
    ),
    (
        "split `rename(serialize = ..., deserialize = ...)` whose two sides differ",
        "macros/schema_split_rename.stderr",
    ),
    (
        "split container `rename_all(serialize = ..., deserialize = ...)` whose two sides differ",
        "macros/schema_split_rename_all.stderr",
    ),
];

/// The attributes the `Schema` derive's rustdoc lists as refused, each named
/// against the snapshot that holds its refusal.
///
/// `every_rejected_schema_type_has_a_case` in [`ui.rs`](ui.rs) holds the type
/// table to its cases, and this list had no counterpart: three of its five
/// entries once had nothing behind them. A mapping rather than a count, per
/// "Name the set where the set has names" in `docs/testing.md` — counting
/// `ui/macros/schema_*` would also count grammar rules no entry lists.
#[test]
fn every_rejected_schema_attribute_has_a_case() {
    const HEADING: &str = "# Rejected, because serde and the schema would disagree";

    let entries = listed_entries(&macro_crate(), HEADING);
    assert!(
        !entries.is_empty(),
        "no list was found under `{HEADING}` in `kynos-macros/src/lib.rs`; this test reads the \
         entries between that heading and `#[proc_macro_derive(Schema`"
    );
    assert_eq!(
        entries.len(),
        RECORDED.len(),
        "`Schema` lists {} refused attribute(s) and {} are recorded; an entry added without a \
         snapshot is a refusal nothing checks: {entries:#?}",
        entries.len(),
        RECORDED.len()
    );

    // Every entry names exactly one recorded refusal. Counting entries alone
    // lets an unbacked entry stand in for a deleted one whose fragment another
    // entry now mentions in passing.
    for entry in &entries {
        let recorded = RECORDED
            .iter()
            .filter(|(fragment, _)| entry.contains(fragment))
            .count();
        assert_eq!(
            recorded, 1,
            "an entry under `{HEADING}` names {recorded} recorded refusal(s), and each names \
             exactly one: {entry:?}"
        );
    }

    for (fragment, snapshot) in RECORDED {
        let naming = entries
            .iter()
            .filter(|entry| entry.contains(fragment))
            .count();
        assert_eq!(
            naming, 1,
            "{naming} entries under `{HEADING}` name {fragment}, and a recorded fragment names \
             exactly one: {entries:#?}"
        );

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui")
            .join(snapshot);
        let recorded = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !recorded.trim().is_empty(),
            "{fragment} is listed as refused, and `{}` holds no snapshot of the refusal",
            path.display()
        );
    }
}

/// The entries of the rustdoc list under `heading` in the `Schema` derive's
/// documentation, each joined across its continuation lines.
fn listed_entries(source: &str, heading: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let list = source
        .lines()
        .map(str::trim_start)
        .skip_while(|line| !line.ends_with(heading))
        .skip(1)
        .take_while(|line| !line.starts_with("#[proc_macro_derive(Schema"));

    for line in list {
        if let Some(entry) = line.strip_prefix("/// - ") {
            entries.push(entry.to_owned());
        } else if let (Some(continued), Some(entry)) =
            (line.strip_prefix("///   "), entries.last_mut())
        {
            entry.push(' ');
            entry.push_str(continued.trim());
        }
    }
    entries
}

/// The route attributes, counted against the entry points that declare them.
///
/// The cases are in [`pipeline.rs`](pipeline.rs). Under `openapi32`, because
/// `query` is gated there and the full set only exists in that build — which is
/// the one `mise run test` uses.
#[cfg(feature = "openapi32")]
#[test]
fn every_route_attribute_has_a_case() {
    /// The eight ungated attributes, `query`, and `operation`.
    const WITNESSED: usize = 10;

    let declared = macro_crate().matches("#[proc_macro_attribute]").count();
    assert_eq!(
        declared, WITNESSED,
        "`kynos-macros` declares {declared} attribute(s) and {WITNESSED} are witnessed; an \
         attribute added without a case is one whose method nothing reads"
    );
}

/// Every row of every `kynos-macros` diagnostic ledger, against a `ui/macros/`
/// snapshot recording what it says.
///
/// A ledger row asserts *which* rule fired by a fragment of its message, and
/// the macro crate counts its rows against the diagnostic sites of the files
/// they cover; what the rule *says* is only seen rendered in the
/// [`ui.rs`](ui.rs) suite's snapshots. So each row's
/// fragment must appear in some snapshot, or the message it identifies is one
/// no reader has seen. A mapping by text rather than a count, since a count
/// cannot tell a snapshot of this rule from one of its neighbour's.
///
/// A row built only with a feature off — under `#[cfg(not(..))]` — is not
/// held: the snapshots record an `--all-features` build, which never reaches
/// it, for the reason [`ui/PENDING.md`](ui/PENDING.md) gives. Rows sharing a
/// fragment share a snapshot, since the fragment is all a row says about the
/// message.
#[test]
fn every_ledgered_diagnostic_has_a_snapshot() {
    /// The rows no case here can reach, by fragment, each with the reason.
    ///
    /// An `assets!` walk needs a directory, and a case is compiled from a
    /// project `trybuild` generates under the target directory, which
    /// `cargo llvm-cov` moves; no committed `dir` names the same directory
    /// from both. The two unreadable-directory rows also go on in the
    /// operating system's own words.
    const UNREACHABLE: &[(&str, &str)] = &[
        (
            "requires `panic = \"unwind\"`",
            "emitted under `panic = \"abort\"` alone, and the suite builds with unwinding",
        ),
        (
            "`no-such-directory` could not be read: ",
            "an `assets!` walk",
        ),
        ("`{..}` could not be read: ", "an `assets!` walk"),
        (
            "`{..}` holds no servable file; an asset set that serves nothing is one no route needs",
            "an `assets!` walk",
        ),
        ("`{..}` nests deeper than 1 levels", "an `assets!` walk"),
        ("`{..}` holds more than 2 files", "an `assets!` walk"),
        (
            "`nested/a b.txt` has a name no path template can express, so it cannot be \
             described; rename it, or add it to `exclude`",
            "an `assets!` walk",
        ),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let macros = root.join("../kynos-macros/src");
    let mut ledgers = Vec::new();
    ledger_files(&macros, &mut ledgers);
    let rows: Vec<(String, Vec<String>)> = ledgers
        .iter()
        .flat_map(|path| {
            let source = fs::read_to_string(path).expect("the macro crate's source is readable");
            let file = path
                .strip_prefix(&macros)
                .expect("walked from the root")
                .display()
                .to_string();
            ledger_rows(&source)
                .into_iter()
                .map(move |fragment| (file.clone(), fragment))
        })
        .collect();

    let directory = root.join("tests/ui/macros");
    let snapshots: Vec<String> = fs::read_dir(&directory)
        .expect("`ui/macros/` is readable")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|it| it == "stderr"))
        .map(|path| fs::read_to_string(path).expect("a readable snapshot"))
        .collect();

    let fragment = |pieces: &[String]| pieces.join("{..}");
    for (unreachable, _) in UNREACHABLE {
        assert!(
            rows.iter()
                .any(|(_, pieces)| fragment(pieces) == *unreachable),
            "{unreachable:?} is listed as unreachable and no ledger row says it any more"
        );
    }

    let unrecorded: BTreeSet<String> = rows
        .iter()
        .filter(|(_, pieces)| {
            !UNREACHABLE
                .iter()
                .any(|(unreachable, _)| fragment(pieces) == *unreachable)
                && !snapshots
                    .iter()
                    .any(|snapshot| renders_pieces(pieces, snapshot))
        })
        .map(|(file, pieces)| format!("{file}: {:?}", fragment(pieces)))
        .collect();
    assert!(
        unrecorded.is_empty(),
        "{} ledgered diagnostic(s) have no `ui/macros/` snapshot saying them; add a case whose \
         `.stderr` records each:\n{}",
        unrecorded.len(),
        unrecorded.into_iter().collect::<Vec<_>>().join("\n")
    );

    // The control: the walk read the ledgers this crate is known to keep,
    // which held 172 rows when this was written.
    assert!(
        rows.len() > 150,
        "the sweep read only {} ledger row(s) from {} file(s)",
        rows.len(),
        ledgers.len()
    );
}

/// Every `tests.rs` under `directory`, where the macro crate keeps its ledgers.
fn ledger_files(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).expect("the macro crate's source is readable") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            ledger_files(&path, found);
        } else if path.file_name().is_some_and(|name| name == "tests.rs") {
            found.push(path);
        }
    }
}

/// The fragment of each row of each ledger in `source`, as the pieces a
/// rendered message holds in order.
///
/// A ledger is a function whose name ends in `ledger` or `cases`, and its rows
/// are the elements of the `vec![..]` it builds and of each `.push(..)` it
/// makes: a `case(..)` call or a tuple, whose last argument is the fragment.
fn ledger_rows(source: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut at = 0;
    while at < source.len() {
        if let Some(end) = opaque_end(source, at) {
            at = end;
            continue;
        }
        let rest = &source[at..];
        if rest.starts_with("fn ") && !ident_before(source, at) {
            let name: String = rest[3..].chars().take_while(|c| is_ident(*c)).collect();
            if (name.ends_with("ledger") || name.ends_with("cases")) && !feature_off(source, at) {
                let open = code_find(source, at, '{').expect("a ledger has a body");
                let close = matching(source, open);
                rows.extend(body_rows(source, open + 1, close));
                at = close;
                continue;
            }
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    rows
}

/// The rows of the ledger whose body spans `start..end`.
fn body_rows(source: &str, start: usize, end: usize) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut at = start;
    while at < end {
        if let Some(after) = opaque_end(source, at) {
            at = after;
            continue;
        }
        let rest = &source[at..];
        // Rows pushed only with a feature off, for the reason a
        // `#[cfg(not(..))]` row is skipped.
        if rest.starts_with("if !cfg!(") {
            let open = code_find(source, at, '{').expect("an `if` has a block");
            at = matching(source, open);
            continue;
        }
        let elements = if rest.starts_with("vec![") {
            let close = matching(source, at + 4);
            let elements = split_top_level(&source[at + 5..close]);
            at = close;
            elements
        } else if rest.starts_with(".push(") {
            let close = matching(source, at + 5);
            let element = vec![&source[at + 6..close]];
            at = close;
            element
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };
        for element in elements {
            // A row built only with a feature off is out of reach here.
            let Some(element) = strip_attributes(element.trim()) else {
                continue;
            };
            if element.is_empty() {
                continue;
            }
            let inner = element
                .strip_prefix("case(")
                .or_else(|| element.strip_prefix('('))
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or_else(|| {
                    panic!("a ledger row that is no `case(..)` or tuple: {element}")
                });
            let arguments = split_top_level(inner);
            let fragment = arguments
                .iter()
                .rev()
                .map(|argument| argument.trim())
                .find(|argument| !argument.is_empty())
                .expect("a row has arguments");
            rows.push(resolve(source, fragment));
        }
    }
    rows
}

/// What a row's fragment expression says, as the pieces a rendering holds.
fn resolve(source: &str, expression: &str) -> Vec<String> {
    if let Some(literal) = expression.strip_prefix('"') {
        return vec![string_literal(literal).expect("a closed string literal")];
    }
    if let Some(owned) = expression.strip_suffix(".to_owned()") {
        return resolve(source, owned);
    }
    if let Some(arguments) = expression.strip_prefix("format!(") {
        let template =
            split_top_level(arguments.strip_suffix(')').expect("a closed `format!`"))[0].trim();
        let template = string_literal(template.strip_prefix('"').expect("a literal template"))
            .expect("a closed string literal");
        return template
            .split(['{', '}'])
            .step_by(2)
            .filter(|piece| !piece.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if expression.chars().all(is_ident) {
        let declaration = format!("const {expression}: &str = \"");
        let (_, literal) = source
            .split_once(&declaration)
            .unwrap_or_else(|| panic!("`{expression}` is no `&str` constant of its file"));
        return vec![string_literal(literal).expect("a closed string literal")];
    }
    panic!("a ledger row whose fragment this sweep cannot read: {expression}")
}

/// Whether `snapshot` holds each of `pieces`, in order.
fn renders_pieces(pieces: &[String], snapshot: &str) -> bool {
    let mut rest = snapshot;
    for piece in pieces {
        let Some(at) = rest.find(piece.as_str()) else {
            return false;
        };
        rest = &rest[at + piece.len()..];
    }
    true
}

/// `element` without the comments and outer attributes leading it, or `None`
/// where one of them is a `#[cfg(not(..))]`.
fn strip_attributes(mut element: &str) -> Option<&str> {
    loop {
        if element.starts_with("#[cfg(not(") {
            return None;
        } else if element.starts_with("#[") {
            let close = matching(element, 1);
            element = element[close + 1..].trim_start();
        } else if element.starts_with("//") {
            element = element[opaque_end(element, 0).expect("a comment")..].trim_start();
        } else {
            return Some(element);
        }
    }
}

/// Whether the item whose `fn` starts at `at` is compiled only with a feature
/// off, by the `#[cfg(not(..))]` among the attributes above it.
fn feature_off(source: &str, at: usize) -> bool {
    source[..at]
        .lines()
        .rev()
        .skip(1)
        .map(str::trim)
        .take_while(|line| line.starts_with("#[") || line.starts_with("//"))
        .any(|line| line.starts_with("#[cfg(not("))
}

fn is_ident(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn ident_before(source: &str, at: usize) -> bool {
    source[..at].chars().next_back().is_some_and(is_ident)
}

/// Where the string, character literal or comment starting at `at` ends, or
/// `None` where none starts there.
fn opaque_end(source: &str, at: usize) -> Option<usize> {
    let rest = &source[at..];
    if rest.starts_with("//") {
        return Some(at + rest.find('\n').unwrap_or(rest.len()));
    }
    if rest.starts_with("/*") {
        return Some(at + rest.find("*/").expect("a closed comment") + 2);
    }
    if let Some(raw) = rest.strip_prefix('r').filter(|_| !ident_before(source, at)) {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        if raw[hashes..].starts_with('"') {
            let closing = format!("\"{}", "#".repeat(hashes));
            let body = 1 + hashes + 1;
            return Some(
                at + body
                    + rest[body..].find(&closing).expect("a closed raw string")
                    + closing.len(),
            );
        }
    }
    if rest.starts_with('"') {
        let mut characters = rest.char_indices().skip(1);
        while let Some((index, character)) = characters.next() {
            match character {
                '\\' => {
                    characters.next();
                }
                '"' => return Some(at + index + 1),
                _ => {}
            }
        }
        panic!("an unclosed string literal");
    }
    if let Some(after) = rest.strip_prefix('\'') {
        // A character literal rather than a lifetime: an escape, or one
        // character and a closing quote.
        if after.starts_with('\\') {
            return Some(at + 1 + after[2..].find('\'').expect("a closed character") + 3);
        }
        let mut characters = after.chars();
        if let (Some(character), Some('\'')) = (characters.next(), characters.next()) {
            return Some(at + 1 + character.len_utf8() + 1);
        }
    }
    None
}

/// The first `target` at or after `from` outside any literal or comment.
fn code_find(source: &str, from: usize, target: char) -> Option<usize> {
    let mut at = from;
    while at < source.len() {
        if let Some(end) = opaque_end(source, at) {
            at = end;
            continue;
        }
        let character = source[at..].chars().next()?;
        if character == target {
            return Some(at);
        }
        at += character.len_utf8();
    }
    None
}

/// The bracket closing the one that opens at `open`.
fn matching(source: &str, open: usize) -> usize {
    let mut depth = 0usize;
    let mut at = open;
    while at < source.len() {
        if let Some(end) = opaque_end(source, at) {
            at = end;
            continue;
        }
        let character = source[at..].chars().next().expect("within the source");
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
        at += character.len_utf8();
    }
    panic!("an unclosed bracket")
}

/// `text` split at each comma no bracket, literal or comment encloses.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut at = 0;
    while at < text.len() {
        if let Some(end) = opaque_end(text, at) {
            at = end;
            continue;
        }
        let character = text[at..].chars().next().expect("within the text");
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        at += character.len_utf8();
    }
    pieces.push(&text[start..]);
    pieces
}
