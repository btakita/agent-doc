//! Pure read-authority recovery policy.
//!
//! I/O adapters classify their concrete relay result into this vocabulary, ask
//! this module for the next transition, and then apply the selected effect. Disk
//! reads, plugin refreshes, sleeps, and logging remain at the adapter boundary.

/// The authority observation shared by realtime and preflight read adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityObservation {
    Current,
    Detached,
    MissingReplica,
    SyncPending,
    Error,
}

/// Facts needed to select the next read-authority recovery transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityRecoveryFacts {
    pub observation: AuthorityObservation,
    pub editor_open: bool,
    pub retries_remaining: bool,
    /// Some adapters own a distinct model-rebuild effect after their bounded
    /// retry loop. Others already performed that work and must not repeat it.
    pub rebuild_after_retry_exhaustion: bool,
}

/// The effect an I/O adapter must apply next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityRecoveryDecision {
    AcceptCurrent,
    Retry { request_plugin_refresh: bool },
    RebuildFromPlugin,
    DescendToDisk,
    FailClosed,
}

/// Decide the next read-authority transition.
///
/// The load-bearing invariant is that disk is reachable only after the editor
/// is proven detached. An attached editor with an unavailable model retries,
/// rebuilds, or fails closed; it never silently adopts disk as current text.
pub const fn decide_authority_recovery(facts: AuthorityRecoveryFacts) -> AuthorityRecoveryDecision {
    use AuthorityObservation::{Current, Detached, Error, MissingReplica, SyncPending};
    use AuthorityRecoveryDecision::{
        AcceptCurrent, DescendToDisk, FailClosed, RebuildFromPlugin, Retry,
    };

    match facts.observation {
        Current => AcceptCurrent,
        Detached => DescendToDisk,
        MissingReplica | SyncPending if facts.retries_remaining => Retry {
            request_plugin_refresh: matches!(facts.observation, MissingReplica),
        },
        MissingReplica | SyncPending
            if facts.editor_open && facts.rebuild_after_retry_exhaustion =>
        {
            RebuildFromPlugin
        }
        MissingReplica | SyncPending | Error if facts.editor_open => FailClosed,
        MissingReplica | SyncPending | Error => DescendToDisk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(
        observation: AuthorityObservation,
        editor_open: bool,
        retries_remaining: bool,
        rebuild_after_retry_exhaustion: bool,
    ) -> AuthorityRecoveryDecision {
        decide_authority_recovery(AuthorityRecoveryFacts {
            observation,
            editor_open,
            retries_remaining,
            rebuild_after_retry_exhaustion,
        })
    }

    #[test]
    fn current_is_accepted_and_detached_descends() {
        assert_eq!(
            decide(AuthorityObservation::Current, true, true, true),
            AuthorityRecoveryDecision::AcceptCurrent
        );
        assert_eq!(
            decide(AuthorityObservation::Detached, false, true, true),
            AuthorityRecoveryDecision::DescendToDisk
        );
    }

    #[test]
    fn bounded_retries_refresh_only_a_missing_replica() {
        assert_eq!(
            decide(AuthorityObservation::MissingReplica, true, true, false),
            AuthorityRecoveryDecision::Retry {
                request_plugin_refresh: true,
            }
        );
        assert_eq!(
            decide(AuthorityObservation::SyncPending, true, true, false),
            AuthorityRecoveryDecision::Retry {
                request_plugin_refresh: false,
            }
        );
    }

    #[test]
    fn attached_transients_rebuild_or_fail_closed_after_retries() {
        for observation in [
            AuthorityObservation::MissingReplica,
            AuthorityObservation::SyncPending,
        ] {
            assert_eq!(
                decide(observation, true, false, true),
                AuthorityRecoveryDecision::RebuildFromPlugin
            );
            assert_eq!(
                decide(observation, true, false, false),
                AuthorityRecoveryDecision::FailClosed
            );
        }
    }

    #[test]
    fn unavailable_authority_descends_only_after_editor_detaches() {
        for observation in [
            AuthorityObservation::MissingReplica,
            AuthorityObservation::SyncPending,
            AuthorityObservation::Error,
        ] {
            assert_eq!(
                decide(observation, false, false, false),
                AuthorityRecoveryDecision::DescendToDisk
            );
        }
        assert_eq!(
            decide(AuthorityObservation::Error, true, false, false),
            AuthorityRecoveryDecision::FailClosed
        );
    }
}
