//! `agent-doc turn-status active|idle` — surface a turn-in-progress status on the
//! agent's own tmux pane (`#claude-busy-status-during-active-turn`, continuous
//! monitor).
//!
//! Designed to be driven by harness turn-lifecycle hooks: a `UserPromptSubmit`
//! hook calls `active` when a turn starts and a `Stop` hook calls `idle` when it
//! ends. Because the hook runs INSIDE the agent's own pane, it sets that pane's
//! border title via `$TMUX_PANE` — no supervisor poll thread and no document/pane
//! resolution. Crucially the harness fires `Stop` only after the WHOLE turn
//! completes, including any Bash it auto-backgrounded and re-invoked on, so the
//! status covers the backgrounded window that pane busy-cue detection (which only
//! reads visible pane content) fundamentally cannot see.
//!
//! Visibility note: the status rides the pane border title, shown when tmux
//! `pane-border-status` is enabled. The command is best-effort — it never fails
//! the turn: outside tmux, or on any tmux error, it succeeds quietly.

use agent_doc_sqlite::state_store::{
    Connection, CoordinationLeaseRecord, clear_coordination_lease_if_heartbeat_at_or_before_in_db,
    clear_coordination_lease_if_holder_in_db, clear_coordination_lease_in_db,
    clear_coordination_leases_heartbeat_at_or_before_in_db, load_coordination_lease_from_db,
    load_coordination_leases_for_scope_kind_from_db, open_state_db,
    upsert_coordination_lease_in_db,
};
use agent_doc_turn::turn_status::{
    TRANSCRIPT_TAIL_PROBE_BYTES, TranscriptTailEvidence, TurnActiveMarker,
    classify_transcript_tail, pane_title_for_status, transcript_turn_end_reason,
    turn_active_expiry_cutoff, turn_active_marker_is_fresh, turn_active_marker_matches_pane,
    turn_lease_ended_by_interrupt, turn_lease_ended_by_withdrawn_prompt,
};
use anyhow::Result;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const TURN_ACTIVE_SCOPE: &str = "turn_active";
/// `#staleharnessturnlive`: the harness transcript named by the hook that wrote
/// a pane's turn-active lease. One row per pane, keyed like the lease; its
/// heartbeat equals the lease heartbeat it belongs to, so a binding left by an
/// older turn never vouches for a newer lease.
const TURN_ACTIVE_TRANSCRIPT_SCOPE: &str = "turn_active_transcript";
/// `#runctrlclaude`: durable per-pane harness turn-end receipt. `holder` is the
/// receipt reason, `heartbeat_secs` the boundary time (Unix seconds). It
/// outlives the lease row on purpose: the lease answers "is a turn live now?",
/// the receipt answers "did the harness reach a boundary after time T?".
const TURN_ENDED_SCOPE: &str = "turn_ended";
/// `#claimownerliveness`: the harness session id each pane's harness last
/// reported through a turn hook (`holder` = session id, `heartbeat_secs` =
/// when it was observed).
const PANE_HARNESS_SESSION_SCOPE: &str = "pane_harness_session";
const SUPERVISOR_STALE_SCOPE: &str = "supervisor_stale";
// Compatibility key written by agent-doc versions before turn-active leases
// became pane-scoped.
const PROJECT_SCOPE_ID: &str = "project";

/// Record the current turn owner in the project state database.
pub fn write_turn_active_marker(base: &Path, pane: &str) -> Result<()> {
    write_turn_active_marker_with_transcript_at(base, pane, now_secs(), None)
}

/// Record the current turn owner together with the harness transcript the
/// `UserPromptSubmit` hook named (`#staleharnessturnlive`). The transcript lets
/// a later read prove the turn was interrupted, which Claude Code never reports
/// through the `Stop` hook.
pub fn write_turn_active_marker_with_transcript(
    base: &Path,
    pane: &str,
    transcript: Option<&Path>,
) -> Result<()> {
    write_turn_active_marker_with_transcript_at(base, pane, now_secs(), transcript)
}

/// Clock-explicit form of [`write_turn_active_marker_with_transcript`], for
/// simulations that drive turn lifecycles on a virtual clock.
pub fn write_turn_active_marker_with_transcript_at(
    base: &Path,
    pane: &str,
    written_at: u64,
    transcript: Option<&Path>,
) -> Result<()> {
    let conn = open_state_db(base)?;
    upsert_coordination_lease_in_db(
        &conn,
        &CoordinationLeaseRecord {
            scope_kind: TURN_ACTIVE_SCOPE.to_string(),
            scope_id: pane.to_string(),
            holder: pane.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: written_at,
        },
    )?;
    match transcript {
        Some(transcript) => upsert_coordination_lease_in_db(
            &conn,
            &CoordinationLeaseRecord {
                scope_kind: TURN_ACTIVE_TRANSCRIPT_SCOPE.to_string(),
                scope_id: pane.to_string(),
                holder: transcript.to_string_lossy().into_owned(),
                holder_pid: Some(std::process::id()),
                heartbeat_secs: written_at,
            },
        ),
        None => {
            clear_coordination_lease_in_db(&conn, TURN_ACTIVE_TRANSCRIPT_SCOPE, pane).map(|_| ())
        }
    }
}

#[cfg(test)]
fn write_turn_active_marker_at(base: &Path, pane: &str, written_at: u64) -> Result<()> {
    write_turn_active_marker_with_transcript_at(base, pane, written_at, None)
}

/// Clear one pane's turn owner (turn idle / superseded). Absent is OK.
pub fn clear_turn_active_marker(base: &Path, pane: &str) -> Result<()> {
    let conn = open_state_db(base)?;
    clear_coordination_lease_in_db(&conn, TURN_ACTIVE_SCOPE, pane)?;
    clear_coordination_lease_in_db(&conn, TURN_ACTIVE_TRANSCRIPT_SCOPE, pane)?;
    // Retire only a matching legacy singleton. A Stop hook from another pane
    // must not erase the active owner written by an older installed binary.
    clear_coordination_lease_if_holder_in_db(&conn, TURN_ACTIVE_SCOPE, PROJECT_SCOPE_ID, pane)?;
    Ok(())
}

/// A harness turn-end receipt for one pane (`#runctrlclaude`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessTurnEnd {
    pub pane: String,
    /// Unix seconds of the boundary.
    pub ended_at: u64,
    /// Which boundary proved it (`session_start_clear`, `interrupt`, ...).
    pub reason: String,
}

fn record_harness_turn_end_in_db(
    conn: &Connection,
    pane: &str,
    ended_at: u64,
    reason: &str,
) -> Result<()> {
    // Monotonic: a late-arriving older receipt never moves the boundary back.
    if let Some(existing) = load_coordination_lease_from_db(conn, TURN_ENDED_SCOPE, pane)?
        && existing.heartbeat_secs > ended_at
    {
        return Ok(());
    }
    upsert_coordination_lease_in_db(
        conn,
        &CoordinationLeaseRecord {
            scope_kind: TURN_ENDED_SCOPE.to_string(),
            scope_id: pane.to_string(),
            holder: reason.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: ended_at,
        },
    )
}

/// Record that `pane`'s harness reached a turn-ending boundary at `ended_at`.
/// Callers must only pass boundaries that
/// [`agent_doc_turn::turn_status::harness_turn_end_reason`] (or a settled
/// interrupt record) proves.
pub fn record_harness_turn_end_at(
    base: &Path,
    pane: &str,
    ended_at: u64,
    reason: &str,
) -> Result<()> {
    let conn = open_state_db(base)?;
    record_harness_turn_end_in_db(&conn, pane, ended_at, reason)
}

/// Record that `pane`'s harness reported `session_id` at `observed_at`
/// (`#claimownerliveness`). Monotonic like the turn-end receipt.
pub fn record_pane_harness_session_at(
    base: &Path,
    pane: &str,
    session_id: &str,
    observed_at: u64,
) -> Result<()> {
    let conn = open_state_db(base)?;
    if let Some(existing) =
        load_coordination_lease_from_db(&conn, PANE_HARNESS_SESSION_SCOPE, pane)?
        && existing.heartbeat_secs > observed_at
    {
        return Ok(());
    }
    upsert_coordination_lease_in_db(
        &conn,
        &CoordinationLeaseRecord {
            scope_kind: PANE_HARNESS_SESSION_SCOPE.to_string(),
            scope_id: pane.to_string(),
            holder: session_id.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: observed_at,
        },
    )
}

/// The session id `pane`'s harness last reported, with when it was observed.
pub fn pane_harness_session(base: &Path, pane: &str) -> Option<(String, u64)> {
    let conn = open_state_db(base).ok()?;
    let row = load_coordination_lease_from_db(&conn, PANE_HARNESS_SESSION_SCOPE, pane)
        .ok()
        .flatten()?;
    Some((row.holder, row.heartbeat_secs))
}

/// [`pane_harness_session`] for the project containing `file`.
pub fn pane_harness_session_for_file(file: &Path, pane: &str) -> Option<(String, u64)> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    pane_harness_session(&root, pane)
}

