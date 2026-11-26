//! The sliding-window estimator [`Quotas`](super::quota::Quotas) decides by:
//! integer arithmetic over two windows' counts, apart from the store and the
//! key that produce them.

use std::time::Duration;

/// The sliding-window estimate, `elapsed` into a window of `window`.
///
/// `current + previous × (window − elapsed) / window`, which stops a fixed
/// window's boundary doubling the rate. Saturating, so overflow refuses rather
/// than wraps into permission.
pub(super) fn estimate(previous: u64, current: u64, elapsed: Duration, window: Duration) -> u64 {
    if window.is_zero() {
        return current;
    }

    let window_ms = window.as_millis().max(1);
    let elapsed_ms = elapsed.as_millis().min(window_ms);
    let weight = window_ms - elapsed_ms;

    let carried = u128::from(previous)
        .saturating_mul(weight)
        .saturating_div(window_ms);

    current.saturating_add(u64::try_from(carried).unwrap_or(u64::MAX))
}

/// The earliest instant the estimate falls to `headroom`.
///
/// Assuming the client sends nothing more: the current window's remainder, plus
/// however much of the next window must pass for the carried weight to decay.
pub(super) fn recovers_in(
    current: u64,
    headroom: u64,
    elapsed: Duration,
    window: Duration,
) -> Duration {
    let remaining_window = window.saturating_sub(elapsed);

    if current <= headroom {
        return remaining_window;
    }

    // Decays to `headroom` after `window × (current − headroom) / current`, in
    // integer milliseconds: an `f64` loses precision past 2^53.
    let excess = u128::from(current - headroom);
    let window_ms = window.as_millis();
    let decay_ms = window_ms
        .saturating_mul(excess)
        .saturating_div(u128::from(current).max(1));
    let decay = Duration::from_millis(u64::try_from(decay_ms).unwrap_or(u64::MAX));

    remaining_window.saturating_add(decay)
}
