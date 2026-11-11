//! Fitting the process's open-file limit to the connection cap.
//!
//! Many systems start a process with a soft `RLIMIT_NOFILE` of 1024, well
//! below the default cap of 10,000 connections. A server short of descriptors
//! does not fail at startup: its accepts fail with `EMFILE` and are retried, so
//! clients stall under load. Raising the soft limit as far as the hard limit
//! needs no privilege, so it is raised to what the cap needs, and a limit that
//! still falls short is reported once, at startup.

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

/// Descriptors held apart from connections and listeners: the standard
/// streams, the runtime's own, and the application's files and pools.
///
/// Small enough that the default cap and a hundred listeners stay within
/// macOS's `OPEN_MAX` of 10,240, past which its `setrlimit` refuses a soft
/// limit even under an unlimited hard one.
pub(in crate::server) const RESERVED_DESCRIPTORS: u64 = 128;

/// The descriptors `max_connections` connections and `listeners` listeners
/// need, with [`RESERVED_DESCRIPTORS`] to spare.
pub(in crate::server) fn required(max_connections: usize, listeners: usize) -> u64 {
    let wide = |count: usize| u64::try_from(count).unwrap_or(u64::MAX);
    wide(max_connections)
        .saturating_add(wide(listeners))
        .saturating_add(RESERVED_DESCRIPTORS)
}

/// The soft limit to raise `limit` to so it covers `required`, as far as its
/// hard limit allows, or `None` where it already covers it or cannot rise.
/// `None` in either half of `limit` is unlimited.
///
/// Raised no further than `required`, so the process, and every child it
/// spawns, keeps the smallest limit that serves the cap.
pub(in crate::server) fn raised(limit: Rlimit, required: u64) -> Option<u64> {
    let current = limit.current?;
    if current >= required {
        return None;
    }
    let target = limit
        .maximum
        .map_or(required, |maximum| maximum.min(required));
    (target > current).then_some(target)
}

/// Raises the soft `RLIMIT_NOFILE` toward the hard limit until it covers
/// `max_connections` connections and `listeners` listeners, and warns, naming
/// both numbers, where it still does not.
pub(in crate::server) fn fit(max_connections: usize, listeners: usize) {
    let required = required(max_connections, listeners);
    let limit = getrlimit(Resource::Nofile);
    let Some(mut soft) = limit.current else {
        return;
    };
    let mut refused = None;
    if let Some(target) = raised(limit, required) {
        match setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(target),
                maximum: limit.maximum,
            },
        ) {
            Ok(()) => {
                tracing::debug!(from = soft, to = target, "raised the open-file soft limit");
                soft = target;
            }
            Err(error) => refused = Some(error),
        }
    }
    if soft < required {
        tracing::warn!(
            soft_limit = soft,
            hard_limit = ?limit.maximum,
            required,
            max_connections,
            error = ?refused,
            "the open-file limit is below what the connection cap needs; accepts will fail with EMFILE under load, so raise it (`ulimit -n`, `LimitNOFILE=`) or lower `max_connections`",
        );
    }
}