/// The newest harness turn-end receipt for `pane`, if any.
pub fn last_harness_turn_end_for_pane(base: &Path, pane: &str) -> Option<HarnessTurnEnd> {
    let conn = open_state_db(base).ok()?;
    let row = load_coordination_lease_from_db(&conn, TURN_ENDED_SCOPE, pane)
        .ok()
        .flatten()?;
    Some(HarnessTurnEnd {
        pane: pane.to_string(),
        ended_at: row.heartbeat_secs,
        reason: row.holder,
    })
}

/// [`last_harness_turn_end_for_pane`] for the project containing `file`.
pub fn last_harness_turn_end_for_pane_for_file(file: &Path, pane: &str) -> Option<HarnessTurnEnd> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    last_harness_turn_end_for_pane(&root, pane)
}

fn marker_from_lease(lease: CoordinationLeaseRecord, now: u64) -> Option<TurnActiveMarker> {
    let marker = TurnActiveMarker {
        pane: lease.holder,
        written_at: lease.heartbeat_secs,
    };
    turn_active_marker_is_fresh(&marker, now).then_some(marker)
}

fn sweep_expired_turn_active_leases_in_db(conn: &Connection, now: u64) -> Result<usize> {
    let Some(cutoff) = turn_active_expiry_cutoff(now) else {
        return Ok(0);
    };
    clear_coordination_leases_heartbeat_at_or_before_in_db(
        conn,
        TURN_ACTIVE_TRANSCRIPT_SCOPE,
        cutoff,
    )?;
    clear_coordination_leases_heartbeat_at_or_before_in_db(conn, TURN_ACTIVE_SCOPE, cutoff)
}

/// A turn-active lease the harness transcript proves was interrupted
/// (`#staleharnessturnlive`) or whose prompt was abandoned before it was
/// persisted (`#sdkclearwedge`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedTurnLease {
    pub pane: String,
    /// Unix seconds the lease was written (the turn's `UserPromptSubmit`).
    pub written_at: u64,
    /// Unix seconds of the harness boundary record (the interrupt record, or
    /// the abandoned prompt's snapshot).
    pub interrupted_at: u64,
    /// The receipt reason the boundary proves (`interrupt` or
    /// `abandoned_prompt`).
    pub reason: &'static str,
    pub transcript: PathBuf,
}

/// Read at most [`TRANSCRIPT_TAIL_PROBE_BYTES`] from the end of `path`.
/// Returns the tail and whether it starts mid-file.
fn read_transcript_tail(path: &Path) -> Option<(String, bool)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_PROBE_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(TRANSCRIPT_TAIL_PROBE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some((String::from_utf8_lossy(&bytes).into_owned(), start > 0))
}

/// The transcript bound to exactly this lease, if its hook named one.
fn transcript_for_marker(conn: &Connection, marker: &TurnActiveMarker) -> Option<PathBuf> {
    let binding = load_coordination_lease_from_db(conn, TURN_ACTIVE_TRANSCRIPT_SCOPE, &marker.pane)
        .ok()??;
    (binding.heartbeat_secs == marker.written_at && !binding.holder.is_empty())
        .then(|| PathBuf::from(binding.holder))
}

fn interrupted_turn_lease(
    conn: &Connection,
    marker: &TurnActiveMarker,
    now: u64,
) -> Option<InterruptedTurnLease> {
    harness_ended_turn_lease(conn, marker, |evidence| {
        turn_lease_ended_by_interrupt(marker, evidence, now)
    })
}

/// The lease's bound transcript proves its turn ended under `ended`.
fn harness_ended_turn_lease(
    conn: &Connection,
    marker: &TurnActiveMarker,
    ended: impl Fn(TranscriptTailEvidence) -> bool,
) -> Option<InterruptedTurnLease> {
    let transcript = transcript_for_marker(conn, marker)?;
    let (tail, starts_mid_file) = read_transcript_tail(&transcript)?;
    let evidence = classify_transcript_tail(&tail, starts_mid_file);
    if !ended(evidence) {
        return None;
    }
    let (TranscriptTailEvidence::Interrupted { at_secs }
    | TranscriptTailEvidence::AbandonedPrompt { at_secs }) = evidence
    else {
        return None;
    };
    Some(InterruptedTurnLease {
        pane: marker.pane.clone(),
        written_at: marker.written_at,
        interrupted_at: at_secs,
        reason: transcript_turn_end_reason(evidence)?,
        transcript,
    })
}

/// Retire a fresh lease whose turn the harness transcript proves was
/// interrupted. The delete is conditioned on the observed heartbeat, so a
/// lease rewritten by a newer prompt in the meantime survives. Returns the
/// evidence when this call (or a concurrent one) retired it.
fn retire_interrupted_turn_lease(
    conn: &Connection,
    marker: &TurnActiveMarker,
    now: u64,
) -> Option<InterruptedTurnLease> {
    let interrupted = interrupted_turn_lease(conn, marker, now)?;
    retire_harness_ended_turn_lease(conn, marker, interrupted)
}

fn retire_harness_ended_turn_lease(
    conn: &Connection,
    marker: &TurnActiveMarker,
    interrupted: InterruptedTurnLease,
) -> Option<InterruptedTurnLease> {
    // `#runctrlclaude`: the interrupt (or abandoned prompt) is a
    // harness-authored turn boundary; keep it after the lease row is gone.
    let _ = record_harness_turn_end_in_db(
        conn,
        &marker.pane,
        interrupted.interrupted_at,
        interrupted.reason,
    );
    let _ = clear_coordination_lease_if_heartbeat_at_or_before_in_db(
        conn,
        TURN_ACTIVE_SCOPE,
        &marker.pane,
        marker.written_at,
    );
    let _ = clear_coordination_lease_if_heartbeat_at_or_before_in_db(
        conn,
        TURN_ACTIVE_TRANSCRIPT_SCOPE,
        &marker.pane,
        marker.written_at,
    );
    let _ = clear_coordination_lease_if_holder_in_db(
        conn,
        TURN_ACTIVE_SCOPE,
        PROJECT_SCOPE_ID,
        &marker.pane,
    );
    Some(interrupted)
}

/// `#staleharnessturnlive`: retire `pane`'s turn-active lease when the harness
/// transcript proves the turn was interrupted (Claude Code runs no `Stop` hook
/// on an interrupt). `None` when there is no fresh lease, no bound transcript,
/// or no settled interrupt newer than the lease.
pub fn reclaim_interrupted_turn_for_pane_at(
    base: &Path,
    pane: &str,
    now: u64,
) -> Option<InterruptedTurnLease> {
    let conn = open_state_db(base).ok()?;
    let lease = load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, pane)
        .ok()
        .flatten()?;
    let marker = marker_from_lease(lease, now)?;
    retire_interrupted_turn_lease(&conn, &marker, now)
}

/// `#cancelturnprompt`: retire `pane`'s turn-active lease when its prompt was
/// abandoned (`file-history-snapshot` with no prompt record, see
/// [`TranscriptTailEvidence::AbandonedPrompt`]) AND the caller has already
/// observed that prompt back in the harness composer and withdrawn it. The
/// withdrawal removes the one ambiguity the settle window waits out, so no
/// settle applies; the snapshot must still be no older than the lease. `None`
/// when there is no fresh lease, no bound transcript, or no such snapshot yet.
///
/// Callers must not use this without the composer observation: on transcript
/// evidence alone, [`reclaim_interrupted_turn_for_pane_at`] is the only rule.
pub fn reclaim_withdrawn_prompt_turn_for_pane_at(
    base: &Path,
    pane: &str,
    now: u64,
) -> Option<InterruptedTurnLease> {
    let conn = open_state_db(base).ok()?;
    let lease = load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, pane)
        .ok()
        .flatten()?;
    let marker = marker_from_lease(lease, now)?;
    let withdrawn = harness_ended_turn_lease(&conn, &marker, |evidence| {
        turn_lease_ended_by_withdrawn_prompt(&marker, evidence)
    })?;
    retire_harness_ended_turn_lease(&conn, &marker, withdrawn)
}

/// [`reclaim_withdrawn_prompt_turn_for_pane_at`] for the project containing
/// `file`.
pub fn reclaim_withdrawn_prompt_turn_for_pane_for_file(
    file: &Path,
    pane: &str,
) -> Option<InterruptedTurnLease> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    reclaim_withdrawn_prompt_turn_for_pane_at(&root, pane, now_secs())
}

/// [`reclaim_interrupted_turn_for_pane_at`] for the project containing `file`.
pub fn reclaim_interrupted_turn_for_pane_for_file(
    file: &Path,
    pane: &str,
) -> Option<InterruptedTurnLease> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    reclaim_interrupted_turn_for_pane_at(&root, pane, now_secs())
}

/// Delete every turn-active lease past `TURN_ACTIVE_TTL_SECS` (GH #135).
///
/// The sweep is keyed to heartbeat age only, never to the pane that wrote the
/// row: a pane that died before its idle hook can never clear its own lease, so
/// a per-pane clear cannot reclaim it. One bounded `DELETE`; the `WHERE` clause
/// re-checks the heartbeat, so a lease refreshed by a live turn between a read
/// and the sweep is never deleted. Returns the number of rows removed.
pub fn sweep_expired_turn_active_markers(base: &Path) -> Result<usize> {
    sweep_expired_turn_active_markers_at(base, now_secs())
}

