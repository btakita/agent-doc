//! Foreground `sync_tmux_layout` when the command's own evidence has no column
//! geometry (GH #383).
//!
//! The decision is a pure function of what the controller has already retained.
//! It adopts an editor-ordered vector only when that vector is already
//! published and already contains the focus. It never reconstructs membership
//! or left-to-right order from a flat visible set.

use crate::FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY;

/// Install-and-restart remedy when a measured frontend generation was retained
/// and then retired without a live replacement. The lead clause is the
/// generation fault; the rest is the same two-sided ZIP recovery as an absent
/// frontend.
pub const FRONTEND_GENERATION_MISMATCHED_REMEDY: &str = concat!(
    "the frontend generation was not accepted; ",
    "install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module)",
);

/// Which blocked readiness a flat sync command can report after it has waited
/// for a published layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatSyncReadiness {
    /// No measured frontend client is retained.
    AuthorityAbsent,
    /// A measured client was retained and retired, and no live one replaced it.
    GenerationMismatched,
    /// A measured client is live and has not published an accepted vector that
    /// contains the focus.
    AwaitingPublishedLayout,
}

impl FlatSyncReadiness {
    pub fn reason(self) -> &'static str {
        match self {
            Self::AuthorityAbsent => "frontend_authority_absent",
            Self::GenerationMismatched => "frontend_generation_mismatched",
            Self::AwaitingPublishedLayout => "frontend_connected_without_accepted_publication",
        }
    }

    pub fn publisher(self) -> &'static str {
        match self {
            Self::AuthorityAbsent => "unavailable",
            Self::GenerationMismatched => "stale",
            Self::AwaitingPublishedLayout => "connected",
        }
    }

    /// Install-and-restart text. A connected frontend that has not published is
    /// a retry, so it has no remedy.
    pub fn remedy(self) -> Option<&'static str> {
        match self {
            Self::AuthorityAbsent => Some(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY),
            Self::GenerationMismatched => Some(FRONTEND_GENERATION_MISMATCHED_REMEDY),
            Self::AwaitingPublishedLayout => None,
        }
    }
}

/// Facts the controller observed before it asks for a flat-sync decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatSyncAuthorityEvidence {
    /// A live, non-retired, editor-ordered surface already contains the focus.
    pub published_layout_contains_focus: bool,
    /// Some non-retired measured (`jetbrains-rd`) client is connected.
    pub frontend_client_live: bool,
    /// Some measured client is retired and none is live.
    pub frontend_generation_stale: bool,
}

/// Adopt the published vector, or defer with one readiness case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatSyncDecision {
    AdoptPublishedLayout,
    Defer(FlatSyncReadiness),
}

/// Total decision. A live measured client wins over a retired generation. A
/// published editor vector that already contains the focus wins over both.
pub fn decide_flat_sync(evidence: FlatSyncAuthorityEvidence) -> FlatSyncDecision {
    if evidence.published_layout_contains_focus {
        FlatSyncDecision::AdoptPublishedLayout
    } else if evidence.frontend_client_live {
        FlatSyncDecision::Defer(FlatSyncReadiness::AwaitingPublishedLayout)
    } else if evidence.frontend_generation_stale {
        FlatSyncDecision::Defer(FlatSyncReadiness::GenerationMismatched)
    } else {
        FlatSyncDecision::Defer(FlatSyncReadiness::AuthorityAbsent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(published: bool, live: bool, stale: bool) -> FlatSyncAuthorityEvidence {
        FlatSyncAuthorityEvidence {
            published_layout_contains_focus: published,
            frontend_client_live: live,
            frontend_generation_stale: stale,
        }
    }

    #[test]
    fn decide_flat_sync_adopts_a_published_layout_that_contains_the_focus() {
        for (live, stale) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                decide_flat_sync(evidence(true, live, stale)),
                FlatSyncDecision::AdoptPublishedLayout,
                "live={live} stale={stale}"
            );
        }
    }

    #[test]
    fn decide_flat_sync_names_authority_absent_when_no_measured_client_exists() {
        let decision = decide_flat_sync(evidence(false, false, false));
        assert_eq!(
            decision,
            FlatSyncDecision::Defer(FlatSyncReadiness::AuthorityAbsent)
        );
        let FlatSyncDecision::Defer(readiness) = decision else {
            unreachable!();
        };
        assert_eq!(readiness.reason(), "frontend_authority_absent");
        assert_eq!(readiness.publisher(), "unavailable");
        assert_eq!(
            readiness.remedy(),
            Some(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY)
        );
    }

    #[test]
    fn decide_flat_sync_names_a_retired_generation_separately_from_absence() {
        let decision = decide_flat_sync(evidence(false, false, true));
        assert_eq!(
            decision,
            FlatSyncDecision::Defer(FlatSyncReadiness::GenerationMismatched)
        );
        let FlatSyncDecision::Defer(readiness) = decision else {
            unreachable!();
        };
        assert_eq!(readiness.reason(), "frontend_generation_mismatched");
        assert_eq!(readiness.publisher(), "stale");
        let remedy = readiness
            .remedy()
            .expect("generation mismatch has a remedy");
        assert!(
            remedy.starts_with("the frontend generation was not accepted"),
            "{remedy}"
        );
        assert!(remedy.contains(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY));
    }

    #[test]
    fn decide_flat_sync_a_live_frontend_without_a_layout_is_a_retry() {
        for stale in [false, true] {
            let decision = decide_flat_sync(evidence(false, true, stale));
            assert_eq!(
                decision,
                FlatSyncDecision::Defer(FlatSyncReadiness::AwaitingPublishedLayout),
                "a live client wins over stale={stale}"
            );
        }
        let FlatSyncDecision::Defer(readiness) = decide_flat_sync(evidence(false, true, false))
        else {
            unreachable!();
        };
        assert_eq!(
            readiness.reason(),
            "frontend_connected_without_accepted_publication"
        );
        assert_eq!(readiness.publisher(), "connected");
        assert_eq!(readiness.remedy(), None);
    }
}
