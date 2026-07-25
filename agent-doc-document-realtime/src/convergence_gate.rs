//! `#fullboundary` — convergence-gated inter-queue-item boundary.
//!
//! This module is the pure decision core for realtime document convergence:
//! given the quiescence proofs for the prior turn's close plus the elapsed wait,
//! it decides whether to dispatch the next item, keep deferring, or fail closed
//! after the bounded timeout. It performs no I/O.

use serde::{Deserialize, Serialize};

/// The four quiescence proofs for the prior turn's close, plus the bounded-wait
/// clock. The next queue item dispatches only when all four proofs hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceFacts {
    /// Prior cycle reached `committed` (staged snapshot == `HEAD`).
    pub committed: bool,
    /// The live editor buffer is proven converged == `HEAD` via editor-IPC ACK.
    pub editor_converged: bool,
    /// This document's editor-IPC inflight count. Must have drained to 0.
    pub inflight: u64,
    /// The authoritative actor is idle: a dispatch-ready prompt with no active turn.
    pub actor_idle: bool,
    /// Milliseconds spent waiting on this gate for the current boundary.
    pub elapsed_ms: u64,
    /// Bounded timeout (ms). Past this, an unconverged boundary blocks dispatch
    /// loudly rather than silently waiting forever or routing through disk fallback.
    pub timeout_ms: u64,
}

/// Stable proof labels, used for `unmet` lists in diagnostics and playback.
pub mod proof {
    pub const COMMITTED: &str = "committed";
    pub const EDITOR_CONVERGED: &str = "editor_converged";
    pub const INFLIGHT_DRAINED: &str = "inflight_drained";
    pub const ACTOR_IDLE: &str = "actor_idle";
}

/// The boundary decision for dispatching the next queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvergenceGateDecision {
    /// All four proofs satisfied: dispatch item N+1 now.
    Dispatch,
    /// Not yet quiescent but still within the bounded timeout.
    Defer { unmet: Vec<&'static str> },
    /// The bounded timeout elapsed while still not converged.
    Blocked { unmet: Vec<&'static str> },
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
/// Quiescent => [`ConvergenceGateDecision::Dispatch`]. Otherwise, still within
/// the bounded timeout => [`ConvergenceGateDecision::Defer`]; at or past the
/// timeout => [`ConvergenceGateDecision::Blocked`]. A `timeout_ms` of 0 means an
/// unconverged boundary defers indefinitely.
pub fn convergence_gate_decision(facts: &ConvergenceFacts) -> ConvergenceGateDecision {
    if facts.is_quiescent() {
        return ConvergenceGateDecision::Dispatch;
    }
    let unmet = facts.unmet_proofs();
    if facts.timeout_ms > 0 && facts.elapsed_ms >= facts.timeout_ms {
        ConvergenceGateDecision::Blocked { unmet }
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
    fn timeout_exceeded_while_unconverged_blocks_dispatch() {
        let facts = ConvergenceFacts {
            editor_converged: false,
            inflight: 5,
            elapsed_ms: 5_000,
            timeout_ms: 5_000,
            ..quiescent_facts()
        };
        match convergence_gate_decision(&facts) {
            ConvergenceGateDecision::Blocked { unmet } => {
                assert_eq!(
                    unmet,
                    vec![proof::EDITOR_CONVERGED, proof::INFLIGHT_DRAINED]
                );
            }
            other => panic!("expected Blocked at timeout, got {other:?}"),
        }
    }

    #[test]
    fn quiescent_at_timeout_still_dispatches() {
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
    fn zero_timeout_defers_indefinitely_without_blocking_transition() {
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

/// `#crdtcasraced` — backoff policy for the CRDT write convergence loop.
///
/// The loop retries a write until the editor acknowledges convergence, backing
/// off exponentially between attempts. It used to reset the backoff to its floor
/// on *every* non-converged apply, on the theory that an applied write is
/// progress. It is not: a write that keeps applying against a frontier that keeps
/// moving (a live editor typing, or two writers contending) re-applies and races
/// forever, and because each pass reset the floor the loop never actually backed
/// off. Operator-reported symptom: a 60s convergence timeout with repeated
/// `compare_and_swap_raced` at `backoff_ms=25` — hundreds of full merge+CAS
/// attempts, each of which makes the contention it is retrying slightly worse.
///
/// Real progress is the canonical content actually changing. Reset the floor
/// only then; otherwise keep doubling toward the cap, so a genuinely contended
/// write degrades to a slow poll instead of a hot spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrdtWriteBackoff {
    pub initial_ms: u64,
    pub max_ms: u64,
}

impl CrdtWriteBackoff {
    pub const fn new(initial_ms: u64, max_ms: u64) -> Self {
        Self { initial_ms, max_ms }
    }

    /// Next backoff after an attempt that did not converge.
    ///
    /// `advanced` is whether this attempt moved the canonical content since the
    /// previous attempt (compare the relay write's content hash).
    pub fn next_ms(self, current_ms: u64, advanced: bool) -> u64 {
        if advanced {
            return self.initial_ms;
        }
        current_ms
            .saturating_mul(2)
            .clamp(self.initial_ms, self.max_ms)
    }
}

#[cfg(test)]
mod crdt_write_backoff_tests {
    use super::CrdtWriteBackoff;

    const POLICY: CrdtWriteBackoff = CrdtWriteBackoff::new(25, 250);

    #[test]
    fn stalled_writes_back_off_toward_the_cap() {
        // The regression: without this, every one of these stayed at 25ms.
        let mut ms = POLICY.initial_ms;
        let mut observed = vec![ms];
        for _ in 0..6 {
            ms = POLICY.next_ms(ms, false);
            observed.push(ms);
        }
        assert_eq!(observed, vec![25, 50, 100, 200, 250, 250, 250]);
    }

    #[test]
    fn real_progress_resets_the_floor() {
        let stalled = POLICY.next_ms(POLICY.next_ms(25, false), false);
        assert_eq!(stalled, 100);
        assert_eq!(POLICY.next_ms(stalled, true), 25);
    }

    /// A contended 60s window must cost tens of attempts, not hundreds. This is
    /// the whole point of the change, so pin it as a budget rather than trusting
    /// the shape of the curve.
    #[test]
    fn a_fully_stalled_window_is_bounded_to_a_slow_poll() {
        let mut elapsed = 0u64;
        let mut ms = POLICY.initial_ms;
        let mut attempts = 0u32;
        while elapsed < 60_000 {
            elapsed += ms;
            attempts += 1;
            ms = POLICY.next_ms(ms, false);
        }
        assert!(
            (240..=250).contains(&attempts),
            "stalled 60s window should settle into a ~250ms poll, got {attempts} attempts"
        );
        // Contrast: the old always-reset behaviour was 60_000 / 25 = 2400.
        assert!(attempts * 4 < 2_400 + attempts);
    }
}
