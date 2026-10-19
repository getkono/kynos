//! What a framework failure guarantees to the program that reports it.
//!
//! Kynos ships no error reporter. An application's `main` is where a build
//! failure is rendered, and the crates that do that rendering — `anyhow`,
//! `eyre`, or a plain `Box<dyn Error>` — all take an error through one bound:
//! `std::error::Error + Send + Sync + 'static`. `eyre::Report`'s only
//! conversion is `impl<E: StdError + Send + Sync + 'static> From<E> for Report`,
//! so a failure that cannot satisfy that bound is one no application can `?` out
//! of `main`.
//!
//! Satisfying it is also what makes the cause chain useful: both crates render a
//! failure by walking `source()` recursively, which is why Kynos keeps causes
//! structured rather than formatting them itself.
//!
//! These assertions exist because the property currently holds by *composition*
//! rather than by construction — every payload happens to be built from `String`,
//! `Copy` std types and two std errors that are themselves `Send + Sync`. Since
//! `kynos::Error` is `#[non_exhaustive]`, one future variant holding an
//! unbounded `Box<dyn Error>` would drop the property with nothing else failing.
//!
//! "Every error type" is held to the source rather than to whoever last added a
//! witness: [`every_error_type_kynos_declares_is_accounted_for`] reads each
//! `thiserror::Error` derive and `impl std::error::Error for` out of `src/` and
//! compares the types it finds with the ones witnessed here and the ones
//! [`UNRECEIVABLE`] names.

/// Asserts that `E` can be `?`d into an `anyhow::Error` or an `eyre::Report`.
///
/// The bound is theirs, spelled out rather than imported. Depending on either
/// crate to check it would add a row to the dependency manifest in
/// [`architecture.md`](../../../docs/architecture.md#dependencies) in order to
/// assert a property that belongs to `std`.
fn reportable<E: std::error::Error + Send + Sync + 'static>() {}

/// Declares one `#[test]` per group, each asserting its types reportable, and
/// `witnessed`, the last path segment of every type any group names.
///
/// The entry is derived from the call by `stringify!`, so the two cannot
/// disagree. Every entry is listed whatever this build enabled, since the
/// sweep reads the source rather than the build; only the call is gated.
macro_rules! reportable {
    ($(
        $(#[$test_attribute:meta])*
        fn $test:ident { $($error:ty;)* }
    )*) => {
        $(
            $(#[$test_attribute])*
            #[test]
            fn $test() {
                $(reportable::<$error>();)*
            }
        )*

        /// Every type a group names, by the last segment of its path, sorted.
        fn witnessed() -> Vec<String> {
            let mut names = [$($(stringify!($error),)*)*]
                .into_iter()
                .map(last_segment)
                .collect::<Vec<_>>();
            names.sort();
            names
        }
    };
}

/// The last segment of a type's path, without its generic arguments.
fn last_segment(path: &str) -> String {
    let path = path
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let path = path.split('<').next().unwrap_or_default();
    path.rsplit("::").next().unwrap_or(path).to_owned()
}

/// The error types `src/` declares that no caller can hold, each with the
/// reason it cannot.
const UNRECEIVABLE: &[&str] = &[
    // Private to `middleware::limits`: a stalled body's reader sees it as a
    // failed read, and the interceptor answers the 408 in its place.
    "Stalled",
];

/// A scope set for `ScopedRejection`, which is generic over one.
struct ReadOnly;

#[derive(Clone, Copy)]
enum Grant {
    Read,
}

impl kynos::security::Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::Read => "read",
        }
    }
}

impl kynos::security::auth::Scopes for ReadOnly {
    type Scope = Grant;
    const REQUIRED: &'static [Grant] = &[Grant::Read];
}

