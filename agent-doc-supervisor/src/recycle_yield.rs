//! Pure recycle-yield marker policy.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document recycle-yield
/// requests.
pub const RECYCLE_YIELD_DIR: &str = ".agent-doc/recycle-yield";

/// Default request freshness window.
pub const DEFAULT_RECYCLE_YIELD_TTL_SECS: u64 = 120;
pub const RECYCLE_YIELD_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_YIELD_TTL_SECS";

/// Canonical reason for a stale-binary self-recycle yield.
pub const RECYCLE_YIELD_STALE_BINARY: &str = "stale_binary_drain";

/// Reason for a fresh-binary state-flush self-recycle yield.
pub const RECYCLE_YIELD_STATE_FLUSH: &str = "state_flush_drain";

/// Persisted recycle-yield request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleYieldRequest {
    /// Why the supervisor asked the loop to yield.
    pub reason: String,
    /// Unix seconds the request was written/refreshed.
    pub requested_secs: u64,
}

pub fn recycle_yield_request(reason: &str, requested_secs: u64) -> RecycleYieldRequest {
    RecycleYieldRequest {
        reason: reason.to_string(),
        requested_secs,
    }
}

/// Resolve the request TTL, honoring the `AGENT_DOC_RECYCLE_YIELD_TTL_SECS`
/// override.
pub fn recycle_yield_ttl() -> Duration {
    let secs = std::env::var(RECYCLE_YIELD_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_YIELD_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn recycle_yield_request_is_fresh(request: &RecycleYieldRequest, now: u64) -> bool {
    agent_doc_lease::timestamp_is_fresh(request.requested_secs, now, recycle_yield_ttl())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_reason_and_timestamp() {
        let request = recycle_yield_request(RECYCLE_YIELD_STALE_BINARY, 42);

        assert_eq!(request.reason, RECYCLE_YIELD_STALE_BINARY);
        assert_eq!(request.requested_secs, 42);
    }

    #[test]
    fn freshness_uses_ttl_window() {
        let request = recycle_yield_request(RECYCLE_YIELD_STATE_FLUSH, 1_000);

        assert!(recycle_yield_request_is_fresh(&request, 1_000));
        assert!(!recycle_yield_request_is_fresh(&request, 11_000));
    }
}
