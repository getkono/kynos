//! The `trybuild` UI suite: what a rejected program's diagnostic actually says.
//!
//! Every compile-fail case has a sibling in `ui/pass/` differing in exactly the
//! property under test, per the pass-control rule in `docs/testing.md`. A
//! negative on its own cannot tell "the rule holds" from "the surface is
//! unusable", because `compile_fail` passes for any reason at all.
//!
//! Snapshots are recorded under the toolchain `mise.toml` pins, and under
//! `--all-features`. Both pins matter: rustc rewords diagnostics between
//! releases, and its "the following other types implement" list enumerates
//! implementations that feature flags add and remove. Re-record with
//! `TRYBUILD=overwrite cargo nextest run -p kynos --test ui --all-features`.
//!
//! Cases that could only fail with a feature *off* are therefore not here.
//! [`ui/PENDING.md`](ui/PENDING.md) names them.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[test]
fn ui() {
    let cases = trybuild::TestCases::new();

    cases.compile_fail("tests/ui/macros/*.rs");
    cases.compile_fail("tests/ui/schema/*.rs");
    cases.compile_fail("tests/ui/antipattern/*.rs");
    cases.compile_fail("tests/ui/traits/*.rs");
    cases.pass("tests/ui/pass/*.rs");
}

/// One `ui/schema/` case per row of the rejection table in `kynos::schema`.
///
/// A count, not a mapping: it catches a row added without a case, which is the
/// drift that actually happens, and does not catch a case renamed to cover a
/// different row. Several rows also list more than one type — the row for
/// `serde_json::Value` names `Map` and `RawValue` too — so the case count is a
/// floor on coverage rather than a measure of it. The pass-control rule is
/// enforced by review, not here.
///
/// The table is the specification of which types are deliberately undescribable,
/// and a row nobody wrote a case for is a rule nobody checks. Counting here
/// turns adding a row without adding a case into a test failure.
#[test]
fn every_rejected_schema_type_has_a_case() {
    const SOURCE: &str = include_str!("../src/schema/mod.rs");

    let rows = rejection_table_rows(SOURCE);
    assert!(
        rows > 0,
        "the rejection table in `schema/mod.rs` was not found; this test is looking for the \
         heading `# Types deliberately left without an implementation`"
    );

    let cases = compile_fail_cases(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui/schema"));
    assert_eq!(
        cases, rows,
        "`tests/ui/schema/` holds {cases} case(s) for {rows} row(s) of the rejection table in \
         `schema/mod.rs`; a row added without a case leaves a documented refusal that nothing \
         checks"
    );
}

/// The body rows of the Markdown table under the rejection heading.
fn rejection_table_rows(source: &str) -> usize {
    source
        .lines()
        .map(str::trim_start)
        .skip_while(|line| !line.ends_with("# Types deliberately left without an implementation"))
        .map(|line| line.trim_start_matches("//!").trim())
        // The table ends at the first line that is not part of it.
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        // The header row and the `| --- |` separator describe the table rather
        // than populating it. Dropped by position rather than by content, so a
        // row that happens to contain a dash still counts.
        .skip(2)
        .count()
}

/// The `.rs` files directly inside `directory`.
fn compile_fail_cases(directory: &Path) -> usize {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|it| it == "rs"))
        .count()
}

