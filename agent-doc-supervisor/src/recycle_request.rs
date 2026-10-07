//! Pure recycle-request state policy.
//!
//! Distinct from `recycle_inflight` (which signals a recycle is *in progress* so
//! dispatch should defer) and `recycle_yield` (which asks a self-driving loop to
//! yield one boundary): a recycle-request is a positive cross-process instruction
//! that a specific open supervisor should reconsider recycling at its next safe
//! boundary. An install fan-out records it per served document; the supervisor
//! re-observes the named cause before acting, so delivery that arrives after the
//! repair cannot recycle the replacement generation. Explicit operator and force
//! requests remain unconditional.

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
/// Canonical reason for an editor transport episode that crossed the durable
/// write-wedge threshold.
pub const RECYCLE_REQUEST_EDITOR_WRITE_WEDGE: &str = "repeated_ack_timeout_active_listener";

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
///
/// GH #136: an install fan-out names the same cause. `install_fanout` is
/// written for every open supervisor because a new binary was installed, so a
/// supervisor that still runs replaced bytes still has exactly the cause the
/// request names. Before this, an install fan-out landing on top of a
/// non-lapsing `stale_supervisor_turn_stage` request *replaced* it in the
/// projection with a request that lapsed after the TTL: five of the seven
/// `prior_request=unconsumed` records in GH #136 were `install_fanout`, aged up
/// to 80,071s, against supervisors that were still stale.
pub fn recycle_request_is_live(request: &RecycleRequest, now: u64, supervisor_stale: bool) -> bool {
    recycle_request_is_fresh(request, now)
        || (supervisor_stale && recycle_reason_is_binary_replacement(&request.reason))
}

/// Reason for a forced install fan-out (`admin install --force`).
pub const RECYCLE_REQUEST_INSTALL_FANOUT_FORCE: &str = "install_fanout_force";

/// GH #136: request reasons whose cause is "this supervisor runs a superseded
/// binary". Such a request has nothing to expire against while the supervisor
/// is still stale.
pub fn recycle_reason_is_binary_replacement(reason: &str) -> bool {
    matches!(
        reason,
        RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE
            | RECYCLE_REQUEST_INSTALL_FANOUT
            | RECYCLE_REQUEST_INSTALL_FANOUT_FORCE
    )
}

/// Whether the cause named by a durable recycle request still holds in the
/// supervisor generation that is about to consume it.
///
/// Requests are delivery of intent, not proof that their cause still exists.
/// In particular, install fan-out and editor-health producers can finish after
/// an in-place reexec has already repaired the condition. Treating those late
/// deliveries as unconditional admin requests makes the fresh generation
/// reexec again and can form a project-wide respawn loop. Unknown reasons are
/// retained as explicit/admin requests for compatibility; force is explicitly
/// unconditional.
pub fn recycle_request_cause_is_live(
    reason: &str,
    supervisor_stale: bool,
    editor_write_wedge_needs_recycle: bool,
    stale_editor_replica: bool,
) -> bool {
    match reason {
        RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE | RECYCLE_REQUEST_INSTALL_FANOUT => {
            supervisor_stale
        }
        RECYCLE_REQUEST_EDITOR_WRITE_WEDGE => editor_write_wedge_needs_recycle,
        RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE => stale_editor_replica,
        RECYCLE_REQUEST_INSTALL_FANOUT_FORCE => true,
        _ => true,
    }
}

/// GH #136: how long the layout path waits for an IDLE stale supervisor to
/// consume its safe-boundary recycle request before the request is declared
/// overdue. It must exceed the supervisor's idle grace
/// (`DEFAULT_RECYCLE_IDLE_GRACE_SECS`, 5s) plus a watch tick and an `execve`;
/// two minutes leaves an order of magnitude of slack. A supervisor whose pane
/// is mid-turn is never overdue: its recycle is correctly deferred to the
/// turn boundary.
///
/// This is a LIVENESS budget for a local process (the supervisor and the
/// state.db it reads are on the same host as the controller), never a safety
/// argument: every GH #136 invariant holds whatever value it takes (see
/// `formal/tla/StaleColumnRecycle.tla`, where it is an arbitrary `Expire`).
/// Override with `AGENT_DOC_STALE_RECYCLE_CONSUME_BOUND_SECS`.
pub const STALE_RECYCLE_CONSUME_BOUND_SECS: u64 = 120;
pub const STALE_RECYCLE_CONSUME_BOUND_SECS_ENV: &str = "AGENT_DOC_STALE_RECYCLE_CONSUME_BOUND_SECS";