pub fn sweep_expired_turn_active_markers_at(base: &Path, now: u64) -> Result<usize> {
    let conn = open_state_db(base)?;
    sweep_expired_turn_active_leases_in_db(&conn, now)
}

/// Count the turn-active leases a sweep at `now` would delete (`gc --dry-run`).
pub fn count_expired_turn_active_markers_at(base: &Path, now: u64) -> Result<usize> {
    let conn = open_state_db(base)?;
    Ok(
        load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE)?
            .into_iter()
            .filter(|lease| marker_from_lease(lease.clone(), now).is_none())
            .count(),
    )
}

/// A read that observed an expired lease reclaims it instead of only skipping
/// it, so expiry deletes the row rather than guaranteeing it survives
/// (GH #135). Best-effort: a failed reclaim never changes the read result.
fn reclaim_expired_turn_active_leases_on_read(conn: &Connection, now: u64) {
    let _ = sweep_expired_turn_active_leases_in_db(conn, now);
}

/// Read the turn-active marker if it exists and is not expired. An expired
/// marker is treated as absent so a missed `idle` hook self-heals instead of
/// wedging the session busy, and its row is reclaimed.
pub fn read_turn_active_marker_at(base: &Path, now: u64) -> Option<TurnActiveMarker> {
    let conn = open_state_db(base).ok()?;
    let leases = load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE).ok()?;
    let lease_count = leases.len();
    let fresh: Vec<TurnActiveMarker> = leases
        .into_iter()
        .filter_map(|lease| marker_from_lease(lease, now))
        .collect();
    if fresh.len() < lease_count {
        reclaim_expired_turn_active_leases_on_read(&conn, now);
    }
    fresh
        .into_iter()
        .filter(|marker| retire_interrupted_turn_lease(&conn, marker, now).is_none())
        .max_by_key(|marker| marker.written_at)
}

fn read_turn_active_marker_for_pane_at(
    base: &Path,
    pane: &str,
    now: u64,
) -> Option<TurnActiveMarker> {
    let conn = open_state_db(base).ok()?;
    let pane_lease = load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, pane)
        .ok()
        .flatten();
    let lease = match pane_lease {
        Some(lease) => lease,
        None => {
            let legacy =
                load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, PROJECT_SCOPE_ID)
                    .ok()??;
            (legacy.holder == pane).then_some(legacy)?
        }
    };
    let marker = marker_from_lease(lease, now);
    if marker.is_none() {
        reclaim_expired_turn_active_leases_on_read(&conn, now);
    }
    // `#staleharnessturnlive`: an interrupted turn ran no `Stop` hook; its
    // transcript is the turn-boundary evidence that retires the lease.
    marker.filter(|marker| retire_interrupted_turn_lease(&conn, marker, now).is_none())
}

/// Read the non-expired turn-active marker under `base`, if present.
pub fn read_turn_active_marker(base: &Path) -> Option<TurnActiveMarker> {
    read_turn_active_marker_at(base, now_secs())
}

/// Read the non-expired turn-active marker for the project containing `file`.
pub fn read_turn_active_marker_for_file(file: &Path) -> Option<TurnActiveMarker> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    read_turn_active_marker(&root)
}

/// True when a non-expired turn-active marker is present under `base`.
pub fn turn_active(base: &Path) -> bool {
    read_turn_active_marker(base).is_some()
}

/// True when the non-expired marker belongs to `pane`.
pub fn turn_active_for_pane(base: &Path, pane: &str) -> bool {
    read_turn_active_marker_for_pane_at(base, pane, now_secs())
        .is_some_and(|marker| turn_active_marker_matches_pane(&marker, pane))
}

/// (`#clearqueuedliveturn`) The fresh turn-active lease for `pane` in the
/// project containing `file`. Its `written_at` is the `UserPromptSubmit` second
/// of the turn, so it identifies one prompt: a deferred operator clear compares
/// it to tell its own turn from a newer prompt.
pub fn turn_active_marker_for_pane_for_file(file: &Path, pane: &str) -> Option<TurnActiveMarker> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    read_turn_active_marker_for_pane_at(&root, pane, now_secs())
        .filter(|marker| turn_active_marker_matches_pane(marker, pane))
}

/// True when the project containing `file` has a fresh marker for `pane`.
pub fn turn_active_for_pane_for_file(file: &Path, pane: &str) -> bool {
    agent_doc_project_root_io::project_root_containing(file)
        .is_some_and(|root| turn_active_for_pane(&root, pane))
}

/// Return the fresh turn-active marker only when it belongs to this document's
/// durable owner pane.
///
/// Project-wide turn markers are pane-scoped, so reading the newest marker by
/// itself can accidentally borrow liveness from a sibling document. Resolve the
/// document's registry owner first and require the marker for that exact pane.
/// Missing/unreadable registry or lease state remains `None`, preserving the
/// fail-closed behavior of callers that use this only as positive liveness
/// evidence.
pub fn active_turn_owner_for_file(file: &Path) -> Option<TurnActiveMarker> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    let owner = agent_doc_session_registry_io::lookup_file_entry_in(&root, file)
        .ok()
        .flatten()?;
    read_turn_active_marker_for_pane_at(&root, &owner.pane, now_secs())
        .filter(|marker| turn_active_marker_matches_pane(marker, &owner.pane))
}

/// Publish the stale-supervisor flag in the project state database.
/// Best-effort cross-process channel for the supervisor → turn-status hook.
pub fn set_supervisor_stale_marker(base: &Path, pane: &str, stale: bool) -> Result<()> {
    let conn = open_state_db(base)?;
    if stale {
        upsert_coordination_lease_in_db(
            &conn,
            &CoordinationLeaseRecord {
                scope_kind: SUPERVISOR_STALE_SCOPE.to_string(),
                scope_id: pane.to_string(),
                holder: "stale".to_string(),
                holder_pid: Some(std::process::id()),
                heartbeat_secs: now_secs(),
            },
        )
    } else {
        clear_coordination_lease_in_db(&conn, SUPERVISOR_STALE_SCOPE, pane).map(|_| ())
    }
}

/// True when the stale-supervisor marker is present under `base` (`#suptmuxstale`).
/// Read-only display probe — absent / unreadable reads as fresh (not stale).
pub fn supervisor_stale(base: &Path, pane: &str) -> bool {
    let Ok(conn) = open_state_db(base) else {
        return false;
    };
    load_coordination_lease_from_db(&conn, SUPERVISOR_STALE_SCOPE, pane)
        .ok()
        .flatten()
        .is_some()
}

/// Effect sink for the process-owned freshness projection. This marker is display
/// output only, never authority for recycle or turn admission.
pub fn project_supervisor_freshness(base: &Path, pane: &str, stale: bool) -> Result<()> {
    set_supervisor_stale_marker(base, pane, stale)?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    let output = tmux
        .cmd()
        .args(["display-message", "-p", "-t", pane, "#{pane_title}"])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "failed to read pane {pane} title: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let title = String::from_utf8_lossy(&output.stdout);
    let title = title.trim_end_matches(['\r', '\n']);
    let document_name = document_name_for_pane(base, pane);
    let updated = agent_doc_turn::turn_status::pane_title_with_freshness(
        title,
        document_name.as_deref(),
        stale,
    );
    if updated != title {
        let output = tmux
            .cmd()
            .args(["select-pane", "-t", pane, "-T", &updated])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "failed to update pane {pane} title: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn set_pane_title(pane: &str, title: &str) {
    let tmux = agent_doc_tmux_io::configured_tmux();
    if let Err(e) = tmux
        .cmd()
        .args(["select-pane", "-t", pane, "-T", title])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        // Best-effort UX surface — must never fail the turn.
        eprintln!("[turn-status] warning: failed to set pane {pane} title: {e}");
    }
}

/// Resolve the registered document basename for a pane. The registry is display
/// evidence only: failure leaves the legacy generic title in place and never
/// affects turn or supervisor authority.
fn document_name_for_pane(base: &Path, pane: &str) -> Option<String> {
    agent_doc_session_registry_io::load_in(base)
        .ok()?
        .values()
        .find(|entry| entry.pane == pane)
        .and_then(|entry| Path::new(&entry.file).file_name())
        .map(|name| name.to_string_lossy().into_owned())
}

/// Set a specific tmux pane's border title from turn/stale-supervisor state.
/// Best-effort: tmux failures are logged and ignored.
pub fn set_pane_title_for_status(base: &Path, pane: &str, active: bool) {
    let document_name = document_name_for_pane(base, pane);
    let title = pane_title_for_status(
        document_name.as_deref(),
        active,
        supervisor_stale(base, pane),
    );
    set_pane_title(pane, &title);
}

/// Clear a specific pane's turn-active projection. This is for supervisors that
/// have stronger idle evidence than a missed harness `Stop` hook.
pub fn clear_turn_status_for_pane(base: &Path, pane: &str) -> Result<()> {
    set_pane_title_for_status(base, pane, false);
    clear_turn_active_marker(base, pane)
}

