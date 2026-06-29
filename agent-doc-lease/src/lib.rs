//! TTL freshness primitives for filesystem leases and request sidecars.
//!
//! This crate intentionally owns only the pure timestamp policy. Domain crates
//! still own their lease bodies, paths, environment knobs, and side effects.

use std::time::Duration;

/// Returns `true` while `observed_secs` is within `ttl` of `now_secs`.
///
/// Future timestamps are treated as fresh to tolerate small wall-clock skew
/// between cooperating processes.
pub fn timestamp_is_fresh(observed_secs: u64, now_secs: u64, ttl: Duration) -> bool {
    now_secs.saturating_sub(observed_secs) <= ttl.as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_freshness_uses_ttl_window() {
        let ttl = Duration::from_secs(30);
        assert!(timestamp_is_fresh(1_000, 1_000, ttl), "same instant");
        assert!(timestamp_is_fresh(1_000, 1_030, ttl), "at the ttl edge");
        assert!(!timestamp_is_fresh(1_000, 1_031, ttl), "past the ttl");
        assert!(timestamp_is_fresh(2_000, 1_000, ttl), "future timestamp");
    }
}