/// Every trait carrying `#[diagnostic::on_unimplemented]`, against the snapshot
/// that records what it says.
///
/// [`nfr.md`](https://github.com/getkono/kynos/blob/master/docs/nfr.md) marks "no diagnostic names an internal
/// type" enforced by this suite, and eight of the fourteen guided traits had no
/// snapshot at all: `Handler`, `Describe`, `RequestContent`, `Alternative`,
/// `MapKey`, `ShortCircuit`, `EndpointMeta` and `IntoEndpoints`. The attribute
/// is what replaces rustc's generic "the trait bound is not satisfied" with a
/// message naming the fix, so an unsnapshotted one is a message nobody checks.
///
/// An explicit mapping rather than a search for the trait's name, because half
/// the messages deliberately never spell it: `Handler`'s says "is not a Kynos
/// handler", which is the improvement being made and not something to grep for.
/// The mapping is held to the source both ways: its names are the set of
/// guided traits, and each snapshot records its trait's `message`, read from
/// the attribute, so a snapshot of some other diagnostic does not stand in.
#[test]
fn every_guided_diagnostic_has_a_snapshot() {
    /// One row per `#[diagnostic::on_unimplemented]` in `crates/kynos/src`.
    const RECORDED: &[(&str, &str)] = &[
        (
            "AdmitsAny",
            "macros/schema_field_skipped_on_read_beside_open_map.stderr",
        ),
        ("Alternative", "traits/alternative.stderr"),
        ("ByteSource", "traits/byte_source.stderr"),
        ("Carries", "traits/carries.stderr"),
        (
            "ClosedFlatten",
            "macros/schema_flatten_internally_tagged_denying_unknown_fields.stderr",
        ),
        ("Describe", "traits/describe.stderr"),
        ("EndpointMeta", "traits/endpoint_meta.stderr"),
        ("Flatten", "traits/flatten.stderr"),
        ("FromRequest", "antipattern/raw_request_extractor.stderr"),
        (
            "FromRequestParts",
            "antipattern/raw_header_map_extractor.stderr",
        ),
        ("Handler", "traits/handler.stderr"),
        ("IntoEndpoints", "traits/into_endpoints.stderr"),
        ("IntoResponse", "antipattern/bare_status_code.stderr"),
        ("Items", "macros/schema_item_bound_on_a_string.stderr"),
        ("Languages", "traits/languages.stderr"),
        ("MapKey", "traits/map_key.stderr"),
        ("Numeric", "macros/schema_numeric_bound_on_a_string.stderr"),
        ("OpenMap", "traits/open_map.stderr"),
        ("ParamValue", "macros/query_params_object_field.stderr"),
        ("Provides", "antipattern/inject_without_provider.stderr"),
        ("Rangeable", "traits/rangeable.stderr"),
        ("RequestContent", "traits/request_content.stderr"),
        ("Responses", "antipattern/problem_as_return_type.stderr"),
        ("Schema", "schema/serde_json_value.stderr"),
        ("ShortCircuit", "traits/short_circuit.stderr"),
        ("Textual", "macros/schema_length_bound_on_a_number.stderr"),
        (
            "UniqueItems",
            "macros/schema_unique_items_on_a_string.stderr",
        ),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut guided = BTreeMap::new();
    guided_traits_in_source(&root.join("src"), &mut guided);

    let recorded: BTreeSet<&str> = RECORDED.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        recorded,
        guided.keys().map(String::as_str).collect(),
        "the traits `crates/kynos/src` guides are not the ones with a snapshot; a diagnostic \
         nobody snapshotted is a diagnostic nobody checked"
    );

    for (trait_name, snapshot) in RECORDED {
        let path = root.join("tests/ui").join(snapshot);
        let recorded = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("`{trait_name}` names {snapshot}, which is not there"));
        let message = &guided[*trait_name];
        assert!(
            renders(message, &recorded),
            "{snapshot} does not record `{trait_name}`'s message {message:?}"
        );
    }
}

/// Every `#[diagnostic::on_unimplemented]` under `directory`, by the trait it
/// guides, with its `message` as written.
fn guided_traits_in_source(directory: &Path, found: &mut BTreeMap<String, String>) {
    const ATTRIBUTE: &str = "#[diagnostic::on_unimplemented(";

    for entry in fs::read_dir(directory).expect("the crate's own source is readable") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            guided_traits_in_source(&path, found);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("readable source");
        for (at, _) in source.match_indices(ATTRIBUTE) {
            let attribute = &source[at + ATTRIBUTE.len()..];
            let message = attribute
                .split_once("message = \"")
                .and_then(|(_, rest)| string_literal(rest))
                .unwrap_or_else(|| panic!("{}: an attribute with no `message`", path.display()));
            let name: String = attribute
                .split_once("trait ")
                .unwrap_or_else(|| panic!("{}: an attribute on no trait", path.display()))
                .1
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            let previous = found.insert(name.clone(), message);
            assert!(previous.is_none(), "`{name}` is guided twice");
        }
    }
}

/// The value of the string literal whose opening quote precedes `text`, with
/// its escapes and line continuations resolved as rustc resolves them.
fn string_literal(text: &str) -> Option<String> {
    let mut value = String::new();
    let mut characters = text.chars().peekable();
    loop {
        match characters.next()? {
            '"' => return Some(value),
            '\\' => match characters.next()? {
                '\n' => while characters.next_if(|next| next.is_whitespace()).is_some() {},
                'n' => value.push('\n'),
                't' => value.push('\t'),
                '0' => value.push('\0'),
                'u' => {
                    let digits: String = characters
                        .by_ref()
                        .skip(1)
                        .take_while(|next| *next != '}')
                        .collect();
                    let code = u32::from_str_radix(&digits, 16).ok()?;
                    value.push(char::from_u32(code)?);
                }
                escaped => value.push(escaped),
            },
            character => value.push(character),
        }
    }
}