/// Set the current tmux pane's border title to reflect the turn state. No-op
/// (Ok) when not running inside a tmux pane so the hook never breaks the turn.
pub fn run(active: bool) -> anyhow::Result<()> {
    let Ok(pane) = std::env::var("TMUX_PANE") else {
        return Ok(());
    };
    let pane = pane.trim().to_string();
    if pane.is_empty() {
        return Ok(());
    }
    // `#suptmuxstale` — decorate the pane title with a stale-supervisor warning when
    // the route-owned supervisor has published its `binary_stale` probe on disk.
    // Read-only display; absent/unreadable marker reads as fresh.
    let base = resolve_marker_base();
    let stale = base
        .as_ref()
        .map(|base| supervisor_stale(base, &pane))
        .unwrap_or(false);
    let document_name = base
        .as_ref()
        .and_then(|base| document_name_for_pane(base, &pane));
    let title = pane_title_for_status(document_name.as_deref(), active, stale);
    set_pane_title(&pane, &title);

    // Also maintain the readable turn-state marker so route/supervisor can tell
    // the agent is mid-turn (the bridge to a future hard busy-lease). The hook
    // runs in the agent's CWD = project root; if there is no `.agent-doc`
    // ancestor, skip the marker (the pane title still updated). Best-effort —
    // never fail the turn.
    if let Some(base) = base {
        let payload = hook_payload_from_stdin();
        let result =
            apply_turn_status_hook_at(&base, &pane, active, payload.as_deref(), now_secs());
        if let Err(e) = result {
            eprintln!("[turn-status] warning: failed to update turn-active marker: {e:#}");
        }
    }
    Ok(())
}

/// How long `turn-status active` waits for the hook payload on stdin. The
/// harness writes the JSON and closes the pipe immediately; a caller that
/// leaves stdin open must not stall the turn.
const HOOK_STDIN_WAIT: std::time::Duration = std::time::Duration::from_millis(500);
/// Upper bound on the hook payload read from stdin.
const HOOK_STDIN_MAX_BYTES: u64 = 1024 * 1024;

/// `transcript_path` from a harness hook payload (`UserPromptSubmit` JSON).
/// `~/` expands against `$HOME`. Absent, empty, or non-JSON payloads yield
/// `None`; the lease is then written without transcript evidence and retires
/// through `Stop` or the TTL exactly as before.
fn hook_transcript_path_from_payload(payload: &str) -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;
    let raw = value.get("transcript_path")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)),
        None => Some(PathBuf::from(raw)),
    }
}

/// Apply one `turn-status active|idle` hook invocation to the project state:
/// `active` writes the pane's lease (bound to the payload's transcript);
/// `idle` clears it and, when the payload is a turn-ending `SessionStart`
/// (`#runctrlclaude`), records a durable turn-end receipt at `now`.
pub fn apply_turn_status_hook_at(
    base: &Path,
    pane: &str,
    active: bool,
    payload: Option<&str>,
    now: u64,
) -> Result<()> {
    // `#claimownerliveness`: remember which harness session the pane runs, so
    // a queue claim made by an earlier session in this pane reads as ended
    // after `/clear`. Only the main session's own events carry it here
    // (`UserPromptSubmit`, `Stop`, `SessionStart`); best-effort.
    if let Some(session_id) = payload.and_then(hook_main_session_id_from_payload)
        && let Err(err) = record_pane_harness_session_at(base, pane, &session_id, now)
    {
        eprintln!("[turn-status] warning: failed to record pane harness session: {err:#}");
    }
    if active {
        let transcript = payload.and_then(hook_transcript_path_from_payload);
        return write_turn_active_marker_with_transcript_at(base, pane, now, transcript.as_deref());
    }
    let ended_turn_started_at = pane_turn_active_lease_heartbeat(base, pane);
    clear_turn_active_marker(base, pane)?;
    if let Some(turn_started_at) = ended_turn_started_at
        && let Err(err) = settle_dispatch_turns_ended_on_pane(base, pane, turn_started_at)
    {
        // Best-effort: the in-flight horizon still bounds an unsettled receipt.
        eprintln!("[turn-status] warning: failed to settle ended dispatch turn: {err:#}");
    }
    match payload.and_then(hook_turn_end_reason_from_payload) {
        Some(reason) => record_harness_turn_end_at(base, pane, now, reason),
        None => Ok(()),
    }
}

/// The `UserPromptSubmit` second of `pane`'s turn-active lease (pane-scoped, or
/// a matching legacy singleton), whatever its freshness: the turn-end hook
/// settles the turn that lease started even when it outlived the TTL.
fn pane_turn_active_lease_heartbeat(base: &Path, pane: &str) -> Option<u64> {
    let conn = open_state_db(base).ok()?;
    match load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, pane).ok()? {
        Some(lease) => Some(lease.heartbeat_secs),
        None => load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, PROJECT_SCOPE_ID)
            .ok()?
            .filter(|legacy| legacy.holder == pane)
            .map(|legacy| legacy.heartbeat_secs),
    }
}

/// `#clearqueuedliveturn`: the harness's own turn-end hook is the turn boundary
/// a route dispatch otherwise never reports. A route dispatch does not move the
/// actor, so the `running` receipt its turn's preflight promoted is never
/// settled by a `Ready` edge, and the turn kept reading as live
/// (`dispatch_turn_running`) for the whole in-flight horizon after it ended —
/// holding a queued operator clear (and refusing/queuing every fresh one) for
/// up to 30 minutes. Settle the receipts of the pane's documents dispatched at
/// or before the ended turn started; a newer turn's receipt is untouched.
/// Without a lease there is no turn identity to bound the settle, so nothing is
/// settled (the in-flight horizon remains the bound). Returns the count.
pub fn settle_dispatch_turns_ended_on_pane(
    base: &Path,
    pane: &str,
    turn_started_at: u64,
) -> Result<usize> {
    let conn = open_state_db(base)?;
    agent_doc_sqlite::state_store::settle_running_dispatch_turns_for_pane_started_at_or_before(
        &conn,
        pane,
        i64::try_from(turn_started_at).unwrap_or(i64::MAX),
    )
}

/// The main-session `session_id` of a Claude hook payload: present only on
/// events the main session itself fires (never `SubagentStop`).
fn hook_main_session_id_from_payload(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;
    let event = value.get("hook_event_name")?.as_str()?;
    if !matches!(event, "UserPromptSubmit" | "Stop" | "SessionStart") {
        return None;
    }
    let session = value.get("session_id")?.as_str()?.trim();
    (!session.is_empty()).then(|| session.to_string())
}

/// `#runctrlclaude`: the turn-end receipt a hook payload proves, if any.
fn hook_turn_end_reason_from_payload(payload: &str) -> Option<&'static str> {
    let value: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;
    agent_doc_turn::turn_status::harness_turn_end_reason(
        value
            .get("hook_event_name")
            .and_then(serde_json::Value::as_str),
        value.get("source").and_then(serde_json::Value::as_str),
    )
}

/// Best-effort read of the hook payload from stdin. Never blocks the turn:
/// an interactive stdin is skipped and a pipe that stays open is abandoned
/// after [`HOOK_STDIN_WAIT`].
fn hook_payload_from_stdin() -> Option<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut payload = String::new();
        let _ = std::io::stdin()
            .take(HOOK_STDIN_MAX_BYTES)
            .read_to_string(&mut payload);
        let _ = sender.send(payload);
    });
    receiver.recv_timeout(HOOK_STDIN_WAIT).ok()
}

