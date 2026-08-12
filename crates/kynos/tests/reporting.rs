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

/// Asserts that `E` can be `?`d into an `anyhow::Error` or an `eyre::Report`.
///
/// The bound is theirs, spelled out rather than imported. Depending on either
/// crate to check it would add a row to the dependency manifest in
/// [`architecture.md`](../../../docs/architecture.md#dependencies) in order to
/// assert a property that belongs to `std`.
fn reportable<E: std::error::Error + Send + Sync + 'static>() {}

/// The framework's own failure is what every example returns from `main`, so
/// this is the one that decides whether an application may keep its own
/// reporter.
#[test]
fn a_build_failure_is_reportable() {
    reportable::<kynos::Error>();
}

/// `ServerError` reaches an application through `Error::Server`, but it is
/// public in its own right and a program may hold one directly.
#[cfg(feature = "server")]
#[test]
fn a_server_failure_is_reportable() {
    reportable::<kynos::server::error::ServerError>();
}

/// `TlsError` is the one users construct furthest from `main` — the whole
/// `TlsConfig` builder returns it — so it has the longest way to travel.
#[cfg(feature = "tls")]
#[test]
fn a_tls_failure_is_reportable() {
    reportable::<kynos::server::tls::error::TlsError>();
}