/// The configured consumption bound (see [`STALE_RECYCLE_CONSUME_BOUND_SECS`]).
pub fn stale_recycle_consume_bound_secs() -> u64 {
    std::env::var(STALE_RECYCLE_CONSUME_BOUND_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(STALE_RECYCLE_CONSUME_BOUND_SECS)
}

/// GH #136: the lifecycle state of the safe-boundary recycle request for a
/// supervisor the layout path has just observed to be stale.
///
/// The observation itself is the consumption witness: a request is consumed
/// either by the supervisor re-execing onto the installed build (it then reads
/// fresh, and this classification is never asked) or by settling the request
/// (the projection leaves `Requested`). So for a still-stale supervisor, an
/// outstanding request is by construction *unconsumed*, and the only question
/// is whether its consumer has had a fair chance to consume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleRecycleRequestState {
    /// No outstanding request. The layout path requests one now.
    NotRequested,
    /// Outstanding, and its consumer still has time: either the first
    /// unconsumed request is younger than the bound, or the supervisor's pane
    /// is running a turn and the recycle is deferred to its boundary.
    Pending {
        reason: String,
        age_secs: u64,
        deferred_by_turn: bool,
    },
    /// Outstanding past the bound while the supervisor sat idle: the consumer
    /// had its chance and did not take it. Nothing may be admitted "on the
    /// strength of" this request any more.
    Overdue { reason: String, age_secs: u64 },
}

impl StaleRecycleRequestState {
    pub fn is_overdue(&self) -> bool {
        matches!(self, Self::Overdue { .. })
    }

    /// Single `prior_request=` log token.
    pub fn log_token(&self) -> String {
        match self {
            Self::NotRequested => "none".to_string(),
            Self::Pending {
                reason,
                age_secs,
                deferred_by_turn,
            } => format!(
                "pending:reason={reason}:age_secs={age_secs}{}",
                if *deferred_by_turn {
                    ":deferred_by_turn"
                } else {
                    ""
                }
            ),
            Self::Overdue { reason, age_secs } => {
                format!("overdue:reason={reason}:age_secs={age_secs}")
            }
        }
    }
}

/// An outstanding (unconsumed) request as the ledger records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutstandingRecycleRequest {
    /// The newest request (its reason is what the consumer acts on).
    pub latest: RecycleRequest,
    /// When the FIRST request not yet consumed was made. Re-requests and
    /// install fan-outs refresh `latest` but never this, so a refresh cannot
    /// reset the consumption clock (GH #136: otherwise every refresh would
    /// re-admit an overdue stale pane for another bound, i.e. a flap).
    pub first_requested_secs: u64,
}

