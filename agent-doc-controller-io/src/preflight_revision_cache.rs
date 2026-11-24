//! Per-revision controller cache for preflight's `repair` and
//! `stuck_cycle_detection` phases (`#q-1d2a127f32aa`, preflightlag goal 3).
//!
//! ## Spec
//! - Live haiven-dev `tasks/sdk/sdk-dev.md` preflights spent 4-10 s in
//!   `repair` and 2-6 s in `stuck_cycle_detection`. Neither phase usually has
//!   work: the previous cycle closed cleanly and its response is in HEAD. They
//!   still paid for proving it. Each check is a projection read (a controller
//!   RPC while a controller runs, a ~165 ms cold `state.db` replay otherwise),
//!   and the stuck check also read `git show HEAD:` and searched it. One
//!   preflight made about eight of these reads, each waiting for a controller
//!   request slot.
//! - The controller already holds the document's live projection. One
//!   read-only RPC, `preflight_revision_verdicts`, answers both verdicts from
//!   it under one memory lock:
//!   - repair is clean when no cycle is open and no document write, retained
//!     write journal or compact projection is pending, exactly the states in
//!     which `enforce_cycle_completion` has nothing to recover;
//!   - the stuck sweep is quiet when no committed capture awaits HEAD. That
//!     verdict needs a `rev-parse` (and a `git show` while a capture awaits
//!     HEAD). It comes from this cache, keyed by the closeout projection's
//!     content revision and the commit it was checked against.
//! - The cache is edit-driven controller work. Each closeout (the controller's
//!   continuation-reconcile notify) schedules a refresh on the controller's
//!   per-document worker
//!   ([`crate::document_maintenance::preflight_revision_cache`]), which
//!   computes the stuck verdict from the live projection and the current HEAD
//!   and records it here.
//! - A read that finds no verdict for the revision it just read computes it
//!   from that same closeout clone and records it. The notify fires before the
//!   closeout's last projection writes, so the refresh can record an
//!   intermediate revision; a refresh the miss scheduled would be debounced
//!   past the preflight's own next read (CI 45328547184).
//! - A revision is the SHA-256 of the closeout projection's JSON, so any
//!   closeout fact changes it. Nothing else feeds the stuck verdict except the
//!   commit, which the caller compares itself.
//!
//! ## Agentic Contracts
//! - A verdict never claims more than the live projection proves: a cycle
//!   state that cannot be reconstructed is not clean, and a stuck verdict for
//!   another revision or commit is not returned as current.
//! - The RPC never writes state and never waits for a worker; it runs git
//!   only to compute a verdict missing for the revision it read.
//! - With no controller, an older one, or any error, preflight runs both
//!   phases inline exactly as before.
//!
//! ## Evals
//! - `a_closed_cycle_with_nothing_pending_is_clean`
//! - `an_open_cycle_or_pending_write_is_not_clean`
//! - `a_stuck_verdict_is_current_only_for_its_revision`
//! - `the_closeout_revision_changes_with_any_closeout_fact`

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// The controller command answering [`PreflightRevisionVerdicts`].
pub const PREFLIGHT_REVISION_VERDICTS_COMMAND: &str = "preflight_revision_verdicts";

/// The stuck-sweep verdict for one closeout revision against one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StuckSweepVerdict {
    /// The commit the verdict was checked against.
    pub head_oid: String,
    /// `true` when the sweep has nothing to settle or report.
    pub quiet: bool,
}

/// What the controller's live projection says about a document's preflight
/// `repair` and `stuck_cycle_detection` phases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightRevisionVerdicts {
    /// SHA-256 of the closeout projection's JSON these verdicts were read from.
    pub closeout_revision: String,
    /// `None` when the cycle state could not be reconstructed (never clean).
    pub cycle_open: Option<bool>,
    pub pending_document_write: bool,
    pub pending_write_journal: bool,
    pub pending_compact_projection: bool,
    /// The cached stuck verdict for `closeout_revision`, when one was computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stuck: Option<StuckSweepVerdict>,
}

impl PreflightRevisionVerdicts {
    /// Whether preflight's cycle-completion recovery has nothing to do.
    pub fn repair_clean(&self) -> bool {
        self.cycle_open == Some(false)
            && !self.pending_document_write
            && !self.pending_write_journal
            && !self.pending_compact_projection
    }

    /// Whether the stuck sweep is known quiet at commit `head_oid`.
    pub fn stuck_quiet_at(&self, head_oid: &str) -> bool {
        self.stuck
            .as_ref()
            .is_some_and(|stuck| stuck.quiet && stuck.head_oid == head_oid)
    }
}

/// The content revision of a closeout projection.
pub fn closeout_revision(
    closeout: Option<&agent_doc_state_backbone::CloseoutProjection>,
) -> String {
    let json = closeout
        .map(|closeout| serde_json::to_string(closeout).unwrap_or_default())
        .unwrap_or_default();
    agent_doc_hash::content_hash(&json)
}

