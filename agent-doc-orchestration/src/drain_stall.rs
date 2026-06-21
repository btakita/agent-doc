//! Binary-detected queue-stall guard (`#qstallguard` Layer B).
//!
//! The skill has pages of prose telling the agent not to stall a drainable
//! queue (`#drain-no-defer`, `#staleloop-recycle-restart`), yet a stall still
//! happened in the field: the binary said `queue_continuation_required=true`
//! with drainable heads and the agent stopped anyway, rationalizing a stop
//! reason from the item prose. Prose is advisory — an LLM can always synthesize
//! a plausible-sounding excuse. The asymmetry this module closes: the binary
//! computes `queue_continuation_required` authoritatively, but nothing *checked*
//! that the agent honored it.
//!
//! This is the check. At a clean closeout where continuation was required,
//! `session-check` drops a one-shot *continuation-pending marker* for the
//! document. At the NEXT preflight the marker is reconciled:
//!   - the loop continued (a fresh drain-owner lease is present) → clear, quiet;
//!   - a real user prompt preempted, the operator stopped the queue, or nothing
//!     drainable remains → a legitimate stop, clear, quiet;
//!   - otherwise (drainable work remained, no valid stop reason, the loop did
//!     not continue) → **stall**: emit a hard `queue_stall_detected` diagnostic.
//!
//! A red diagnostic is far harder to rationalize past than prose, and it is the
//! signal SimWorld asserts on — so the no-stall invariant survives refactors.
//!
//! The marker is a one-shot: every reconciliation clears it, so the diagnostic
//! fires once per stall rather than spamming. Mirrors the [`crate::recycle_yield`]
//! / [`crate::drain_owner`] sidecar layout (keyed on the document path).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document continuation
/// markers. Mirrors the sibling recycle-yield / drain-owner sidecar layout.
const DRAIN_STALL_DIR: &str = ".agent-doc/drain-stall";

/// The canonical `ops.log` / preflight-warning marker emitted on a detected stall.
pub const QUEUE_STALL_DETECTED: &str =
    "queue_stall_detected reason=no_valid_stop_with_continuation_required";

/// Persisted continuation-pending marker body. Written at a clean closeout that
/// required continuation; reconciled (and cleared) at the next preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationPending {
    /// The cycle id that committed with continuation still required.
    pub cycle_id: String,
    /// Unix seconds the marker was written.
    pub recorded_secs: u64,
}

/// Facts the stall classifier reasons over. Gathered at preflight from already-
/// computed signals — no new IO inside the classifier so it is pure + testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StallFacts {
    /// A continuation-pending marker from the prior clean closeout is present.
    pub continuation_pending_marker: bool,
    /// This preflight still computes `queue_continuation_required=true`.
    pub continuation_required_now: bool,
    /// Loop-scope drainable head count this preflight.
    pub drainable_head_count: usize,
    /// A real user prompt edited the in-scope exchange tail this turn
    /// (`user_intent_prompt_changes` non-empty) — a legitimate preemption.
    pub user_prompt_preempts: bool,
    /// The operator stopped the queue via the sanctioned mechanism
    /// (`queue: stop` frontmatter / `--- stop` fence).
    pub queue_stopped: bool,
    /// The loop is actively continuing — a fresh drain-owner lease is held, so
    /// this invocation IS the next drain iteration, not a stalled re-trigger.
    pub loop_is_continuing: bool,
}

/// Outcome of reconciling the continuation-pending marker against the current
/// preflight facts. Every non-`NoMarker` outcome means the caller clears the
/// one-shot marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StallVerdict {
    /// No prior continuation marker — nothing to reconcile.
    NoMarker,
    /// The loop continued (fresh drain lease) — healthy, clear the marker.
    LoopContinued,
    /// A legitimate stop (user preemption / queue stopped / nothing drainable)
    /// — clear the marker, no diagnostic.
    LegitimateStop,
    /// The drain stalled: drainable work remained, the loop did not continue,
    /// and no valid stop reason applied. Carries the diagnostic message.
    Stalled(String),
}