/// Whether `snapshot` records `message`, each `{Parameter}` it names standing
/// for whatever type the snapshot's program put there.
fn renders(message: &str, snapshot: &str) -> bool {
    let mut rest = snapshot;
    for (index, fixed) in message.split(['{', '}']).enumerate() {
        // Odd pieces are the parameters' names.
        if index % 2 == 1 || fixed.is_empty() {
            continue;
        }
        let Some(at) = rest.find(fixed) else {
            return false;
        };
        rest = &rest[at + fixed.len()..];
    }
    true
}

/// Every row of every `kynos-macros` diagnostic ledger, against a `ui/macros/`
/// snapshot recording what it says.
///
/// A ledger row asserts *which* rule fired by a fragment of its message, and
/// the macro crate counts its rows against the diagnostic sites of the files
/// they cover; what the rule *says* is only seen rendered here. So each row's
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

/// Every `Router` and `Group` builder carries all four type parameters across.
///
/// Both types are parameterised by `<C, P, I, S>`, and two of those are the
/// phantom lists the compile-time conflict check reads: `I`, the interceptors
/// mounted on this scope, and `S`, what the scopes mounted here brought with
/// them. A builder naming fewer than four in its return type resets the ones it
/// omits to their default of `()`, and the check then compares a newcomer
/// against an empty list and finds nothing to collide with.
///
/// This has gone wrong twice. `catch_panics` returned `Router<C, Catch>` and
/// dropped `I`. That fix landed before `S` existed, and the commit adding `S`
/// updated the group's half and not the router's, so it returned
/// `Router<C, Catch, I>` and dropped `S` instead. Both were silent by
/// construction: the signature is well-formed, every caller still compiles, and
/// only a program that *should* have been refused shows the difference.
///
/// A count of the arguments rather than a mapping of the methods, for the
/// reason `every_rejected_schema_type_has_a_case` gives: it catches the drift
/// that actually happens -- a parameter dropped from a return type -- and does
/// not pretend to check that the parameter carried is the right one. Methods
/// returning `Self` need no case: every `impl` block declaring one is
/// `impl<C, P, I, S>`, so `Self` is all four by construction.
#[test]
fn every_builder_preserves_the_type_parameters() {
    const PARAMETERS: usize = 4;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;

    for relative in ["src/router/mod.rs", "src/router/group.rs"] {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).expect("the crate's own source is readable");

        for (number, line) in source.lines().enumerate() {
            // Comments are skipped so a rustdoc paragraph naming a return type
            // -- this file's own prose does -- is not read as one.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }

            for type_name in ["Router", "Group"] {
                let opening = format!("-> {type_name}<");
                let Some(at) = line.find(&opening) else {
                    continue;
                };

                let arguments = &line[at + opening.len()..];
                let count = top_level_arguments(arguments).unwrap_or_else(|| {
                    panic!(
                        "{relative}:{} returns `{type_name}<` whose argument list does not close \
                         on one line; this test reads return types a line at a time",
                        number + 1
                    )
                });

                assert_eq!(
                    count,
                    PARAMETERS,
                    "{relative}:{} returns `{type_name}<` with {count} type argument(s) and \
                     `{type_name}` has {PARAMETERS}; the omitted one falls back to its default of \
                     `()`, which silently empties a phantom list the conflict check reads. Name \
                     all {PARAMETERS}, or return `Self`.",
                    number + 1
                );

                checked += 1;
            }
        }
    }

    assert!(
        checked > 0,
        "no `-> Router<` or `-> Group<` return type was found; this test is looking in \
         `src/router/mod.rs` and `src/router/group.rs`, and finding none means it has stopped \
         checking anything"
    );
}

/// The comma-separated arguments of a generic list whose opening `<` is already
/// consumed, or `None` when it does not close in `text`.
///
/// Depth-aware, so `Router<C, P, I, <E::Stacks as Flatten<S>>::Out>` counts
/// four rather than five: only a comma directly inside the outer list
/// separates an argument.
fn top_level_arguments(text: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut separators = 0;

    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(separators + 1);
                }
            }
            ',' if depth == 1 => separators += 1,
            _ => {}
        }
    }

    None
}
