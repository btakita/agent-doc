//! Pure recycle-request state policy.
//!
//! Distinct from `recycle_inflight` (which signals a recycle is *in progress* so
//! dispatch should defer) and `recycle_yield` (which asks a self-driving loop to
//! yield one boundary): a recycle-request is a positive cross-process instruction
//! that a specific open supervisor should recycle onto the freshly-installed
//! binary at its next idle boundary, EVEN when it is not yet stale and auto-recycle
//! is opted out. An install fan-out records it per served document; the supervisor
//! idle loop honors it like an `explicit_admin` recycle.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default request freshness window. Generous so a supervisor mid-turn still sees
/// the request when it next reaches an idle boundary.
pub const DEFAULT_RECYCLE_REQUEST_TTL_SECS: u64 = 900;
pub const RECYCLE_REQUEST_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_REQUEST_TTL_SECS";

/// Canonical reason for an install-driven cross-supervisor recycle request.
pub const RECYCLE_REQUEST_INSTALL_FANOUT: &str = "install_fanout";
/// Canonical reason for a stale supervisor observed at any turn stage.
pub const RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE: &str = "stale_supervisor_turn_stage";
/// Canonical reason for a route-owned editor authority whose relay replica is
/// missing or whose disk projection no longer matches canonical state.
pub const RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE: &str = "stale_editor_replica_turn_stage";

/// Projected recycle-request state.
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

/// GH #121: whether the owning supervisor should still honour `request`.
///
/// The TTL keeps an install fan-out or editor-replica request from firing long
/// after its cause. A `stale_supervisor_turn_stage` request is different: its
/// cause is the supervisor running superseded bytes, which is still true for as
/// long as `supervisor_stale` holds. A supervisor whose document cycle stayed
/// open past the TTL (a long turn) used to let the request lapse silently, so
/// the layout path's "safe-boundary recycle requested" never executed — five
/// requests, zero recycles, one pid for three days. Honour it until the
/// supervisor is no longer stale; consuming it settles the request.
pub fn recycle_request_is_live(request: &RecycleRequest, now: u64, supervisor_stale: bool) -> bool {
    recycle_request_is_fresh(request, now)
        || (supervisor_stale && request.reason == RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE)
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
    fn stale_supervisor_request_outlives_the_ttl_only_while_the_supervisor_is_stale() {
        // GH #121: a request made at 16:16 for a supervisor whose cycle stayed
        // open for longer than the TTL must still fire at the next boundary.
        let stale = recycle_request(RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE, 1_000);
        let long_after = 1_000 + 2 * 3_600;
        assert!(!recycle_request_is_fresh(&stale, long_after));
        assert!(recycle_request_is_live(&stale, long_after, true));
        assert!(
            !recycle_request_is_live(&stale, long_after, false),
            "once the supervisor is fresh the lapsed request has nothing to repair"
        );
        // Other reasons keep the TTL regardless of staleness.
        let fanout = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 1_000);
        assert!(!recycle_request_is_live(&fanout, long_after, true));
        assert!(recycle_request_is_live(&fanout, 1_000, false));
    }

    #[test]
    fn freshness_uses_ttl_window() {
        let request = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 1_000);

        assert!(recycle_request_is_fresh(&request, 1_000));
        assert!(!recycle_request_is_fresh(&request, 1_000_000));
    }
}