/// Resolve the project root for the turn-active marker from the current working
/// directory (the harness hook runs there). `None` when there is no
/// `.agent-doc` ancestor.
fn resolve_marker_base() -> Option<PathBuf> {
    agent_doc_project_root_io::project_root_from_cwd().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::turn_status::TURN_ACTIVE_TTL_SECS;

    fn seed_actor_dispatch(base: &Path, document_id: &str, pane: &str, dispatched_at: u64) {
        let conn = open_state_db(base).unwrap();
        conn.execute(
            "INSERT INTO documents (document_id, canonical_path, session_id, generation, pane_id, \
             window_id, harness_id, actor_state) VALUES (?1, ?1, 's', 1, ?2, '@1', 'claude', 'ready')",
            (document_id, pane),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dispatch_attempts (document_id, generation, command_kind, result_status, \
             dispatch_start_proven, timestamp) VALUES (?1, 1, 'run', 'accepted', 0, ?2)",
            (document_id, dispatched_at as i64),
        )
        .unwrap();
        // The dispatched turn's own preflight (`#dispatchturnstartreceipt`).
        assert_eq!(
            agent_doc_sqlite::state_store::mark_open_dispatches_turn_started(&conn, document_id)
                .unwrap(),
            1
        );
    }

    fn dispatch_turn_running(base: &Path, document_id: &str, now: u64) -> bool {
        let conn = open_state_db(base).unwrap();
        agent_doc_sqlite::state_store::has_running_dispatch_turn_as_of(
            &conn,
            document_id,
            now as i64,
        )
        .unwrap()
    }

    /// `#clearqueuedliveturn`: a route-dispatched turn never moves the actor, so
    /// the only turn-end evidence for its `running` receipt is the harness's own
    /// turn-end hook. Before the fix the `Stop` hook cleared the lease but left
    /// the receipt `running`, so the turn read as live for the whole in-flight
    /// horizon after it ended and a clear queued behind it waited ~30 minutes.
    #[test]
    fn stop_hook_settles_the_route_dispatched_turn_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let dispatched_at = now_secs() - 30;
        seed_actor_dispatch(base, "doc", "%149", dispatched_at);
        let prompt_at = dispatched_at + 2;
        apply_turn_status_hook_at(base, "%149", true, None, prompt_at).unwrap();
        assert!(dispatch_turn_running(base, "doc", prompt_at + 1));

        let stop = r#"{"hook_event_name":"Stop","session_id":"s"}"#;
        apply_turn_status_hook_at(base, "%149", false, Some(stop), prompt_at + 20).unwrap();
        assert!(
            !dispatch_turn_running(base, "doc", prompt_at + 21),
            "the Stop hook must end the dispatched turn it closes"
        );
    }

    /// The settle is bounded by the ended turn's lease: a receipt dispatched
    /// after that turn started belongs to a newer turn, a pane without a lease
    /// has no turn identity, and another pane's document is untouched.
    #[test]
    fn stop_hook_settles_nothing_beyond_the_turn_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let now = now_secs();
        // A newer dispatch than the ended turn's prompt.
        seed_actor_dispatch(base, "doc", "%149", now - 5);
        apply_turn_status_hook_at(base, "%149", true, None, now - 10).unwrap();
        apply_turn_status_hook_at(base, "%149", false, None, now).unwrap();
        assert!(dispatch_turn_running(base, "doc", now));

        // No lease at all: nothing bounds the settle.
        seed_actor_dispatch(base, "lease-less", "%150", now - 30);
        apply_turn_status_hook_at(base, "%150", false, None, now).unwrap();
        assert!(dispatch_turn_running(base, "lease-less", now));

        // Another pane's turn end.
        seed_actor_dispatch(base, "other", "%7", now - 30);
        apply_turn_status_hook_at(base, "%151", true, None, now - 20).unwrap();
        apply_turn_status_hook_at(base, "%151", false, None, now).unwrap();
        assert!(dispatch_turn_running(base, "other", now));
    }

    /// `#claimownerliveness`: the pane's harness session is recorded from the
    /// main session's hooks (a `/clear` `SessionStart` replaces it), never from
    /// a subagent event, and never moves backwards in time.
    #[test]
    fn hooks_record_the_panes_current_harness_session() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let prompt =
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"pre-clear","prompt":"p"}"#;
        apply_turn_status_hook_at(base, "%180", true, Some(prompt), 100).unwrap();
        assert_eq!(
            pane_harness_session(base, "%180"),
            Some(("pre-clear".to_string(), 100))
        );
        let clear =
            r#"{"hook_event_name":"SessionStart","source":"clear","session_id":"post-clear"}"#;
        apply_turn_status_hook_at(base, "%180", false, Some(clear), 200).unwrap();
        assert_eq!(
            pane_harness_session(base, "%180"),
            Some(("post-clear".to_string(), 200))
        );
        let subagent = r#"{"hook_event_name":"SubagentStop","session_id":"sub"}"#;
        apply_turn_status_hook_at(base, "%180", false, Some(subagent), 300).unwrap();
        record_pane_harness_session_at(base, "%180", "late-old", 150).unwrap();
        assert_eq!(
            pane_harness_session(base, "%180"),
            Some(("post-clear".to_string(), 200))
        );
        assert_eq!(pane_harness_session(base, "%9"), None);
    }

    #[test]
    fn turn_active_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "no marker before active"
        );

        write_turn_active_marker_at(base, "%7", 1000).unwrap();
        let marker = read_turn_active_marker_at(base, 1000).expect("present after write");
        assert_eq!(marker.pane, "%7");
        assert_eq!(marker.written_at, 1000);

        clear_turn_active_marker(base, "%7").unwrap();
        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "absent after clear"
        );
        // Clearing an absent marker is a no-op, not an error.
        clear_turn_active_marker(base, "%7").unwrap();
    }

    #[test]
    fn supervisor_stale_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(!supervisor_stale(base, "%7"), "fresh before any marker");

        set_supervisor_stale_marker(base, "%7", true).unwrap();
        assert!(supervisor_stale(base, "%7"), "stale after marker write");
        assert!(!supervisor_stale(base, "%8"), "another pane is independent");
        set_supervisor_stale_marker(base, "%8", false).unwrap();
        assert!(
            supervisor_stale(base, "%7"),
            "fresh sibling cannot clear stale owner"
        );

        set_supervisor_stale_marker(base, "%7", false).unwrap();
        assert!(!supervisor_stale(base, "%7"), "fresh after marker clear");
        // Clearing an absent marker is a no-op, not an error.
        set_supervisor_stale_marker(base, "%7", false).unwrap();
    }

    #[test]
    fn turn_active_for_pane_matches_only_marker_pane() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", now_secs()).unwrap();

        assert!(turn_active_for_pane(base, "%7"));
        assert!(!turn_active_for_pane(base, "%8"));
    }

    #[test]
    fn active_turn_owner_for_file_requires_exact_registered_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let file = base.join("doc.md");
        std::fs::write(&file, "body\n").unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            file.display().to_string(),
            tmux_router::RegistryEntry {
                pane: "%152".to_string(),
                pid: std::process::id(),
                cwd: base.display().to_string(),
                started: "2026-10-08T22:06:04Z".to_string(),
                session_id: "session-live".to_string(),
                file: file.display().to_string(),
                window: "@2".to_string(),
                supervisor_instance_id: "supervisor-live".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(base, &registry).unwrap();

        write_turn_active_marker(base, "%999").unwrap();
        assert!(
            active_turn_owner_for_file(&file).is_none(),
            "a sibling pane's fresh marker is not document liveness"
        );

        write_turn_active_marker(base, "%152").unwrap();
        let marker = active_turn_owner_for_file(&file).expect("exact owner is active");
        assert_eq!(marker.pane, "%152");

        write_turn_active_marker_at(
            base,
            "%152",
            now_secs().saturating_sub(TURN_ACTIVE_TTL_SECS),
        )
        .unwrap();
        assert!(
            active_turn_owner_for_file(&file).is_none(),
            "an expired exact-owner lease is not liveness proof"
        );
    }

    #[test]
    fn concurrent_pane_markers_survive_sibling_idle() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", now_secs()).unwrap();
        write_turn_active_marker_at(base, "%8", now_secs()).unwrap();
        assert!(turn_active_for_pane(base, "%7"));
        assert!(turn_active_for_pane(base, "%8"));

        clear_turn_active_marker(base, "%8").unwrap();
        assert!(
            turn_active_for_pane(base, "%7"),
            "sibling Stop must preserve the active pane"
        );
        assert!(!turn_active_for_pane(base, "%8"));
    }

    #[test]
    fn pane_scoped_reads_and_clears_support_legacy_project_marker() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let conn = open_state_db(base).unwrap();
        upsert_coordination_lease_in_db(
            &conn,
            &CoordinationLeaseRecord {
                scope_kind: TURN_ACTIVE_SCOPE.to_string(),
                scope_id: PROJECT_SCOPE_ID.to_string(),
                holder: "%7".to_string(),
                holder_pid: None,
                heartbeat_secs: now_secs(),
            },
        )
        .unwrap();

        assert!(turn_active_for_pane(base, "%7"));
        clear_turn_active_marker(base, "%8").unwrap();
        assert!(
            turn_active_for_pane(base, "%7"),
            "foreign idle must not clear a legacy owner"
        );
        clear_turn_active_marker(base, "%7").unwrap();
        assert!(!turn_active_for_pane(base, "%7"));
    }

    fn turn_active_rows(base: &Path) -> Vec<String> {
        let conn = open_state_db(base).unwrap();
        let mut ids: Vec<String> =
            load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE)
                .unwrap()
                .into_iter()
                .map(|lease| lease.scope_id)
                .collect();
        ids.sort();
        ids
    }

    fn agent_doc_base() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    #[test]
    fn registered_document_name_is_resolved_by_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "sample-session.md".to_string(),
            tmux_router::RegistryEntry {
                pane: "%71".to_string(),
                pid: std::process::id(),
                cwd: base.display().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-71".to_string(),
                file: "tasks/sample-session.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: "supervisor-71".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(base, &registry).unwrap();

        assert_eq!(
            document_name_for_pane(base, "%71").as_deref(),
            Some("sample-session.md")
        );
        assert_eq!(document_name_for_pane(base, "%72"), None);
    }

    /// GH #135: a pane that died before its idle hook leaves a row no per-pane
    /// clear can ever reach. The age sweep removes it without knowing the pane.
    #[test]
    fn sweep_deletes_expired_orphan_row_for_nonexistent_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let now = 1_000_000;
        write_turn_active_marker_at(base, "%53", now - 292_943).unwrap();
        write_turn_active_marker_at(base, "%436", now - 76_837).unwrap();
        write_turn_active_marker_at(base, "%434", now - 144).unwrap();

        let deleted = sweep_expired_turn_active_markers_at(base, now).unwrap();

        assert_eq!(deleted, 2, "both expired rows are reclaimed");
        assert_eq!(turn_active_rows(base), vec!["%434".to_string()]);
    }

    /// The live turn's lease is never swept, including one exactly one second
    /// inside the TTL, and the sweep is a no-op when nothing is expired.
    #[test]
    fn sweep_keeps_fresh_rows() {
        let dir = agent_doc_base();
        let base = dir.path();
        let now = 1_000_000;
        write_turn_active_marker_at(base, "%7", now).unwrap();
        write_turn_active_marker_at(base, "%8", now - (TURN_ACTIVE_TTL_SECS - 1)).unwrap();
        write_turn_active_marker_at(base, "%9", now - TURN_ACTIVE_TTL_SECS).unwrap();

        assert_eq!(count_expired_turn_active_markers_at(base, now).unwrap(), 1);
        assert_eq!(sweep_expired_turn_active_markers_at(base, now).unwrap(), 1);
        assert_eq!(
            turn_active_rows(base),
            vec!["%7".to_string(), "%8".to_string()]
        );
        assert_eq!(sweep_expired_turn_active_markers_at(base, now).unwrap(), 0);

        // Inside the first TTL window nothing can be expired.
        let early = agent_doc_base();
        write_turn_active_marker_at(early.path(), "%1", 0).unwrap();
        assert_eq!(
            sweep_expired_turn_active_markers_at(early.path(), TURN_ACTIVE_TTL_SECS - 1).unwrap(),
            0
        );
        assert_eq!(turn_active_rows(early.path()), vec!["%1".to_string()]);
    }

    /// `clear_matching_turn_status_projection` gates its clear on
    /// `turn_active_for_pane_for_file`, which reads an expired lease as absent
    /// and so never reached the clear. That read now reclaims the row itself.
    #[test]
    fn expired_row_is_reclaimed_by_the_read_that_reports_it_absent() {
        let dir = agent_doc_base();
        let base = dir.path();
        let file = base.join("session.md");
        std::fs::write(&file, "# session\n").unwrap();
        let stale = now_secs() - TURN_ACTIVE_TTL_SECS - 10;
        write_turn_active_marker_at(base, "%65", stale).unwrap();
        write_turn_active_marker_at(base, "%66", now_secs()).unwrap();

        assert!(
            !turn_active_for_pane_for_file(&file, "%65"),
            "an expired lease still reads as absent"
        );
        assert_eq!(
            turn_active_rows(base),
            vec!["%66".to_string()],
            "the read reclaimed the expired row and kept the fresh one"
        );

        // The project-wide read reclaims too.
        write_turn_active_marker_at(base, "%55", stale).unwrap();
        let marker = read_turn_active_marker(base).expect("fresh marker survives");
        assert_eq!(marker.pane, "%66");
        assert_eq!(turn_active_rows(base), vec!["%66".to_string()]);
    }

    /// `#staleharnessturnlive` SimWorld: one project, one owner pane, one
    /// Claude-shaped transcript, and a virtual clock. Hook events drive the
    /// same IO entry points the `UserPromptSubmit` / `Stop` hooks use; the
    /// harness appends transcript records the way Claude Code does.
    struct TurnSimWorld {
        _dir: tempfile::TempDir,
        base: PathBuf,
        transcript: PathBuf,
        pane: &'static str,
        now: u64,
    }

    impl TurnSimWorld {
        fn new(pane: &'static str) -> Self {
            let dir = agent_doc_base();
            let base = dir.path().to_path_buf();
            let transcript = base.join("transcript.jsonl");
            std::fs::write(&transcript, "").unwrap();
            Self {
                _dir: dir,
                base,
                transcript,
                pane,
                now: 1_791_583_800,
            }
        }

        fn tick(&mut self, secs: u64) {
            self.now += secs;
        }

        fn append(&self, record: serde_json::Value) {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&self.transcript)
                .unwrap();
            writeln!(file, "{record}").unwrap();
        }

        fn stamp(&self) -> String {
            agent_doc_turn::turn_status::format_rfc3339_utc_secs(self.now)
        }

        /// `UserPromptSubmit`: the hook writes the lease, then the harness
        /// records the prompt.
        fn submit_prompt(&self, text: &str) {
            write_turn_active_marker_with_transcript_at(
                &self.base,
                self.pane,
                self.now,
                Some(&self.transcript),
            )
            .unwrap();
            self.append(serde_json::json!({
                "type": "user", "isSidechain": false, "timestamp": self.stamp(),
                "message": {"role": "user", "content": text},
            }));
        }

        fn tool_round_trip(&self) {
            self.append(serde_json::json!({
                "type": "assistant", "timestamp": self.stamp(),
                "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "Bash", "input": {}}]},
            }));
            self.append(serde_json::json!({
                "type": "user", "isSidechain": false, "timestamp": self.stamp(),
                "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}]},
            }));
        }

        /// Operator Esc: Claude Code records the interrupt and runs NO Stop hook.
        fn interrupt(&self) {
            self.append(serde_json::json!({
                "type": "user", "isSidechain": false, "timestamp": self.stamp(),
                "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]},
            }));
            self.append(serde_json::json!({"type": "pr-link", "timestamp": self.stamp()}));
        }

        fn stop_hook(&self) {
            clear_turn_active_marker(&self.base, self.pane).unwrap();
        }
        /// `#sdkclearwedge`: Claude Code's prompt-admission order. The
        /// `UserPromptSubmit` hook writes the lease; once the hooks finish the
        /// harness writes the prompt snapshot and, in the same flush, the
        /// prompt record (`persist == true`). An operator cancel in between
        /// drops the prompt back into the composer: snapshot only, no prompt
        /// record, no interrupt record, no Stop hook (`persist == false`).
        fn submit_prompt_with_snapshot(&mut self, message_id: &str, hook_secs: u64, persist: bool) {
            write_turn_active_marker_with_transcript_at(
                &self.base,
                self.pane,
                self.now,
                Some(&self.transcript),
            )
            .unwrap();
            let submitted = self.stamp();
            self.tick(hook_secs);
            self.append(serde_json::json!({
                "type": "file-history-snapshot", "messageId": message_id,
                "snapshot": {"messageId": message_id, "trackedFileBackups": {}, "timestamp": self.stamp()},
                "isSnapshotUpdate": false,
            }));
            if persist {
                self.append(serde_json::json!({
                    "type": "user", "isSidechain": false, "uuid": message_id, "timestamp": submitted,
                    "message": {"role": "user", "content": "/agent-doc tasks/sdk.md"},
                }));
            }
        }

        fn finish_turn_without_stop(&self) {
            self.append(serde_json::json!({
                "type": "assistant", "isSidechain": false, "timestamp": self.stamp(),
                "message": {"role": "assistant", "content": [{"type": "text", "text": "done"}]},
            }));
            self.append(serde_json::json!({"type": "system", "subtype": "turn_duration", "timestamp": self.stamp()}));
        }

        /// `#devclear`: one Codex rollout record at the virtual clock.
        fn codex_record(&self, kind: &str, payload: serde_json::Value) {
            self.append(serde_json::json!({
                "timestamp": self.stamp(), "type": kind, "payload": payload,
            }));
        }

        /// Codex prompt submission: the rollout records `task_started`, the
        /// `UserPromptSubmit` hook (`turn-status active`) writes the lease
        /// bound to the rollout named in its payload, then the prompt lands.
        fn codex_submit_prompt(&self, text: &str) {
            self.codex_record(
                "event_msg",
                serde_json::json!({"type": "thread_settings_applied"}),
            );
            self.codex_record("event_msg", serde_json::json!({"type": "task_started"}));
            let payload = serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "01a11743",
                "turn_id": "01a1284b",
                "transcript_path": self.transcript,
                "cwd": "/repo",
                "prompt": text,
            })
            .to_string();
            apply_turn_status_hook_at(&self.base, self.pane, true, Some(&payload), self.now)
                .unwrap();
            self.codex_record(
                "response_item",
                serde_json::json!({"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": text}]}),
            );
            self.codex_record("turn_context", serde_json::json!({"cwd": "/repo"}));
        }

        /// Codex Esc: the rollout records `turn_aborted` and Codex runs NO
        /// Stop hook.
        fn codex_interrupt(&self) {
            self.codex_record(
                "response_item",
                serde_json::json!({"type": "message", "role": "developer",
                    "content": [{"type": "input_text", "text": "<turn_aborted>\nThe previous turn was interrupted on purpose.\n</turn_aborted>"}]}),
            );
            self.codex_record(
                "event_msg",
                serde_json::json!({"type": "turn_aborted", "turn_id": "01a1284b",
                    "reason": "interrupted", "duration_ms": 1711}),
            );
        }

        fn live(&self) -> bool {
            read_turn_active_marker_for_pane_at(&self.base, self.pane, self.now).is_some()
        }
    }

    #[test]
    fn sim_interrupted_turn_lease_retires_without_a_stop_hook() {
        let mut world = TurnSimWorld::new("%161");
        world.submit_prompt("unwedge agent-doc");
        world.tick(5);
        world.tool_round_trip();
        assert!(world.live(), "a turn mid tool call is live");
        world.tick(6);
        world.interrupt();
        assert!(
            world.live(),
            "an unsettled interrupt keeps the lease (a same-second prompt may be landing)"
        );
        world.tick(agent_doc_turn::turn_status::TURN_INTERRUPT_SETTLE_SECS);
        let interrupted = reclaim_interrupted_turn_for_pane_at(&world.base, world.pane, world.now)
            .expect("settled interrupt retires the lease");
        assert_eq!(interrupted.pane, "%161");
        assert_eq!(interrupted.interrupted_at, interrupted.written_at + 11);
        assert!(
            !world.live(),
            "the idle owner pane no longer reads as mid-turn"
        );
        assert!(turn_active_rows(&world.base).is_empty());

        // The next prompt opens a new lease that the old interrupt cannot retire.
        world.tick(30);
        world.submit_prompt("unwedge agent-doc for contracts.md");
        world.tick(10);
        assert!(world.live());
        assert_eq!(
            reclaim_interrupted_turn_for_pane_at(&world.base, world.pane, world.now),
            None
        );
        world.stop_hook();
        assert!(!world.live());
    }

    #[test]
    fn sim_interrupt_never_retires_a_newer_or_unbound_lease() {
        // A prompt submitted in the same second as the interrupt: its lease is
        // written before its transcript record, and must survive the settle.
        let mut world = TurnSimWorld::new("%7");
        world.submit_prompt("first");
        world.tick(4);
        world.interrupt();
        write_turn_active_marker_with_transcript_at(
            &world.base,
            world.pane,
            world.now,
            Some(&world.transcript),
        )
        .unwrap();
        world.append(serde_json::json!({
            "type": "user", "isSidechain": false, "timestamp": world.stamp(),
            "message": {"role": "user", "content": "second"},
        }));
        world.tick(60);
        assert!(world.live(), "the newer prompt's turn stays live");

        // A lease written without a transcript (older hook, Codex, manual
        // `turn-status active`) keeps the Stop/TTL contract unchanged.
        let mut world = TurnSimWorld::new("%8");
        write_turn_active_marker_at(&world.base, world.pane, world.now).unwrap();
        world.tick(2);
        world.interrupt();
        world.tick(60);
        assert!(world.live());

        // A binding left by an earlier turn never vouches for a newer lease.
        let mut world = TurnSimWorld::new("%9");
        world.submit_prompt("bound");
        world.tick(2);
        write_turn_active_marker_at(&world.base, world.pane, world.now).unwrap();
        world.tick(1);
        world.interrupt();
        world.tick(60);
        assert!(world.live());
    }

    /// `#runctrlclaude` (live 2026-10-10, agent-doc-bugs.md): Ctrl-C landed
    /// while the `UserPromptSubmit` preflight hook was still running, so the
    /// transcript holds NO interrupt record and the lease stays live. The
    /// operator then typed `/clear`; Claude Code ran `SessionStart` (source
    /// `clear`), whose `turn-status idle` hook cleared the lease. That boundary
    /// must leave a durable receipt, while `Stop` and `compact` must not.
    #[test]
    fn sim_ctrl_c_during_hook_then_clear_leaves_a_turn_end_receipt() {
        let mut world = TurnSimWorld::new("%149");
        let submit = r#"{"hook_event_name":"UserPromptSubmit","transcript_path":"/nonexistent/t.jsonl","prompt":"/agent-doc x.md"}"#;
        apply_turn_status_hook_at(&world.base, world.pane, true, Some(submit), world.now).unwrap();
        world.tick(17);
        // Ctrl-C mid-hook: nothing written, no Stop hook.
        world.tick(15);
        assert!(world.live(), "no harness evidence ends the lease yet");
        assert_eq!(
            last_harness_turn_end_for_pane(&world.base, world.pane),
            None
        );

        let clear = r#"{"hook_event_name":"SessionStart","source":"clear","session_id":"n"}"#;
        apply_turn_status_hook_at(&world.base, world.pane, false, Some(clear), world.now).unwrap();
        assert!(!world.live());
        let receipt = last_harness_turn_end_for_pane(&world.base, world.pane)
            .expect("/clear is a turn-end receipt");
        assert_eq!(receipt.ended_at, world.now);
        assert_eq!(receipt.reason, "session_start_clear");

        // A later Stop (possibly blocked into a continuation) and an
        // auto-compaction SessionStart neither add nor move a receipt.
        world.tick(40);
        let stop = r#"{"hook_event_name":"Stop","stop_hook_active":false}"#;
        apply_turn_status_hook_at(&world.base, world.pane, false, Some(stop), world.now).unwrap();
        let compact = r#"{"hook_event_name":"SessionStart","source":"compact"}"#;
        apply_turn_status_hook_at(&world.base, world.pane, false, Some(compact), world.now)
            .unwrap();
        apply_turn_status_hook_at(&world.base, world.pane, false, None, world.now).unwrap();
        assert_eq!(
            last_harness_turn_end_for_pane(&world.base, world.pane),
            Some(receipt.clone())
        );
        // Receipts are monotonic: an older boundary never moves it back.
        record_harness_turn_end_at(&world.base, world.pane, receipt.ended_at - 5, "interrupt")
            .unwrap();
        assert_eq!(
            last_harness_turn_end_for_pane(&world.base, world.pane),
            Some(receipt)
        );
        // Receipts are pane-scoped.
        assert_eq!(last_harness_turn_end_for_pane(&world.base, "%150"), None);
    }

    /// `#runctrlclaude`: a settled interrupt that retires a lease also leaves
    /// a receipt at the interrupt record's time, which survives the lease row.
    #[test]
    fn sim_interrupt_retirement_leaves_a_turn_end_receipt() {
        let mut world = TurnSimWorld::new("%161");
        world.submit_prompt("unwedge agent-doc");
        world.tick(5);
        world.interrupt();
        let interrupted_at = world.now;
        world.tick(agent_doc_turn::turn_status::TURN_INTERRUPT_SETTLE_SECS);
        assert!(!world.live());
        let receipt =
            last_harness_turn_end_for_pane(&world.base, world.pane).expect("interrupt receipt");
        assert_eq!(receipt.ended_at, interrupted_at);
        assert_eq!(receipt.reason, "interrupt");
    }

    /// `#sdkclearwedge` incident replay (haiven-dev `tasks/sdk.md`, pane %180,
    /// 2026-10-11): the previous turn ended normally; a dispatched
    /// `/agent-doc` prompt wrote its lease at 04:30:21, its snapshot landed at
    /// 04:30:23, and the operator cancelled before the prompt record landed.
    /// Every Clear Session Context then refused `harness_turn_live
    /// evidence=turn_active_lease` while the pane sat idle at its prompt. The
    /// orphaned snapshot is the harness-authored boundary that retires it.
    #[test]
    fn sim_cancel_before_prompt_persists_retires_lease_after_settle() {
        let mut world = TurnSimWorld::new("%180");
        world.submit_prompt_with_snapshot("prev", 1, true);
        world.tick(30);
        world.finish_turn_without_stop();
        world.stop_hook();
        world.tick(200);

        world.submit_prompt_with_snapshot("7287902a", 2, false);
        let snapshot_at = world.now;
        assert!(
            world.live(),
            "an unsettled snapshot may still get its prompt record"
        );
        world.tick(agent_doc_turn::turn_status::ABANDONED_PROMPT_SETTLE_SECS - 1);
        assert!(world.live());
        world.tick(1);
        let retired = reclaim_interrupted_turn_for_pane_at(&world.base, world.pane, world.now)
            .expect("settled abandoned prompt retires the lease");
        assert_eq!(retired.reason, "abandoned_prompt");
        assert_eq!(retired.interrupted_at, snapshot_at);
        assert_eq!(retired.written_at, snapshot_at - 2);
        assert!(
            !world.live(),
            "the idle owner pane no longer reads as mid-turn"
        );
        let receipt =
            last_harness_turn_end_for_pane(&world.base, world.pane).expect("boundary receipt");
        assert_eq!(receipt.reason, "abandoned_prompt");
        assert_eq!(receipt.ended_at, snapshot_at);

        // The re-run after the operator's /clear opens a lease the old
        // snapshot can never retire.
        world.tick(5);
        world.submit_prompt_with_snapshot("next", 2, true);
        world.tick(600);
        assert!(world.live());
    }

    /// `#sdkclearwedge` fail-closed: a live turn whose model is still thinking
    /// (no record after the prompt for minutes) keeps its lease, in both
    /// snapshot orders, and a cancelled prompt followed by any turn activity
    /// is not reclaimed.
    #[test]
    fn sim_live_prompt_without_follow_up_records_keeps_its_lease() {
        let mut world = TurnSimWorld::new("%180");
        world.submit_prompt_with_snapshot("live", 25, true);
        world.tick(600);
        assert!(world.live(), "a persisted prompt is a live turn until Stop");

        let mut world = TurnSimWorld::new("%181");
        world.submit_prompt("prompt record first");
        world.append(serde_json::json!({
            "type": "file-history-snapshot", "messageId": "unrelated-but-after-a-prompt",
            "snapshot": {"messageId": "unrelated-but-after-a-prompt", "timestamp": world.stamp()},
            "isSnapshotUpdate": false,
        }));
        world.tick(600);
        assert!(
            world.live(),
            "a snapshot NEWER than the turn's own prompt record is not an abandoned prompt"
        );

        let mut world = TurnSimWorld::new("%182");
        world.submit_prompt_with_snapshot("cancelled", 2, false);
        world.tick(1);
        world.append(serde_json::json!({"type": "queue-operation", "operation": "dequeue", "timestamp": world.stamp()}));
        world.tick(600);
        assert!(
            world.live(),
            "a dequeued input after the snapshot may have started a turn"
        );
    }

    /// `#cancelturnprompt` incident replay (agent-loop agent-doc-bugs.md, pane
    /// %149, 2026-10-11): Run Agent Doc's lease at 05:59:23, Cancel Turn's
    /// `C-c` at 05:59:30 while the preflight hook ran, snapshot 0.7s later, no
    /// prompt record; Claude Code put `/agent-doc …` back in the composer. Once
    /// Cancel Turn has withdrawn that restored trigger, the lease retires at
    /// once instead of holding Clear Session Context for the settle window. A
    /// withdrawal reported before the snapshot lands (or for a prompt whose
    /// record persisted) retires nothing.
    #[test]
    fn sim_withdrawn_cancelled_prompt_retires_lease_without_settle() {
        let mut world = TurnSimWorld::new("%149");
        world.submit_prompt_with_snapshot("prev", 1, true);
        world.tick(13);
        world.finish_turn_without_stop();
        world.stop_hook();
        world.tick(600);

        // Lease written; the snapshot has not landed yet.
        write_turn_active_marker_with_transcript_at(
            &world.base,
            world.pane,
            world.now,
            Some(&world.transcript),
        )
        .unwrap();
        let lease_at = world.now;
        world.tick(7);
        assert_eq!(
            reclaim_withdrawn_prompt_turn_for_pane_at(&world.base, world.pane, world.now),
            None,
            "no snapshot yet: the previous turn's records prove nothing about this lease"
        );
        assert!(world.live());
        world.append(serde_json::json!({
            "type": "file-history-snapshot", "messageId": "8f4af13e",
            "snapshot": {"messageId": "8f4af13e", "trackedFileBackups": {}, "timestamp": world.stamp()},
            "isSnapshotUpdate": false,
        }));
        let snapshot_at = world.now;
        assert!(
            reclaim_interrupted_turn_for_pane_at(&world.base, world.pane, world.now).is_none(),
            "transcript evidence alone still waits out the settle window"
        );
        let retired =
            reclaim_withdrawn_prompt_turn_for_pane_at(&world.base, world.pane, world.now)
                .expect("a withdrawn abandoned prompt retires its lease at once");
        assert_eq!(retired.reason, "abandoned_prompt");
        assert_eq!(retired.written_at, lease_at);
        assert_eq!(retired.interrupted_at, snapshot_at);
        assert!(!world.live());
        assert_eq!(
            last_harness_turn_end_for_pane(&world.base, world.pane)
                .expect("boundary receipt")
                .reason,
            "abandoned_prompt"
        );

        // A persisted prompt is a live turn, withdrawal claim or not.
        world.tick(30);
        world.submit_prompt_with_snapshot("live", 2, true);
        assert_eq!(
            reclaim_withdrawn_prompt_turn_for_pane_at(&world.base, world.pane, world.now),
            None
        );
        assert!(world.live());
    }

    #[test]
    fn sim_project_wide_read_skips_an_interrupted_pane() {
        let mut world = TurnSimWorld::new("%161");
        write_turn_active_marker_at(&world.base, "%200", world.now).unwrap();
        world.tick(1);
        world.submit_prompt("newest lease, then interrupted");
        world.tick(2);
        world.interrupt();
        world.tick(10);
        let marker = read_turn_active_marker_at(&world.base, world.now).expect("sibling lease");
        assert_eq!(marker.pane, "%200");
        assert_eq!(turn_active_rows(&world.base), vec!["%200".to_string()]);
    }

    /// `#devclear` (live 2026-10-11, haiven-dev dev.md pane %127): the route
    /// dispatched a prompt into a Codex pane, the operator pressed Esc ~2s
    /// later, and Codex printed "Conversation interrupted" at an idle prompt.
    /// Codex runs no Stop hook on an interrupt, so the lease stayed live and
    /// Clear Session Context refused `evidence=turn_active_lease`. The
    /// rollout's `turn_aborted` record must retire the lease and leave an
    /// interrupt receipt, exactly as Claude's interrupt record does.
    #[test]
    fn sim_codex_interrupted_turn_lease_retires_without_a_stop_hook() {
        let mut world = TurnSimWorld::new("%127");
        world.codex_submit_prompt("Run agent-doc on tasks/dev.md");
        assert!(world.live(), "a submitted Codex turn is live");
        world.tick(2);
        world.codex_interrupt();
        let interrupted_at = world.now;
        assert!(world.live(), "an unsettled interrupt keeps the lease");
        // A background command the aborted turn left running finishes later:
        // not turn activity.
        world.tick(1);
        world.codex_record("event_msg", serde_json::json!({"type": "exec_command_end"}));
        world.codex_record("event_msg", serde_json::json!({"type": "token_count"}));
        world.tick(agent_doc_turn::turn_status::TURN_INTERRUPT_SETTLE_SECS);
        assert!(
            !world.live(),
            "the idle Codex pane no longer reads as mid-turn"
        );
        assert!(turn_active_rows(&world.base).is_empty());
        let receipt =
            last_harness_turn_end_for_pane(&world.base, world.pane).expect("interrupt receipt");
        assert_eq!(receipt.ended_at, interrupted_at);
        assert_eq!(receipt.reason, "interrupt");

        // The next Codex prompt opens a lease the old abort cannot retire,
        // and Codex's Stop hook (not a receipt) clears it normally.
        world.tick(30);
        world.codex_submit_prompt("next");
        world.tick(20);
        assert!(world.live());
        assert_eq!(
            reclaim_interrupted_turn_for_pane_at(&world.base, world.pane, world.now),
            None
        );
        let stop = r#"{"hook_event_name":"Stop","stop_hook_active":false}"#;
        apply_turn_status_hook_at(&world.base, world.pane, false, Some(stop), world.now).unwrap();
        assert!(!world.live());
        assert_eq!(
            last_harness_turn_end_for_pane(&world.base, world.pane),
            Some(receipt)
        );
    }

    /// `#devclear`: fail closed while a Codex turn may still be running.
    #[test]
    fn sim_codex_abort_never_retires_a_live_or_newer_turn() {
        let mut world = TurnSimWorld::new("%127");
        // An abort of an EARLIER turn, then a new prompt a second later
        // whose hook wrote the lease before the turn's records landed.
        world.codex_submit_prompt("first");
        world.tick(2);
        world.codex_interrupt();
        write_turn_active_marker_with_transcript_at(
            &world.base,
            world.pane,
            world.now + 1,
            Some(&world.transcript),
        )
        .unwrap();
        world.tick(10);
        assert!(
            world.live(),
            "an abort older than the lease says nothing about it"
        );

        // A live Codex turn that is only emitting bookkeeping stays live.
        let mut world = TurnSimWorld::new("%127");
        world.codex_submit_prompt("long tool call");
        world.tick(1);
        world.codex_record("event_msg", serde_json::json!({"type": "token_count"}));
        world.tick(600);
        assert!(world.live());
        // An abort with an unrecognised reason proves nothing.
        world.codex_record(
            "event_msg",
            serde_json::json!({"type": "turn_aborted", "reason": "replaced"}),
        );
        world.tick(30);
        assert!(world.live());
    }

    /// `#devclear`: the exact read Clear Session Context's live-turn guard makes
    /// (`turn_active_for_pane`, real clock) releases an interrupted Codex turn.
    #[test]
    fn codex_interrupt_releases_the_operator_clear_guard_read() {
        let dir = agent_doc_base();
        let base = dir.path();
        let rollout = base.join("rollout-codex.jsonl");
        let now = now_secs();
        let started = now - 10;
        let stamp = agent_doc_turn::turn_status::format_rfc3339_utc_secs;
        std::fs::write(
            &rollout,
            format!(
                "{}\n{}\n",
                serde_json::json!({"timestamp": stamp(started), "type": "event_msg",
                    "payload": {"type": "task_started"}}),
                serde_json::json!({"timestamp": stamp(started + 2), "type": "event_msg",
                    "payload": {"type": "turn_aborted", "reason": "interrupted"}}),
            ),
        )
        .unwrap();
        write_turn_active_marker_with_transcript_at(base, "%127", started, Some(&rollout)).unwrap();
        assert!(!turn_active_for_pane(base, "%127"));
        assert_eq!(
            last_harness_turn_end_for_pane(base, "%127").map(|r| r.ended_at),
            Some(started + 2)
        );
    }

    #[test]
    fn hook_payload_names_the_transcript() {
        assert_eq!(
            hook_transcript_path_from_payload(
                r#"{"session_id":"s","transcript_path":"/tmp/x/s.jsonl","hook_event_name":"UserPromptSubmit","prompt":"p"}"#
            ),
            Some(PathBuf::from("/tmp/x/s.jsonl"))
        );
        for payload in [
            "",
            "not json",
            r#"{"transcript_path":""}"#,
            r#"{"prompt":"p"}"#,
        ] {
            assert_eq!(
                hook_transcript_path_from_payload(payload),
                None,
                "{payload}"
            );
        }
    }
}