/// Pure GH #136 classification. Total over its inputs; `now` earlier than the
/// request (clock skew) reads as age 0, never as overdue.
pub fn classify_stale_recycle_request(
    outstanding: Option<&OutstandingRecycleRequest>,
    now: u64,
    consumer_turn_active: bool,
    bound_secs: u64,
) -> StaleRecycleRequestState {
    let Some(outstanding) = outstanding else {
        return StaleRecycleRequestState::NotRequested;
    };
    let reason = outstanding.latest.reason.clone();
    let age_secs = now.saturating_sub(outstanding.first_requested_secs);
    if consumer_turn_active || age_secs <= bound_secs {
        StaleRecycleRequestState::Pending {
            reason,
            age_secs,
            deferred_by_turn: consumer_turn_active && age_secs > bound_secs,
        }
    } else {
        StaleRecycleRequestState::Overdue { reason, age_secs }
    }
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
        // GH #136: an install fan-out names the same cause (a replaced
        // binary), so it too outlives the TTL while the supervisor is stale.
        let fanout = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 1_000);
        assert!(recycle_request_is_live(&fanout, long_after, true));
        assert!(!recycle_request_is_live(&fanout, long_after, false));
        assert!(recycle_request_is_live(&fanout, 1_000, false));
    }

    #[test]
    fn gh136_install_fanout_does_not_downgrade_a_stale_supervisor_request() {
        // An install fan-out landing on a stale supervisor's request replaced
        // its reason; the replacement must not lapse while the cause holds.
        let long_after = 1_000 + 22 * 3_600;
        for reason in [
            RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE,
            RECYCLE_REQUEST_INSTALL_FANOUT,
            RECYCLE_REQUEST_INSTALL_FANOUT_FORCE,
        ] {
            let request = recycle_request(reason, 1_000);
            assert!(
                recycle_request_is_live(&request, long_after, true),
                "{reason}"
            );
            assert!(
                !recycle_request_is_live(&request, long_after, false),
                "{reason}"
            );
        }
        let replica = recycle_request(RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE, 1_000);
        assert!(
            !recycle_request_is_live(&replica, long_after, true),
            "an editor-replica request is not caused by the binary and keeps its TTL"
        );
    }

    #[test]
    fn delayed_causal_requests_do_not_recycle_a_repaired_generation() {
        for reason in [
            RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE,
            RECYCLE_REQUEST_INSTALL_FANOUT,
        ] {
            assert!(recycle_request_cause_is_live(reason, true, false, false));
            assert!(
                !recycle_request_cause_is_live(reason, false, false, false),
                "a delayed {reason} must not reexec an already-fresh generation"
            );
        }
        assert!(recycle_request_cause_is_live(
            RECYCLE_REQUEST_EDITOR_WRITE_WEDGE,
            false,
            true,
            false,
        ));
        assert!(!recycle_request_cause_is_live(
            RECYCLE_REQUEST_EDITOR_WRITE_WEDGE,
            false,
            false,
            false,
        ));
        assert!(recycle_request_cause_is_live(
            RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE,
            false,
            false,
            true,
        ));
        assert!(recycle_request_cause_is_live(
            RECYCLE_REQUEST_INSTALL_FANOUT_FORCE,
            false,
            false,
            false,
        ));
        assert!(recycle_request_cause_is_live(
            "operator_request",
            false,
            false,
            false,
        ));
    }

    fn outstanding(first: u64, latest: u64) -> OutstandingRecycleRequest {
        OutstandingRecycleRequest {
            latest: recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, latest),
            first_requested_secs: first,
        }
    }

    #[test]
    fn gh136_stale_request_classification_is_total_and_monotone_in_age() {
        let bound = STALE_RECYCLE_CONSUME_BOUND_SECS;
        assert_eq!(
            classify_stale_recycle_request(None, 10, false, bound),
            StaleRecycleRequestState::NotRequested
        );
        // Exhaustive over a window around the bound, both turn states, and a
        // refresh that moved `latest` but not `first`.
        for turn_active in [false, true] {
            let mut was_overdue = false;
            for now in 0..=(3 * bound) {
                let state = classify_stale_recycle_request(
                    Some(&outstanding(0, now)),
                    now,
                    turn_active,
                    bound,
                );
                let overdue = state.is_overdue();
                assert_eq!(overdue, !turn_active && now > bound, "now={now}");
                // Monotone: once overdue (idle), ageing never un-overdues it,
                // and a refresh of `latest` does not reset the clock.
                assert!(!was_overdue || overdue, "flap at now={now}");
                was_overdue = overdue;
            }
        }
        // Clock skew never reads as overdue.
        assert!(
            !classify_stale_recycle_request(Some(&outstanding(500, 500)), 10, false, bound)
                .is_overdue()
        );
        assert_eq!(
            classify_stale_recycle_request(Some(&outstanding(0, 0)), 31_622, false, bound)
                .log_token(),
            "overdue:reason=install_fanout:age_secs=31622"
        );
        assert_eq!(
            classify_stale_recycle_request(Some(&outstanding(0, 0)), 300, true, bound).log_token(),
            "pending:reason=install_fanout:age_secs=300:deferred_by_turn"
        );
    }

    #[test]
    fn freshness_uses_ttl_window() {
        let request = recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, 1_000);

        assert!(recycle_request_is_fresh(&request, 1_000));
        assert!(!recycle_request_is_fresh(&request, 1_000_000));
    }
}
