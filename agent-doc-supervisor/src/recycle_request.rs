//! Pure recycle-REQUEST marker policy.
//!
//! Distinct from `recycle_inflight` (which signals a recycle is *in progress* so
//! dispatch should defer) and `recycle_yield` (which asks a self-driving loop to
//! yield one boundary): a recycle-request is a positive cross-process instruction
//! that a specific route-owned supervisor should recycle onto the freshly-installed
//! binary at its next idle boundary, EVEN when it is not yet stale and auto-recycle
//! is opted out. An install fan-out writes it per served document; the supervisor
//! idle loop honors it like an `explicit_admin` recycle.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document recycle requests.
pub const RECYCLE_REQUEST_DIR: &str = ".agent-doc/recycle-request";

/// Default request freshness window. Generous so a supervisor mid-turn still sees
/// the request when it next reaches an idle boundary.
pub const DEFAULT_RECYCLE_REQUEST_TTL_SECS: u64 = 900;
pub const RECYCLE_REQUEST_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_REQUEST_TTL_SECS";

/// Canonical reason for an install-driven cross-supervisor recycle request.
pub const RECYCLE_REQUEST_INSTALL_FANOUT: &str = "install_fanout";
/// Canonical reason for a preflight-observed stale supervisor recycle request.
pub const RECYCLE_REQUEST_STALE_SUPERVISOR_PREFLIGHT: &str = "stale_supervisor_preflight";

/// Persisted recycle-request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleRequest {
    /// Why the supervisor was asked to recycle.
    pub reason: String,
    /// Unix seconds the request was written/refreshed.
    pub requested_secs: u64,
}

pub fn recycle_request(reason: &str, requested_secs: u64) -> RecycleRequest {
    RecycleRequest {
        reason: reason.to_string(),
        requested_secs,
    }
}

/// Resolve the request TTL, honoring the `AGENT_DOC_RECYCLE_REQUEST_TTL_SECS`
/// override.
pub fn recycle_request_ttl() -> Duration {
    let secs = std::env::var(RECYCLE_REQUEST_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_REQUEST_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn recycle_request_is_fresh(request: &RecycleRequest, now: u64) -> bool {
    agent_doc_lease::timestamp_is_fresh(request.requested_secs, now, recycle_request_ttl())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_reason_and_timestamp() {
        let request = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 42);

        assert_eq!(request.reason, RECYCLE_REQUEST_INSTALL_FANOUT);
        assert_eq!(request.requested_secs, 42);
    }

    #[test]
    fn freshness_uses_ttl_window() {
        let request = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 1_000);

        assert!(recycle_request_is_fresh(&request, 1_000));
        assert!(!recycle_request_is_fresh(&request, 1_000_000));
    }
}