reportable! {
    /// The framework's own failure is what every example returns from `main`,
    /// so this is the one that decides whether an application may keep its own
    /// reporter. What it wraps is public in its own right, and a program may
    /// hold one directly.
    fn a_build_failure_is_reportable {
        kynos::Error;
        kynos::router::error::RouterError;
        kynos::schema::registry::SchemaConflict;
        kynos::schema::registry::UnenforceableKeyPattern;
        kynos::middleware::contribution::ContributionConflict;
        kynos::middleware::MiddlewareError;
        kynos::extract::link::UnresolvedLink;
    }

    /// `ServerError` reaches an application through `Error::Server`, but it is
    /// public in its own right and a program may hold one directly.
    #[cfg(feature = "server")]
    fn a_server_failure_is_reportable {
        kynos::server::error::ServerError;
    }

    /// `TlsError` is the one users construct furthest from `main` — the whole
    /// `TlsConfig` builder returns it — so it has the longest way to travel.
    #[cfg(feature = "tls")]
    fn a_tls_failure_is_reportable {
        kynos::server::tls::error::TlsError;
    }

    /// `InvalidNetwork` is returned where a trusted proxy is read from
    /// configuration, which is startup.
    fn a_network_refusal_is_reportable {
        kynos::http::forwarded::InvalidNetwork;
        kynos::http::forwarded::InvalidNetworkEntry;
    }

    /// `InvalidAsset` is returned at startup, where a hand-built asset set is
    /// assembled, so it travels to `main` the way a build failure does.
    #[cfg(feature = "assets")]
    fn an_asset_refusal_is_reportable {
        kynos::router::assets::error::InvalidAsset;
    }

    /// A rejection is a handler's to return or wrap, so it travels wherever
    /// the handler's own error does.
    fn a_rejection_is_reportable {
        kynos::error::rejection::PathRejection;
        kynos::error::rejection::QueryRejection;
        kynos::error::rejection::HeaderRejection;
        kynos::error::rejection::BodyRejection;
        kynos::error::rejection::NegotiationRejection;
        kynos::error::rejection::RangeRejection;
        kynos::error::rejection::AuthRejection;
        kynos::error::rejection::ScopedRejection<ReadOnly>;
    }

    /// The cookie rejection, behind the feature that compiles it.
    #[cfg(feature = "cookie")]
    fn a_cookie_rejection_is_reportable {
        kynos::error::rejection::CookieRejection;
    }

    /// What a constructor or a reader hands back to the code that called it.
    fn a_refused_value_is_reportable {
        kynos::extract::params::header::UnencodableHeader;
        kynos::http::cookie::Unreadable;
        kynos::response::status::InvalidLocation;
        kynos::response::language::tag::TagDefect;
        kynos::response::range::source::Truncated;
        kynos::schema::UnknownVariant;
        kynos::middleware::limits::body_timeout::BodyTimedOut;
    }
}

/// Every `.rs` file under `directory`, recursively, but a sibling `tests.rs`.
fn sources(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).expect("a readable source directory") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            files.extend(sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            files.push(path);
        }
    }
    files
}

/// The error types `source` declares: each type a `thiserror::Error` derive
/// sits on, and each `impl std::error::Error for` names, by its own name.
///
/// Comment lines are dropped first, so a doc example cannot count.
fn declared_errors(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .collect();

    let mut types = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("#[derive(") && line.contains("thiserror::Error") {
            let item = lines[index + 1..]
                .iter()
                .find_map(|line| {
                    let line = line
                        .strip_prefix("pub ")
                        .or_else(|| line.strip_prefix("pub(crate) "))
                        .unwrap_or(line);
                    line.strip_prefix("struct ")
                        .or_else(|| line.strip_prefix("enum "))
                })
                .expect("a derive sits on an item");
            types.push(last_segment(
                item.split(|character: char| !character.is_alphanumeric() && character != '_')
                    .next()
                    .unwrap_or_default(),
            ));
        }
        if let Some(rest) = line
            .split_once("std::error::Error for ")
            .map(|(_, rest)| rest)
        {
            if line.starts_with("impl") {
                types.push(last_segment(
                    rest.trim_end_matches(" {}").trim_end_matches('{'),
                ));
            }
        }
    }
    types
}

/// The reader finds each spelling of a declaration, and nothing in a comment.
#[test]
fn the_sweep_reads_each_spelling_of_an_error_type() {
    assert_eq!(
        declared_errors(
            "#[derive(Debug, thiserror::Error)]\n\
             #[error(\"a\")]\n\
             pub enum Derived {}\n\
             #[derive(Debug, thiserror::Error)]\n\
             struct Private;\n\
             impl std::error::Error for Implemented {}\n\
             impl<R: Scopes> std::error::Error for Generic<R> {}\n\
             // impl std::error::Error for Commented {}"
        ),
        ["Derived", "Private", "Implemented", "Generic"]
    );
}

/// The error types Kynos declares, read from its source, are exactly the ones
/// witnessed reportable here and the ones no caller can hold.
///
/// A type added without a witness is one whose `Send + Sync` nothing pins, so
/// it can stop being reportable with nothing failing.
#[test]
fn every_error_type_kynos_declares_is_accounted_for() {
    let mut declared = sources(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src"
    )))
    .iter()
    .flat_map(|file| {
        declared_errors(&std::fs::read_to_string(file).expect("a readable source file"))
    })
    .collect::<Vec<_>>();
    declared.sort();

    let mut accounted = witnessed();
    accounted.extend(UNRECEIVABLE.iter().map(|name| (*name).to_owned()));
    accounted.sort();

    assert_eq!(
        declared, accounted,
        "the error types declared in `src/` and the ones accounted for here differ; witness a \
         new one with `reportable` or name it in `UNRECEIVABLE` with the reason no caller can \
         hold it"
    );
}