/// Derive the verdicts from one consistent read of the live projection.
pub fn verdicts_from_projection(
    closeout: Option<&agent_doc_state_backbone::CloseoutProjection>,
    document: Option<&agent_doc_state_backbone::DocumentProjection>,
    stuck: Option<StuckSweepVerdict>,
) -> PreflightRevisionVerdicts {
    let cycle_open = match closeout {
        None => Some(false),
        Some(closeout) => agent_doc_cycle_state_io::reconstruct_cycle_state_from_closeout(closeout)
            .ok()
            .map(|state| state.is_some_and(|state| state.is_open())),
    };
    PreflightRevisionVerdicts {
        closeout_revision: closeout_revision(closeout),
        cycle_open,
        pending_document_write: document.is_some_and(|document| document.pending_write.is_some()),
        pending_write_journal: document
            .is_some_and(|document| !document.pending_write_journal.is_empty()),
        pending_compact_projection: document
            .is_some_and(|document| document.pending_compact_projection.is_some()),
        stuck,
    }
}

#[derive(Debug, Clone)]
struct CachedStuck {
    closeout_revision: String,
    verdict: StuckSweepVerdict,
}

fn stuck_cache() -> &'static Mutex<HashMap<String, CachedStuck>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedStuck>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the stuck verdict a refresh computed for `document_hash` at
/// `closeout_revision`. A later revision replaces it.
pub fn record_stuck_verdict(
    document_hash: &str,
    closeout_revision: &str,
    verdict: StuckSweepVerdict,
) {
    stuck_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(
            document_hash.to_string(),
            CachedStuck {
                closeout_revision: closeout_revision.to_string(),
                verdict,
            },
        );
}

/// The cached stuck verdict for `document_hash`, only when it was computed for
/// `closeout_revision`.
pub fn cached_stuck_verdict(
    document_hash: &str,
    closeout_revision: &str,
) -> Option<StuckSweepVerdict> {
    stuck_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(document_hash)
        .filter(|cached| cached.closeout_revision == closeout_revision)
        .map(|cached| cached.verdict.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_state_backbone::{CloseoutProjection, DocumentProjection};

    fn closed_closeout() -> CloseoutProjection {
        CloseoutProjection {
            cycle_id: Some("cycle-1".to_string()),
            phase: Some(agent_doc_turn::CyclePhase::Committed),
            ..CloseoutProjection::default()
        }
    }

    #[test]
    fn a_closed_cycle_with_nothing_pending_is_clean() {
        let closeout = closed_closeout();
        let document = DocumentProjection::default();
        let verdicts = verdicts_from_projection(Some(&closeout), Some(&document), None);
        assert_eq!(verdicts.cycle_open, Some(false));
        assert!(verdicts.repair_clean());
        assert!(
            verdicts_from_projection(None, None, None).repair_clean(),
            "a document with no projection has nothing to recover"
        );
    }

    #[test]
    fn an_open_cycle_or_pending_write_is_not_clean() {
        // A real open cycle: the turn-intent checkpoint a preflight records.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("doc.md");
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, base).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let open = agent_doc_cycle_state_io::load_document_closeout_projection(&doc)
            .unwrap()
            .expect("an open cycle's closeout projection");
        let verdicts = verdicts_from_projection(Some(&open), None, None);
        assert_eq!(verdicts.cycle_open, Some(true));
        assert!(!verdicts.repair_clean());

        let closeout = closed_closeout();
        let clean = verdicts_from_projection(Some(&closeout), None, None);
        for pending in [
            PreflightRevisionVerdicts {
                pending_document_write: true,
                ..clean.clone()
            },
            PreflightRevisionVerdicts {
                pending_write_journal: true,
                ..clean.clone()
            },
            PreflightRevisionVerdicts {
                pending_compact_projection: true,
                ..clean.clone()
            },
        ] {
            assert!(!pending.repair_clean(), "{pending:?}");
        }

        let unreadable = PreflightRevisionVerdicts {
            cycle_open: None,
            ..verdicts_from_projection(Some(&closeout), None, None)
        };
        assert!(
            !unreadable.repair_clean(),
            "an unreadable cycle state is never clean"
        );
    }

    #[test]
    fn a_stuck_verdict_is_current_only_for_its_revision() {
        let hash = "doc-a_stuck_verdict_is_current_only_for_its_revision";
        let quiet = StuckSweepVerdict {
            head_oid: "abc".to_string(),
            quiet: true,
        };
        record_stuck_verdict(hash, "rev-1", quiet.clone());
        assert_eq!(cached_stuck_verdict(hash, "rev-1"), Some(quiet.clone()));
        assert_eq!(cached_stuck_verdict(hash, "rev-2"), None);

        let verdicts = verdicts_from_projection(None, None, Some(quiet));
        assert!(verdicts.stuck_quiet_at("abc"));
        assert!(
            !verdicts.stuck_quiet_at("def"),
            "a newer commit is checked again"
        );
    }

    #[test]
    fn the_closeout_revision_changes_with_any_closeout_fact() {
        let closeout = closed_closeout();
        let other = CloseoutProjection {
            capture_id: Some("capture-2".to_string()),
            ..closed_closeout()
        };
        assert_ne!(
            closeout_revision(Some(&closeout)),
            closeout_revision(Some(&other))
        );
        assert_eq!(
            closeout_revision(Some(&closeout)),
            closeout_revision(Some(&closeout.clone()))
        );
        assert_ne!(closeout_revision(None), closeout_revision(Some(&closeout)));
    }
}