/// Pure stall classifier (`#qstallguard` Layer B). Side-effect free so it is
/// unit-testable without the filesystem; the caller owns marker IO.
///
/// The valid-stop set mirrors the skill's exhaustive loop skip-list: a real user
/// prompt, an operator-set `queue: stop`, or a drained queue. A degraded/stale
/// supervisor, high accretion, or a `semantic_completion_match` warning are NOT
/// valid stop reasons and intentionally do not suppress the diagnostic.
pub fn classify_stall(facts: &StallFacts) -> StallVerdict {
    if !facts.continuation_pending_marker {
        return StallVerdict::NoMarker;
    }
    if facts.loop_is_continuing {
        return StallVerdict::LoopContinued;
    }
    if facts.user_prompt_preempts
        || facts.queue_stopped
        || !facts.continuation_required_now
        || facts.drainable_head_count == 0
    {
        return StallVerdict::LegitimateStop;
    }
    StallVerdict::Stalled(format!(
        "{QUEUE_STALL_DETECTED} (prior cycle committed with {} drainable head(s) but the \
         loop neither continued nor recorded a valid stop reason — a real user prompt, \
         `queue: stop`, or a drained queue. A degraded/stale supervisor, high accretion, \
         or a semantic_completion_match warning are NOT valid stop reasons.)",
        facts.drainable_head_count
    ))
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the continuation-marker path for a document. Mirrors
/// [`crate::recycle_yield`]: hash the document path and land the sidecar in the
/// nearest ancestor `.agent-doc/` directory.
fn marker_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(DRAIN_STALL_DIR).join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(DRAIN_STALL_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Record (at a clean closeout) that this committed cycle still required queue
/// continuation. Idempotent; the next preflight reconciles + clears it.
pub fn mark_continuation_pending(file: &str, cycle_id: &str) -> Result<()> {
    let path = marker_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create drain-stall dir {}", parent.display()))?;
    }
    let marker = ContinuationPending {
        cycle_id: cycle_id.to_string(),
        recorded_secs: now_secs(),
    };
    let body = serde_json::to_string(&marker).context("failed to serialize drain-stall marker")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write drain-stall marker {}", path.display()))?;
    Ok(())
}

/// Read the raw continuation-pending marker for `file`, if present.
pub fn read_continuation_pending(file: &str) -> Option<ContinuationPending> {
    let path = marker_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Convenience boolean: is a continuation-pending marker present for `file`?
pub fn continuation_pending(file: &Path) -> bool {
    read_continuation_pending(&file.to_string_lossy()).is_some()
}

/// Best-effort one-shot clear of the continuation-pending marker. Called after
/// every reconciliation so the diagnostic fires once per stall.
pub fn clear_continuation_pending(file: &str) {
    let path = marker_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear drain-stall marker {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> StallFacts {
        StallFacts {
            continuation_pending_marker: true,
            continuation_required_now: true,
            drainable_head_count: 1,
            user_prompt_preempts: false,
            queue_stopped: false,
            loop_is_continuing: false,
        }
    }

    #[test]
    fn stall_fires_when_drainable_work_remained_and_loop_did_not_continue() {
        // The canonical field failure: prior cycle required continuation, work
        // remained, the loop did not continue, and there was no valid stop reason.
        match classify_stall(&base()) {
            StallVerdict::Stalled(msg) => {
                assert!(msg.contains("queue_stall_detected"));
                assert!(msg.contains("no_valid_stop_with_continuation_required"));
            }
            other => panic!("expected Stalled, got {other:?}"),
        }
    }

    #[test]
    fn no_marker_is_inert() {
        let facts = StallFacts {
            continuation_pending_marker: false,
            ..base()
        };
        assert_eq!(classify_stall(&facts), StallVerdict::NoMarker);
    }

    #[test]
    fn loop_continuation_clears_without_diagnostic() {
        let facts = StallFacts {
            loop_is_continuing: true,
            ..base()
        };
        assert_eq!(classify_stall(&facts), StallVerdict::LoopContinued);
    }

    #[test]
    fn each_valid_stop_reason_suppresses_the_diagnostic() {
        // A real user prompt preempting the loop.
        assert_eq!(
            classify_stall(&StallFacts {
                user_prompt_preempts: true,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
        // Operator stopped the queue the sanctioned way.
        assert_eq!(
            classify_stall(&StallFacts {
                queue_stopped: true,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
        // Nothing drainable remains.
        assert_eq!(
            classify_stall(&StallFacts {
                drainable_head_count: 0,
                continuation_required_now: false,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
    }

    #[test]
    fn degraded_supervisor_is_not_a_valid_stop_reason() {
        // There is deliberately no `supervisor_stale` / `high_accretion` field:
        // those are NOT valid stop reasons, so a stall with drainable work still
        // fires regardless of supervisor health. This test documents that intent.
        let facts = base();
        assert!(matches!(classify_stall(&facts), StallVerdict::Stalled(_)));
    }

    #[test]
    fn marker_roundtrips_and_clears_one_shot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        assert!(read_continuation_pending(&file).is_none());
        mark_continuation_pending(&file, "cycle-123").unwrap();
        let marker = read_continuation_pending(&file).expect("marker present");
        assert_eq!(marker.cycle_id, "cycle-123");

        clear_continuation_pending(&file);
        assert!(read_continuation_pending(&file).is_none());
        // Idempotent clear must not panic / warn-fail.
        clear_continuation_pending(&file);
    }
}
