//! The deprecation an expansion names at a chrono type.
//!
//! Cargo cannot deprecate a feature and `#[deprecated]` on a trait
//! implementation has no effect, so the warning is this item's: always
//! present, so an expansion naming it compiles in every build, and deprecated
//! only under `time-chrono04`, so it warns exactly when the feature is on.

/// Named at each chrono type a route attribute or derive is handed.
#[cfg_attr(
    feature = "time-chrono04",
    deprecated(
        note = "Kynos's `time-chrono04` feature is deprecated and may be removed in any \
                future 0.x minor: chrono is unmaintained (RUSTSEC-2026-0543). Move to \
                `time-jiff1`, whose types map onto the same schema shapes"
    )
)]
#[allow(non_upper_case_globals)]
pub const chrono_is_unmaintained: () = ();
