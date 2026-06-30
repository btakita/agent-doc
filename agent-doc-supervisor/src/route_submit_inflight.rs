//! Pure route-submit in-flight marker policy.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding route-submit in-flight
/// markers keyed by document.
pub const ROUTE_IN_FLIGHT_DIR: &str = ".agent-doc/route-in-flight";

/// Default route-submit marker freshness window.
pub const ROUTE_IN_FLIGHT_TTL_SECS: u64 = 30;

/// Extended freshness window for dispatch-only ready probes, which can wait on
/// editor delivery longer than a normal dispatch submit.
pub const ROUTE_READY_PROBE_TTL_SECS: u64 = 150;

/// Directory (relative to the project root) holding route-submit blocked
/// markers keyed by document.
pub const ROUTE_BLOCKED_DIR: &str = ".agent-doc/route-submit-blocked";

/// Freshness window for accepted route submits that never produced dispatch
/// start proof.
pub const ROUTE_BLOCKED_TTL_SECS: u64 = 120;

pub const ROUTE_DISPATCH_SUBMIT_REASON: &str = "dispatch_submit";
pub const ROUTE_DISPATCH_ONLY_READY_PROBE_REASON: &str = "dispatch_only_ready_probe";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSubmitInFlight {
    pub file: String,
    pub pane: String,
    pub harness: String,
    #[serde(default)]
    pub reason: String,
    pub written_at: u64,
}

impl RouteSubmitInFlight {
    pub fn reason_label(&self) -> &str {
        if self.reason.is_empty() {
            "legacy"
        } else {
            &self.reason
        }
    }

    pub fn ttl_secs(&self) -> u64 {
        route_submit_ttl_secs_for_reason(self.reason_label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSubmitBlocked {
    pub file: String,
    pub pane: String,
    pub harness: String,
    pub reason: String,
    pub written_at: u64,
}

pub fn route_submit_inflight_marker(
    file: impl Into<String>,
    pane: impl Into<String>,
    harness: impl Into<String>,
    reason: impl Into<String>,
    written_at: u64,
) -> RouteSubmitInFlight {
    RouteSubmitInFlight {
        file: file.into(),
        pane: pane.into(),
        harness: harness.into(),
        reason: reason.into(),
        written_at,
    }
}

pub fn route_submit_blocked_marker(
    file: impl Into<String>,
    pane: impl Into<String>,
    harness: impl Into<String>,
    reason: impl Into<String>,
    written_at: u64,
) -> RouteSubmitBlocked {
    RouteSubmitBlocked {
        file: file.into(),
        pane: pane.into(),
        harness: harness.into(),
        reason: reason.into(),
        written_at,
    }
}

pub fn route_submit_ttl_secs_for_reason(reason: &str) -> u64 {
    match reason {
        ROUTE_DISPATCH_ONLY_READY_PROBE_REASON => ROUTE_READY_PROBE_TTL_SECS,
        _ => ROUTE_IN_FLIGHT_TTL_SECS,
    }
}

pub fn route_submit_inflight_marker_is_fresh(marker: &RouteSubmitInFlight, now: u64) -> bool {
    agent_doc_lease::timestamp_is_fresh(
        marker.written_at,
        now,
        Duration::from_secs(marker.ttl_secs()),
    )
}

pub fn route_submit_blocked_marker_is_fresh(marker: &RouteSubmitBlocked, now: u64) -> bool {
    agent_doc_lease::timestamp_is_fresh(
        marker.written_at,
        now,
        Duration::from_secs(ROUTE_BLOCKED_TTL_SECS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflight_marker_constructor_preserves_fields() {
        let marker = route_submit_inflight_marker(
            "plan.md",
            "%42",
            "codex",
            ROUTE_DISPATCH_ONLY_READY_PROBE_REASON,
            1_000,
        );

        assert_eq!(marker.file, "plan.md");
        assert_eq!(marker.pane, "%42");
        assert_eq!(marker.harness, "codex");
        assert_eq!(marker.reason, ROUTE_DISPATCH_ONLY_READY_PROBE_REASON);
        assert_eq!(marker.written_at, 1_000);
    }

    #[test]
    fn blocked_marker_constructor_preserves_fields() {
        let marker = route_submit_blocked_marker(
            "plan.md",
            "%1",
            "codex",
            "accepted_without_dispatch_start_proof",
            2_000,
        );

        assert_eq!(marker.file, "plan.md");
        assert_eq!(marker.pane, "%1");
        assert_eq!(marker.harness, "codex");
        assert_eq!(marker.reason, "accepted_without_dispatch_start_proof");
        assert_eq!(marker.written_at, 2_000);
    }

    #[test]
    fn ready_probe_reason_uses_extended_ttl() {
        let marker = route_submit_inflight_marker(
            "plan.md",
            "%1",
            "codex",
            ROUTE_DISPATCH_ONLY_READY_PROBE_REASON,
            1_000,
        );

        assert_eq!(marker.ttl_secs(), ROUTE_READY_PROBE_TTL_SECS);
        assert!(route_submit_inflight_marker_is_fresh(
            &marker,
            1_000 + ROUTE_IN_FLIGHT_TTL_SECS + 1
        ));
        assert!(!route_submit_inflight_marker_is_fresh(
            &marker,
            1_000 + ROUTE_READY_PROBE_TTL_SECS + 1
        ));
    }

    #[test]
    fn default_and_legacy_reasons_use_normal_ttl() {
        let default = route_submit_inflight_marker(
            "plan.md",
            "%1",
            "codex",
            ROUTE_DISPATCH_SUBMIT_REASON,
            1_000,
        );
        let legacy = route_submit_inflight_marker("plan.md", "%1", "codex", "", 1_000);

        assert_eq!(default.reason_label(), ROUTE_DISPATCH_SUBMIT_REASON);
        assert_eq!(default.ttl_secs(), ROUTE_IN_FLIGHT_TTL_SECS);
        assert_eq!(legacy.reason_label(), "legacy");
        assert_eq!(legacy.ttl_secs(), ROUTE_IN_FLIGHT_TTL_SECS);
        assert!(!route_submit_inflight_marker_is_fresh(
            &default,
            1_000 + ROUTE_IN_FLIGHT_TTL_SECS + 1
        ));
    }

    #[test]
    fn blocked_marker_uses_blocked_ttl() {
        let marker = route_submit_blocked_marker(
            "plan.md",
            "%1",
            "codex",
            "accepted_without_dispatch_start_proof",
            1_000,
        );

        assert!(route_submit_blocked_marker_is_fresh(
            &marker,
            1_000 + ROUTE_BLOCKED_TTL_SECS
        ));
        assert!(!route_submit_blocked_marker_is_fresh(
            &marker,
            1_000 + ROUTE_BLOCKED_TTL_SECS + 1
        ));
    }
}
