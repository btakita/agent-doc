//! The relay commit barrier's verdict, carried from the relay through the
//! controller RPC to every CLI refusal (`#inflighttypinghold` (3)).
//!
//! The barrier used to return a bare `bool`, so each CLI refusal site printed
//! the one text it had ("CRDT relay convergence is still pending") even when
//! the relay refused for a different reason with a different remedy. Since
//! `#clobberfollowups` the barrier also refuses while a live editor replica is
//! quarantined: that editor holds operator text the canonical lacks, so
//! waiting on "convergence" and the recoveries an agent reaches for next are
//! the wrong moves. The verdict names the reason so the refusal can too.

use serde::{Deserialize, Serialize};

/// Why the relay commit barrier refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitBarrierRefusal {
    /// The inbound editor-to-canonical cut is not established yet (the
    /// historical, generic refusal). Also what a controller that predates the
    /// reason, and any refusal without a more specific cause, reads as.
    ConvergencePending,
    /// A live editor replica's updates are quarantined while it still owes the
    /// canonical projection: the editor holds operator text the canonical
    /// lacks (`crdt_commit_barrier_refused_quarantined_replica`).
    QuarantinedReplica,
}

impl CommitBarrierRefusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConvergencePending => "convergence_pending",
            Self::QuarantinedReplica => "quarantined_replica",
        }
    }
}

/// The relay commit barrier's verdict for one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "refusal", rename_all = "snake_case")]
pub enum CommitBarrierVerdict {
    Ready,
    Refused(CommitBarrierRefusal),
}

impl CommitBarrierVerdict {
    /// The generic refusal, for every site that has no more specific cause.
    pub const CONVERGENCE_PENDING: Self = Self::Refused(CommitBarrierRefusal::ConvergencePending);

    /// A verdict from the legacy `bool` the barrier used to return.
    pub const fn from_ready(ready: bool) -> Self {
        if ready {
            Self::Ready
        } else {
            Self::CONVERGENCE_PENDING
        }
    }

    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn refusal(self) -> Option<CommitBarrierRefusal> {
        match self {
            Self::Ready => None,
            Self::Refused(refusal) => Some(refusal),
        }
    }

    /// Decode the controller's `crdt_commit_barrier` response data.
    ///
    /// A controller that predates the verdict answers a bare `bool` even when
    /// asked for the verdict; it reads as [`Self::from_ready`]. Anything else
    /// that does not decode is an error, never a silent "ready".
    pub fn from_wire(data: &serde_json::Value) -> Result<Self, String> {
        if let Some(ready) = data.as_bool() {
            return Ok(Self::from_ready(ready));
        }
        serde_json::from_value(data.clone())
            .map_err(|error| format!("undecodable commit barrier verdict {data}: {error}"))
    }
}

/// The request payload that asks a controller for the full verdict instead of
/// the legacy `bool`. An older controller ignores it and answers the `bool`;
/// an older client never sends it and still receives the `bool`.
pub const VERDICT_REQUEST_PAYLOAD: &str = r#"{"verdict":true}"#;

/// Whether a `crdt_commit_barrier` request payload asks for the full verdict.
pub fn request_wants_verdict(payload: Option<&str>) -> bool {
    payload
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|payload| payload.get("verdict").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#inflighttypinghold` (3): the verdict round-trips, and a controller
    /// that predates it (a bare `bool`) still decodes, as the generic refusal.
    #[test]
    fn inflighttypinghold_commit_barrier_verdict_wire_is_backward_compatible() {
        for verdict in [
            CommitBarrierVerdict::Ready,
            CommitBarrierVerdict::CONVERGENCE_PENDING,
            CommitBarrierVerdict::Refused(CommitBarrierRefusal::QuarantinedReplica),
        ] {
            let wire = serde_json::to_value(verdict).unwrap();
            assert_eq!(CommitBarrierVerdict::from_wire(&wire).unwrap(), verdict);
        }
        assert_eq!(
            serde_json::to_value(CommitBarrierVerdict::Refused(
                CommitBarrierRefusal::QuarantinedReplica
            ))
            .unwrap(),
            serde_json::json!({"status": "refused", "refusal": "quarantined_replica"})
        );
        assert_eq!(
            CommitBarrierVerdict::from_wire(&serde_json::json!(true)).unwrap(),
            CommitBarrierVerdict::Ready
        );
        assert_eq!(
            CommitBarrierVerdict::from_wire(&serde_json::json!(false)).unwrap(),
            CommitBarrierVerdict::CONVERGENCE_PENDING
        );
        assert!(CommitBarrierVerdict::from_wire(&serde_json::json!("ready")).is_err());

        assert!(request_wants_verdict(Some(VERDICT_REQUEST_PAYLOAD)));
        assert!(!request_wants_verdict(None));
        assert!(!request_wants_verdict(Some("{}")));
        assert!(!request_wants_verdict(Some("not json")));
    }
}
