//! `#fullboundary` — convergence-gated inter-queue-item boundary (decision core).
//!
//! Root cause this guards (see `tasks/agent-doc/plan-fullboundary-convergence-gate.md`):
//! almost every drift failure dogfooding a session document is a *concurrent-access
//! race* on the doc between three writers — the `finalize` client, the live editor
//! buffer, and the supervisor — because there is **no hard serialization between
//! consuming queue item N and dispatching item N+1**. The drain-owner lease
//! (`#kp5z`) serializes dispatch *ownership* but not editor *convergence*, and the
//! existing closeout even DETECTS non-convergence (`postcommit_worktree_check
//! match=false`) yet proceeds anyway.
//!
//! This module is the **pure decision core**: given the four quiescence proofs for
//! the prior turn's close plus the elapsed wait, it decides whether to dispatch the
//! next item, keep deferring, or trip the bounded timeout into a loud force-disk
//! fallback. It performs no I/O so it is fully unit-testable and safe to land ahead
//! of the live-supervisor-critical dispatch wiring (which needs `make tmux-ci`). The
//! supervisor inter-item dispatch path (idle-queue watch / drain) is the consumer.

use serde::{Deserialize, Serialize};

/// The four quiescence proofs for the prior turn's close, plus the bounded-wait
/// clock. The next queue item dispatches only when all four proofs hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceFacts {
    /// Prior cycle reached `committed` (staged snapshot == `HEAD`).
    pub committed: bool,
    /// The live editor buffer is proven converged == `HEAD` via editor-IPC ACK
    /// (the `postcommit_worktree_check match=true` ground truth) — NOT merely a
    /// git commit plus a fire-and-forget VCS-refresh signal.
    pub editor_converged: bool,
    /// This document's editor-IPC inflight count. Must have drained to 0 — a
    /// non-zero count is the `inflight=5` / `send_failed` pile-up smoking gun.
    pub inflight: u64,
    /// The authoritative actor is idle: a dispatch-ready prompt with no active turn.
    pub actor_idle: bool,
    /// Milliseconds spent waiting on this gate for the current boundary.
    pub elapsed_ms: u64,
    /// Bounded timeout (ms). Past this, an unconverged boundary trips the loud
    /// force-disk fallback rather than deferring forever (which would wedge the
    /// queue on a dead editor).
    pub timeout_ms: u64,
}

/// Stable proof labels, used for `unmet` lists in diagnostics and the playback
/// artifact. Kept as `&'static str` so log lines and tests share one vocabulary.
pub mod proof {
    pub const COMMITTED: &str = "committed";
    pub const EDITOR_CONVERGED: &str = "editor_converged";
    pub const INFLIGHT_DRAINED: &str = "inflight_drained";
    pub const ACTOR_IDLE: &str = "actor_idle";
}

/// The boundary decision for dispatching the next queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvergenceGateDecision {
    /// All four proofs satisfied — the prior turn closed quiescently; dispatch
    /// item N+1 now.
    Dispatch,
    /// Not yet quiescent but still within the bounded timeout — keep deferring and
    /// poll again. `unmet` lists which proofs are still outstanding.
    Defer { unmet: Vec<&'static str> },
    /// The bounded timeout elapsed while still not converged (editor IPC
    /// wedged/dead). The closeout MAY fall back to a direct `--force-disk` write so
    /// work is never permanently stuck — but this is an ERROR condition that must be
    /// logged loudly with a replayable playback artifact. `unmet` lists the proofs
    /// that never satisfied.
    ForceDiskFallback { unmet: Vec<&'static str> },
}

impl ConvergenceFacts {
    /// True when all four quiescence proofs hold.
    pub fn is_quiescent(&self) -> bool {
        self.committed && self.editor_converged && self.inflight == 0 && self.actor_idle
    }

    /// The proofs still outstanding, in a stable order, for diagnostics.
    pub fn unmet_proofs(&self) -> Vec<&'static str> {
        let mut unmet = Vec::new();
        if !self.committed {
            unmet.push(proof::COMMITTED);
        }
        if !self.editor_converged {
            unmet.push(proof::EDITOR_CONVERGED);
        }
        if self.inflight != 0 {
            unmet.push(proof::INFLIGHT_DRAINED);
        }
        if !self.actor_idle {
            unmet.push(proof::ACTOR_IDLE);
        }
        unmet
    }
}

