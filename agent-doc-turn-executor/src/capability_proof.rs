//! Managed capability-proof policy for turn executors.
//!
//! The concrete probe runs in orchestration because it launches child
//! processes. This module owns the pure retry/timeout policy and the back-off
//! schedule used to decide when a turn executor remains gated or gives up.

use std::time::Duration;

/// Default number of managed-capability-proof attempts (the failing attempt
/// plus retries) before the dispatch gate commits to `Failed`.
pub const DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS: u32 = 3;
/// Default base back-off between managed-capability-proof retries.
pub const DEFAULT_MANAGED_PROOF_RETRY_BACKOFF: Duration = Duration::from_secs(2);
/// Default managed-capability child probe timeout.
pub const DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT: Duration = Duration::from_secs(45);
/// Upper bound on a single retry back-off so exponential growth stays bounded.
pub const MAX_MANAGED_PROOF_BACKOFF: Duration = Duration::from_secs(30);

/// Resolved managed-capability-proof retry/timeout policy.
///
/// The proof runs a network/SSH/write-root probe in a background supervisor
/// thread. This policy bounds retries so a short transient failure can
/// self-heal instead of wedging dispatch permanently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedProofPolicy {
    /// Total attempts (1 = no retry).
    pub max_attempts: u32,
    /// Base back-off between retries (grows exponentially, capped).
    pub base_backoff: Duration,
    /// Child probe timeout.
    pub probe_timeout: Duration,
}

impl Default for ManagedProofPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS,
            base_backoff: DEFAULT_MANAGED_PROOF_RETRY_BACKOFF,
            probe_timeout: DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagedProofPolicyInputs {
    pub frontmatter_max_attempts: Option<u32>,
    pub config_max_attempts: Option<u32>,
    pub frontmatter_retry_backoff_secs: Option<u64>,
    pub config_retry_backoff_secs: Option<u64>,
    pub frontmatter_probe_timeout_secs: Option<u64>,
    pub config_probe_timeout_secs: Option<u64>,
}

/// Resolve the managed-capability-proof policy, preferring document
/// frontmatter facts over project/global config facts, then built-in defaults.
pub fn resolve_managed_proof_policy(inputs: ManagedProofPolicyInputs) -> ManagedProofPolicy {
    let max_attempts = inputs
        .frontmatter_max_attempts
        .or(inputs.config_max_attempts)
        .map(|n| n.max(1))
        .unwrap_or(DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS);
    let base_backoff = inputs
        .frontmatter_retry_backoff_secs
        .or(inputs.config_retry_backoff_secs)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_MANAGED_PROOF_RETRY_BACKOFF);
    let probe_timeout = inputs
        .frontmatter_probe_timeout_secs
        .or(inputs.config_probe_timeout_secs)
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    ManagedProofPolicy {
        max_attempts,
        base_backoff,
        probe_timeout,
    }
}

/// Outcome of [`proof_retry_decision`]: either retry after a back-off or commit
/// to a permanent `Failed` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofRetryDecision {
    /// Retry the probe after sleeping `backoff`.
    Retry { backoff: Duration },
    /// Stop retrying; set the dispatch gate to `Failed`.
    GiveUp,
}

/// Decide whether a failed managed-capability-proof attempt should be retried.
///
/// `attempt` is the 1-based count of attempts already made, including the one
/// that just failed. Back-off grows exponentially from `base_backoff` and is
/// capped at [`MAX_MANAGED_PROOF_BACKOFF`].
pub fn proof_retry_decision(
    attempt: u32,
    max_attempts: u32,
    base_backoff: Duration,
) -> ProofRetryDecision {
    if attempt >= max_attempts.max(1) {
        return ProofRetryDecision::GiveUp;
    }
    let shift = attempt.saturating_sub(1).min(5);
    let multiplier = 1u32 << shift;
    let backoff = base_backoff
        .saturating_mul(multiplier)
        .min(MAX_MANAGED_PROOF_BACKOFF);
    ProofRetryDecision::Retry { backoff }
}

pub fn managed_capability_proof_status_message(harness_binary: &str, event: &str) -> String {
    format!("[start] managed {harness_binary} capability proof: {event}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_managed_proof_policy_uses_defaults_when_unset() {
        let policy = resolve_managed_proof_policy(ManagedProofPolicyInputs::default());
        assert_eq!(policy.max_attempts, DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS);
        assert_eq!(policy.base_backoff, DEFAULT_MANAGED_PROOF_RETRY_BACKOFF);
        assert_eq!(policy.probe_timeout, DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    }

    #[test]
    fn resolve_managed_proof_policy_prefers_frontmatter_over_config() {
        let policy = resolve_managed_proof_policy(ManagedProofPolicyInputs {
            frontmatter_max_attempts: Some(5),
            config_max_attempts: Some(2),
            frontmatter_retry_backoff_secs: Some(3),
            config_retry_backoff_secs: Some(1),
            frontmatter_probe_timeout_secs: Some(90),
            config_probe_timeout_secs: Some(10),
        });
        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.base_backoff, Duration::from_secs(3));
        assert_eq!(policy.probe_timeout, Duration::from_secs(90));
    }

    #[test]
    fn resolve_managed_proof_policy_clamps_and_falls_back_to_config() {
        let policy = resolve_managed_proof_policy(ManagedProofPolicyInputs {
            config_max_attempts: Some(0),
            config_probe_timeout_secs: Some(0),
            ..ManagedProofPolicyInputs::default()
        });
        assert_eq!(policy.max_attempts, 1);
        assert_eq!(policy.probe_timeout, DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    }

    #[test]
    fn proof_retry_decision_retries_with_exponential_backoff() {
        let base = Duration::from_secs(2);
        assert_eq!(
            proof_retry_decision(1, 3, base),
            ProofRetryDecision::Retry {
                backoff: Duration::from_secs(2)
            }
        );
        assert_eq!(
            proof_retry_decision(2, 3, base),
            ProofRetryDecision::Retry {
                backoff: Duration::from_secs(4)
            }
        );
    }

    #[test]
    fn proof_retry_decision_gives_up_at_budget() {
        let base = Duration::from_secs(2);
        assert_eq!(proof_retry_decision(3, 3, base), ProofRetryDecision::GiveUp);
        assert_eq!(proof_retry_decision(1, 1, base), ProofRetryDecision::GiveUp);
    }

    #[test]
    fn proof_retry_decision_caps_backoff() {
        let base = Duration::from_secs(20);
        assert_eq!(
            proof_retry_decision(2, 10, base),
            ProofRetryDecision::Retry {
                backoff: MAX_MANAGED_PROOF_BACKOFF
            }
        );
    }

    #[test]
    fn managed_capability_proof_status_message_names_harness() {
        let message = managed_capability_proof_status_message(
            "opencode",
            "opencode_capability_proof status=proven network=proven",
        );

        assert_eq!(
            message,
            "[start] managed opencode capability proof: opencode_capability_proof status=proven network=proven"
        );
    }
}
