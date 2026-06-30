//! Pure recycle-in-flight marker policy.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding the recycle-in-flight
/// marker.
pub const RECYCLE_INFLIGHT_DIR: &str = ".agent-doc/recycle-inflight";

/// Fixed marker filename shared across all documents the supervisor hosts.
pub const RECYCLE_INFLIGHT_FILE: &str = "supervisor.json";

/// Default marker freshness window.
pub const DEFAULT_RECYCLE_INFLIGHT_TTL_SECS: u64 = 15;
pub const RECYCLE_INFLIGHT_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS";

/// Canonical reason for a `lib-install` auto-recycle.
pub const RECYCLE_INFLIGHT_AUTO_INSTALL: &str = "auto_install_reexec";

/// Canonical reason for an operator-requested supervisor restart reexec.
pub const RECYCLE_INFLIGHT_RESTART: &str = "restart_reexec";

/// R2/R3 (`#jbdisprecycle`): bound on how long a caller waits for an in-flight
/// supervisor recycle to settle before giving up.
pub const RECYCLE_SETTLE_WAIT: Duration = Duration::from_secs(10);

/// Poll cadence while waiting for a recycle to settle.
pub const RECYCLE_SETTLE_POLL: Duration = Duration::from_millis(250);

/// Persisted recycle-in-flight marker body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleInflightMarker {
    /// Why the supervisor is recycling.
    pub reason: String,
    /// Unix seconds the marker was written/refreshed.
    pub marked_secs: u64,
}

pub fn recycle_inflight_marker(reason: &str, marked_secs: u64) -> RecycleInflightMarker {
    RecycleInflightMarker {
        reason: reason.to_string(),
        marked_secs,
    }
}

/// Resolve the marker TTL, honoring the `AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS`
/// override.
pub fn recycle_inflight_ttl() -> Duration {
    let secs = std::env::var(RECYCLE_INFLIGHT_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_INFLIGHT_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn recycle_inflight_marker_is_fresh(marker: &RecycleInflightMarker, now: u64) -> bool {
    agent_doc_lease::timestamp_is_fresh(marker.marked_secs, now, recycle_inflight_ttl())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_carries_reason_and_timestamp() {
        let marker = recycle_inflight_marker(RECYCLE_INFLIGHT_AUTO_INSTALL, 42);

        assert_eq!(marker.reason, RECYCLE_INFLIGHT_AUTO_INSTALL);
        assert_eq!(marker.marked_secs, 42);
    }

    #[test]
    fn freshness_uses_ttl_window() {
        let marker = recycle_inflight_marker(RECYCLE_INFLIGHT_RESTART, 1_000);

        assert!(recycle_inflight_marker_is_fresh(&marker, 1_000));
        assert!(!recycle_inflight_marker_is_fresh(&marker, 11_000));
    }
}