/// Decide whether the next queue item may dispatch across the convergence boundary.
///
/// Quiescent ⇒ [`ConvergenceGateDecision::Dispatch`]. Otherwise, still within the
/// bounded timeout ⇒ [`ConvergenceGateDecision::Defer`]; at or past the timeout ⇒
/// [`ConvergenceGateDecision::ForceDiskFallback`]. A `timeout_ms` of 0 means "no
/// bounded fallback" — an unconverged boundary defers indefinitely (caller opts out
/// of the force-disk escape hatch).
pub fn convergence_gate_decision(facts: &ConvergenceFacts) -> ConvergenceGateDecision {
    if facts.is_quiescent() {
        return ConvergenceGateDecision::Dispatch;
    }
    let unmet = facts.unmet_proofs();
    if facts.timeout_ms > 0 && facts.elapsed_ms >= facts.timeout_ms {
        ConvergenceGateDecision::ForceDiskFallback { unmet }
    } else {
        ConvergenceGateDecision::Defer { unmet }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiescent_facts() -> ConvergenceFacts {
        ConvergenceFacts {
            committed: true,
            editor_converged: true,
            inflight: 0,
            actor_idle: true,
            elapsed_ms: 0,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn all_proofs_satisfied_dispatches() {
        let facts = quiescent_facts();
        assert!(facts.is_quiescent());
        assert!(facts.unmet_proofs().is_empty());
        assert_eq!(
            convergence_gate_decision(&facts),
            ConvergenceGateDecision::Dispatch
        );
    }

    #[test]
    fn uncommitted_within_timeout_defers_not_dispatches() {
        let facts = ConvergenceFacts {
            committed: false,
            elapsed_ms: 100,
            ..quiescent_facts()
        };
        assert!(!facts.is_quiescent());
        assert_eq!(
            convergence_gate_decision(&facts),
            ConvergenceGateDecision::Defer {
                unmet: vec![proof::COMMITTED]
            }
        );
    }

    #[test]
    fn inflight_pileup_blocks_dispatch() {
        // The inflight=5 / send_failed smoking gun must NOT dispatch item N+1.
        let facts = ConvergenceFacts {
            inflight: 5,
            elapsed_ms: 10,
            ..quiescent_facts()
        };
        match convergence_gate_decision(&facts) {
            ConvergenceGateDecision::Defer { unmet } => {
                assert_eq!(unmet, vec![proof::INFLIGHT_DRAINED]);
            }
            other => panic!("expected Defer on inflight pile-up, got {other:?}"),
        }
    }

    #[test]
    fn editor_not_converged_blocks_even_when_committed() {
        // postcommit_worktree_check match=false must block dispatch, not proceed.
        let facts = ConvergenceFacts {
            editor_converged: false,
            elapsed_ms: 0,
            ..quiescent_facts()
        };
        assert!(matches!(
            convergence_gate_decision(&facts),
            ConvergenceGateDecision::Defer { .. }
        ));
        assert_eq!(facts.unmet_proofs(), vec![proof::EDITOR_CONVERGED]);
    }

    #[test]
    fn timeout_exceeded_while_unconverged_trips_force_disk_fallback() {
        let facts = ConvergenceFacts {
            editor_converged: false,
            inflight: 5,
            elapsed_ms: 5_000,
            timeout_ms: 5_000,
            ..quiescent_facts()
        };
        match convergence_gate_decision(&facts) {
            ConvergenceGateDecision::ForceDiskFallback { unmet } => {
                assert_eq!(unmet, vec![proof::EDITOR_CONVERGED, proof::INFLIGHT_DRAINED]);
            }
            other => panic!("expected ForceDiskFallback at timeout, got {other:?}"),
        }
    }

    #[test]
    fn quiescent_at_timeout_still_dispatches() {
        // Reaching convergence exactly at the deadline dispatches — never a
        // needless force-disk fallback when the editor actually converged.
        let facts = ConvergenceFacts {
            elapsed_ms: 9_999,
            timeout_ms: 5_000,
            ..quiescent_facts()
        };
        assert_eq!(
            convergence_gate_decision(&facts),
            ConvergenceGateDecision::Dispatch
        );
    }

    #[test]
    fn zero_timeout_defers_indefinitely_no_force_disk() {
        // timeout_ms == 0 opts out of the force-disk escape hatch: an unconverged
        // boundary keeps deferring rather than ever forcing a disk write.
        let facts = ConvergenceFacts {
            committed: false,
            elapsed_ms: 1_000_000,
            timeout_ms: 0,
            ..quiescent_facts()
        };
        assert!(matches!(
            convergence_gate_decision(&facts),
            ConvergenceGateDecision::Defer { .. }
        ));
    }

    #[test]
    fn unmet_proofs_listed_in_stable_order() {
        let facts = ConvergenceFacts {
            committed: false,
            editor_converged: false,
            inflight: 3,
            actor_idle: false,
            elapsed_ms: 0,
            timeout_ms: 5_000,
        };
        assert_eq!(
            facts.unmet_proofs(),
            vec![
                proof::COMMITTED,
                proof::EDITOR_CONVERGED,
                proof::INFLIGHT_DRAINED,
                proof::ACTOR_IDLE,
            ]
        );
    }
}
