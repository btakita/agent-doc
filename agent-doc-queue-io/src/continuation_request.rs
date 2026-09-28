//! Run-keyed ledger of the last continuation a Stop hook asked for.
//!
//! # Why this is not a field on `ContinuationMarker`
//!
//! `#stopneedsclosedcycle` bounded Stop-hook recursion across turns by comparing
//! the current queue head against `ContinuationMarker::last_requested_head`.
//! The comparison is sound; its *storage* was not. The marker belongs to queue
//! reconciliation, which creates and deletes it on its own schedule, so the
//! recursion bound inherited a lifetime that has nothing to do with the hook:
//!
//! * `record_continuation_requested_head` documented itself as a "no-op when no
//!   marker exists", and the hook can legitimately reach its block with no
//!   marker at all — `continuation_proven` is satisfied by the marker **or** by
//!   the drain-stall projection. On that branch the guard had nothing to read
//!   and the arming write silently did nothing, so the bound was not merely
//!   weak, it was absent.
//! * `clear_continuation_marker` deletes the field along with the marker, so a
//!   reconcile between two stops disarms a guard that had been armed.
//!
//! Measured on `tasks/agent-doc/agent-doc-bugs.md` 2026-09-28, 24 blocks against
//! 2 skips, every repeat inside a single run:
//!
//! ```text
//! 00:03:27 head=62 turn=cycle-1790552355729   block
//! 00:05:07 head=62 turn=cycle-1790552355729   repeat
//! 00:30:47 head=26 turn=cycle-1790552355729   block   <- head moved, no run ran
//! 00:31:19 head=26 turn=cycle-1790552355729   repeat
//! 01:12:56 head=22 turn=cycle-1790556757989   block
//! 01:24:54 head=22 turn=cycle-1790556757989   block   <- guard had nothing to read
//! 01:33:18 head=22 turn=cycle-1790556757989   block
//! 01:34:02 head=22 turn=cycle-1790556757989   skip    <- only once a marker existed
//! ```
//!
//! # Reconcile against the current run
//!
//! The 00:30:47 line is the one head-equality can never catch: the head advanced
//! 62 -> 26 while `turn` stayed on `cycle-1790552355729`. A head can change
//! because the operator edited the document, because a reconcile rewrote the
//! queue, or because a malformed head was reparsed — none of which is drain
//! progress. Only a **new completed run** is.
//!
//! So the request is keyed to the run that produced it. A second request against
//! the same run asks a run that has already ended to do work it did not do, and
//! no amount of asking can change its outcome. Head equality is kept as the
//! second clause: it catches the run that did complete and still struck nothing
//! (`#qchurn`), which is the case `#stopneedsclosedcycle` was written for.
//!
//! Modelled in `formal/tla/StopHookContinuation.tla`, whose wedge config holds
//! the guard in the marker and must violate `AtMostOneRequestPerRun`.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// State-ledger kind. Deliberately distinct from the continuation marker's
/// `continuation`, so the marker's lifecycle cannot reach this row.
const CONTINUATION_REQUEST_STATE_KIND: &str = "continuation_request";

/// The last continuation a Stop hook asked the loop to take, and the run it
/// asked on behalf of.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuationRequest {
    /// Identity of the completed run (cycle) that was current when the request
    /// was made. `None` records "this document had no cycle state", which is
    /// itself a run identity: nothing ran, so nothing can have drained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub head_prompt: String,
    pub requested_at_secs: u64,
}

/// Why a repeated continuation request cannot make progress.
///
/// Two distinct proofs, kept distinct because they answer different questions in
/// a diagnostic: one says the loop never got a turn, the other says it got one
/// and the head survived it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonAdvancingContinuation {
    /// No run has completed since the last request. The run being asked has
    /// already ended, so re-asking cannot change what it drained.
    RunDidNotAdvance,
    /// A run completed and the head is still the same. The drain did not strike
    /// it — a malformed head that its own reap could not match, or an admission
    /// that never opened a cycle (`#qchurn`).
    HeadDidNotAdvance,
}

impl NonAdvancingContinuation {
    pub fn token(self) -> &'static str {
        match self {
            Self::RunDidNotAdvance => "run_did_not_advance",
            Self::HeadDidNotAdvance => "head_did_not_advance",
        }
    }
}

/// Whether requesting `head_prompt` for `run_id` would repeat a request that has
/// already been proven not to advance.
///
/// Pure, so both the decision and its diagnostic token come from one evaluation
/// and cannot drift apart.
pub fn non_advancing_continuation(
    previous: Option<&ContinuationRequest>,
    run_id: Option<&str>,
    head_prompt: &str,
) -> Option<NonAdvancingContinuation> {
    let previous = previous?;
    if previous.run_id.as_deref() == run_id {
        return Some(NonAdvancingContinuation::RunDidNotAdvance);
    }
    if previous.head_prompt == head_prompt {
        return Some(NonAdvancingContinuation::HeadDidNotAdvance);
    }
    None
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn state_identity(file: &Path) -> Result<Option<(std::path::PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

pub fn load_continuation_request(file: &Path) -> Result<Option<ContinuationRequest>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        CONTINUATION_REQUEST_STATE_KIND,
    )?
    else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&record.payload_json).ok())
}

/// Arm the recursion bound for `run_id` / `head_prompt`.
///
/// Unconditional by design: the previous implementation required a continuation
/// marker to already exist and returned `Ok(())` when one did not, which is the
/// exact branch the hook reaches when the drain-stall projection is what proved
/// the continuation. An arming write that can decline to arm is not a bound.
pub fn record_continuation_request(
    file: &Path,
    run_id: Option<&str>,
    head_prompt: &str,
) -> Result<()> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(());
    };
    let request = ContinuationRequest {
        run_id: run_id.map(str::to_string),
        head_prompt: head_prompt.to_string(),
        requested_at_secs: now_secs(),
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        &conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: CONTINUATION_REQUEST_STATE_KIND.to_string(),
            canonical_path,
            payload_json: serde_json::to_string(&request)
                .context("serialize continuation request")?,
            updated_at_secs: now_secs(),
        },
    )
}

/// Drop the recursion bound once the document no longer owes a continuation.
///
/// Called where the continuation marker is cleared: a drained queue is the
/// reconciliation point at which a later request is about genuinely new work
/// rather than a repeat, so keeping the old request would latch the guard shut.
pub fn clear_continuation_request(file: &Path) -> Result<()> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
        &conn,
        &document_hash,
        CONTINUATION_REQUEST_STATE_KIND,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ContinuationRequest, NonAdvancingContinuation, non_advancing_continuation,
    };

    fn request(run_id: Option<&str>, head: &str) -> ContinuationRequest {
        ContinuationRequest {
            run_id: run_id.map(str::to_string),
            head_prompt: head.to_string(),
            requested_at_secs: 0,
        }
    }

    /// Nothing has been asked yet, so nothing is a repeat.
    #[test]
    fn a_first_request_always_advances() {
        assert_eq!(non_advancing_continuation(None, Some("cycle-1"), "do [#a]"), None);
    }

    /// The production shape the shipped guard could not see: the head moved
    /// 62 -> 26 bytes inside `cycle-1790552355729` while no run completed. Head
    /// equality says "advanced"; the run says nothing ran.
    #[test]
    fn a_head_that_moves_without_a_run_is_still_non_advancing() {
        let previous = request(Some("cycle-1790552355729"), "do [#sixtytwobyteshead]");
        assert_eq!(
            non_advancing_continuation(
                Some(&previous),
                Some("cycle-1790552355729"),
                "do [#twentysix]"
            ),
            Some(NonAdvancingContinuation::RunDidNotAdvance)
        );
    }

    /// `#qchurn`: a run completed and the head survived it. Still a repeat, and
    /// still reported as the head clause rather than the run clause.
    #[test]
    fn a_run_that_completes_without_striking_the_head_is_non_advancing() {
        let previous = request(Some("cycle-1"), "do [#focusstashedactor");
        assert_eq!(
            non_advancing_continuation(
                Some(&previous),
                Some("cycle-2"),
                "do [#focusstashedactor"
            ),
            Some(NonAdvancingContinuation::HeadDidNotAdvance)
        );
    }

    /// The whole point of blocking: ordinary drain progress must still continue.
    #[test]
    fn a_new_run_with_a_new_head_advances() {
        let previous = request(Some("cycle-1"), "do [#a]");
        assert_eq!(
            non_advancing_continuation(Some(&previous), Some("cycle-2"), "do [#b]"),
            None
        );
    }

    /// A document with no cycle state has run nothing, so the absence of a run
    /// is itself a run identity and must compare equal to itself. Failing open
    /// here would restore the unbounded loop for exactly the documents that
    /// never manage to open a cycle.
    #[test]
    fn absent_cycle_state_is_a_run_identity_that_matches_itself() {
        let previous = request(None, "do [#a]");
        assert_eq!(
            non_advancing_continuation(Some(&previous), None, "do [#b]"),
            Some(NonAdvancingContinuation::RunDidNotAdvance)
        );
        // ... and a document that starts running is no longer in that state.
        assert_eq!(
            non_advancing_continuation(Some(&previous), Some("cycle-1"), "do [#b]"),
            None
        );
    }

    #[test]
    fn the_run_clause_is_reported_before_the_head_clause() {
        let previous = request(Some("cycle-1"), "do [#a]");
        assert_eq!(
            non_advancing_continuation(Some(&previous), Some("cycle-1"), "do [#a]"),
            Some(NonAdvancingContinuation::RunDidNotAdvance)
        );
    }

    #[test]
    fn tokens_are_distinct_and_stable() {
        assert_eq!(
            NonAdvancingContinuation::RunDidNotAdvance.token(),
            "run_did_not_advance"
        );
        assert_eq!(
            NonAdvancingContinuation::HeadDidNotAdvance.token(),
            "head_did_not_advance"
        );
    }
}
