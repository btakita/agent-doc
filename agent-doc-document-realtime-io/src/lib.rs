//! # Module: realtime_model
//!
//! ## Spec (`#rtwatch` — realtime editor-buffer ↔ disk read authority)
//! The agent-doc cycle (`preflight` / `write` / `finalize` / `session-check`)
//! currently sources "current document" by reading the **disk file** (and a
//! preflight snapshot/baseline). When an editor (IDEA / VS Code) holds **unsaved
//! edits**, its live buffer is *newer than disk* and is already treated as
//! `content_ours`-authoritative over the socket IPC apply path — so the cycle
//! reasons about a **staler** document than the one the user is editing, and an
//! agent write can clobber legitimate user queue/exchange content that only
//! exists in the buffer (the `#queue-user-edit-overwrite` / `test test` clobber
//! / `#ipcdrift` family).
//!
//! The deterministic read-authority decision lives in
//! `agent-doc-document-realtime`: given the on-disk content and an optional live
//! editor-buffer snapshot, decide which is authoritative for the agent to read,
//! following the operator's stated model — *"the editor buffer is the source of
//! truth for the document state when the editor is running... falling back to the
//! file on disk."* The authority rule keys off the buffer's **dirty** flag
//! (unsaved edits not yet flushed to disk) rather than comparing cross-source
//! timestamps, so it is unambiguous and deterministically testable without a live
//! editor:
//!
//! - editor buffer absent (no editor / closed) → **disk** wins;
//! - buffer content equals disk (saved, in sync) → **disk** wins (canonical);
//! - buffer is dirty / unsaved (content differs from disk) → **editor buffer**
//!   wins (it holds edits newer than disk);
//! - buffer is clean (matches its last save) but disk content differs → **disk**
//!   wins and the result is flagged `diverged` (disk was changed after the
//!   editor's last save — a drift signal the caller logs).
//!
//! Per the Shared Foundation pattern (`CLAUDE.md` — FFI-first for editor
//! integration; all deterministic behavior in the binary), this crate owns the
//! durable editor-buffer feed and ops-log/IPC side-effect adapter. Cycle read
//! sites (`preflight.rs` / `write.rs` / `session_check.rs`) source current-doc
//! through [`resolve_current_doc`], which delegates to the focused pure policy.
//!
//! ## Evals
//! - `durable_buffer_state_none_when_buffer_in_sync_with_disk`
//! - `durable_buffer_state_wins_when_unsaved_buffer_ahead_of_disk`
//! - `durable_buffer_state_none_when_no_editor_feed`

use anyhow::{Context, Result};

use agent_doc_document_realtime::{
    BufferState, Reconciliation,
    broadcast::{BroadcastPeer, compute_broadcast_plan},
    editor_identity::{jetbrains_editor_id_pid, sanitize_editor_id_for_filename},
    reconcile_current_doc,
    write_policy::{self, VisibleWriteReconcile},
};

// ── Rung 2 (`#rtwfeed`): durable, staleness-gated editor-buffer feed ──
//
// Rung 1 above is the *pure authority decision* over a `BufferState` the caller
// is assumed to trust. Rung 2 is the durable *source* of that `BufferState`: it
// reads the editor-buffer snapshot the plugin persists on every change
// (`.agent-doc/live-buffer/<hash>`, [`agent_doc_debounce::LiveBufferSnapshot`],
// written via the `#pcp6` full-content digest path), and only promotes it to an
// authoritative `BufferState` when the existing staleness classifier
// ([`agent_doc_debounce::live_buffer_diverges_from_content`]) proves the editor
// holds genuine *unsaved edits ahead of disk*.
//
// Gating on that classifier — not the raw `dirty`/`version` digest — is what
// keeps this safe. The classifier already suppresses the two clobber-in-reverse
// hazards this whole plan exists to avoid: (1) the editor digest merely *lags*
// agent-doc's own just-written disk content (`#pcp2` write-provenance), and
// (2) the editor buffer provably *equals* disk (`#pcp6` content match). So a
// stale buffer can never override a fresh disk write, and the feed only wins
// when the user has typed something disk does not yet have.

/// Map the staleness classifier's output to an authoritative [`BufferState`].
///
/// Pure (no I/O): `divergence` is `Some` only when
/// [`agent_doc_debounce::live_buffer_diverges_from_content`] proved the editor
/// holds unsaved edits ahead of disk. We additionally require the snapshot to
/// carry the **full buffer content** (`#pcp6`); a len/hash-only digest proves
/// *that* the buffer diverged but not *what* it contains, so we cannot
/// substitute it and fall back to disk (`None`). The snapshot timestamp is the
/// monotonic generation stamp.
fn buffer_state_from_divergence(
    divergence: Option<&agent_doc_debounce::LiveBufferSnapshot>,
) -> Option<BufferState> {
    let snapshot = divergence?;
    let content = snapshot.content.clone()?;
    Some(BufferState::new(
        content,
        true,
        snapshot.timestamp_ms as u64,
    ))
}

/// Canonical path string used to key the editor-buffer sidecar lookup. Mirrors
/// the convention the visible-write reconcile guard uses
/// (`guard_visible_write_reconcile`) so the path hashed here matches the
/// absolute path the editor plugin reported the buffer under; a relative vs
/// absolute mismatch would silently miss the sidecar.
fn indicator_path(file: &std::path::Path) -> String {
    file.canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string()
}

const VISIBLE_WRITE_TYPING_DEBOUNCE_MS: u64 = 500;
const VISIBLE_WRITE_TYPING_TIMEOUT_MS: u64 = 5_000;

/// Max re-merge attempts when reconciling the visible-write guard with a
/// foreign disk write that landed after the merge was computed
/// (#ipc-drift-visbuf-reconcile). After this many drifting re-reads, fall back
/// to the fail-closed guard so the operator retries.
pub const VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS: usize = 3;

pub fn guard_visible_write_idle(file: &std::path::Path, source: &str) -> Result<()> {
    guard_visible_write_idle_with_budget(
        file,
        source,
        VISIBLE_WRITE_TYPING_DEBOUNCE_MS,
        VISIBLE_WRITE_TYPING_TIMEOUT_MS,
    )
}

pub fn guard_visible_write_idle_with_budget(
    file: &std::path::Path,
    source: &str,
    debounce_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let indicator_path = indicator_path(file);
    let idle_reached =
        agent_doc_debounce::await_idle_via_file(&indicator_path, debounce_ms, timeout_ms);
    let facts = write_policy::VisibleWriteTypingFacts {
        idle_reached,
        timeout_ms,
    };
    let decision = write_policy::decide_visible_write_after_typing(facts);
    agent_doc_flow_io::log_flow_event(
        file,
        write_policy::visible_write_guard_event(decision, source),
        agent_doc_ops_log_io::log_op,
    );
    if decision == write_policy::VisibleWriteDecision::Apply {
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_deferred_active_typing file={} source={} debounce_ms={} timeout_ms={}",
            file.display(),
            source,
            debounce_ms,
            timeout_ms
        ),
    );
    anyhow::bail!(
        "visible document write for {} deferred: editor typing did not settle within {}ms; retry after typing stops",
        file.display(),
        timeout_ms
    )
}

pub fn guard_visible_write_idle_and_current(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    guard_visible_write_idle_current_or_target(file, source, expected_current, None)
}

pub fn guard_visible_write_idle_current_or_target(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<()> {
    match guard_visible_write_reconcile_with_target(file, source, expected_current, target_content)?
    {
        VisibleWriteReconcile::Clean => Ok(()),
        VisibleWriteReconcile::DiskDrifted { fresh_current } => {
            agent_doc_flow_io::log_flow_event(
                file,
                write_policy::visible_write_current_changed_event(source),
                agent_doc_ops_log_io::log_op,
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_deferred_current_changed file={} source={} expected_hash={} current_hash={}",
                    file.display(),
                    source,
                    agent_doc_hash::content_hash(expected_current),
                    agent_doc_hash::content_hash(&fresh_current)
                ),
            );
            anyhow::bail!(
                "visible document write for {} deferred: document changed after the response merge was computed; retry after typing stops",
                file.display()
            )
        }
    }
}

/// Like [`guard_visible_write_idle_and_current`] but, instead of failing closed
/// when the on-disk file drifted *after* the merge was computed, reports the
/// fresh disk content so a CRDT-merge caller can re-merge the captured response
/// and retry. A genuine live editor-buffer divergence (pending user edit) still
/// fails closed — only a clean foreign disk write is reported as reconcilable.
pub fn guard_visible_write_reconcile_with_target(
    file: &std::path::Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<VisibleWriteReconcile> {
    guard_visible_write_idle(file, source)?;
    let indicator_path = indicator_path(file);
    let actual_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    if let Some(live) =
        agent_doc_debounce::live_buffer_diverges_from_content(&indicator_path, expected_current)
    {
        // #nm1x provenance suppression: when the editor-visible buffer matches the
        // current on-disk content, the editor holds no unsaved edits *ahead* of
        // disk. The divergence is then disk-vs-expected — an independent / foreign
        // document edit, the reconcilable `DiskDrifted` case below — not a pending
        // user edit. Only a genuine unsaved editor buffer ahead of disk fails
        // closed. This replaces the coarse "any live-buffer divergence blocks
        // finalize" gate with an actor-aware check (the live-buffer actor is not
        // diverging when it already equals disk).
        let disk_hash = agent_doc_hash::content_hash(&actual_current);
        let editor_matches_disk = live_buffer_snapshot_matches_content(&live, &actual_current);
        if editor_matches_disk {
            let expected_hash = agent_doc_hash::content_hash(expected_current);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_live_buffer_matches_disk file={} source={} expected_len={} expected_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    expected_hash,
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else if live
            .content
            .as_deref()
            .is_some_and(|content| content_matches_recent_committed_blob(file, content, 15))
        {
            // A buffer that byte-matches a recent committed blob is stale recovery
            // evidence, not new unsaved operator text. Let the caller reconcile
            // from disk/current instead of failing closed on an editor sidecar
            // that is merely behind the committed document.
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_live_buffer_committed_blob_reconcile file={} source={} expected_len={} expected_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else if let Some(target) = target_content
            && actual_current == expected_current
            && live_buffer_snapshot_matches_content(&live, target)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_live_buffer_matches_target file={} source={} expected_len={} expected_hash={} target_len={} target_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    target.len(),
                    agent_doc_hash::content_hash(target),
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else if live.no_unsaved_operator_edits {
            // #falsetyping-guard: the editor-visible buffer diverges from both the
            // expected merge baseline and current disk, but the reporting editor
            // has proven the divergence is replica-driven — a `remoteCrdtApply`
            // (CRDT-replica churn) moved the buffer and there are NO unsaved local
            // operator edits ahead of disk. This is the false-positive the old
            // guard failed closed on ("buffer differs; save or discard"), wedging
            // finalize/write while the realtime replica kept reconciling. Route to
            // the reconcile path instead of bailing: fall through to the
            // disk-vs-expected decision below (Clean when disk still matches the
            // baseline, or DiskDrifted so the caller re-merges the captured
            // response against fresh disk). Genuine unsaved operator edits carry
            // `no_unsaved_operator_edits == false` and still fail closed above.
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_replica_churn_reconcile file={} source={} expected_len={} expected_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else {
            agent_doc_flow_io::log_flow_event(
                file,
                write_policy::visible_write_current_changed_event(source),
                agent_doc_ops_log_io::log_op,
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_deferred_live_buffer_changed file={} source={} expected_len={} expected_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
            anyhow::bail!(
                "visible editor buffer for {} differs from the expected disk state; save or discard the editor buffer, then retry",
                file.display()
            );
        }
    }
    if actual_current == expected_current {
        return Ok(VisibleWriteReconcile::Clean);
    }

    // Disk drifted but the live editor buffer did not diverge: a foreign
    // agent-doc supervisor appended to the same document mid-generation. This
    // is reconcilable — report the fresh disk content so the caller re-merges
    // instead of failing closed and stranding the captured response.
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_disk_drift_reconcilable file={} source={} expected_hash={} current_hash={}",
            file.display(),
            source,
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(&actual_current)
        ),
    );
    Ok(VisibleWriteReconcile::DiskDrifted {
        fresh_current: actual_current,
    })
}

fn live_buffer_snapshot_matches_content(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    content: &str,
) -> bool {
    if snapshot.len == content.len()
        && snapshot
            .hash
            .eq_ignore_ascii_case(&agent_doc_hash::content_hash(content))
    {
        return true;
    }
    snapshot.content.as_ref().is_some_and(|editor_text| {
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(editor_text)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content)
    })
}

/// Source the durable editor-buffer feed for `file`, gated by the existing
/// staleness-suppression classifier against the current `disk` content.
///
/// Returns `Some(BufferState)` **only** when the editor provably holds unsaved
/// edits ahead of disk and the full buffer text is available; otherwise `None`
/// (disk is authoritative). Durable across controller restart because the feed
/// reads the persisted `.agent-doc/live-buffer/<hash>` sidecar, not in-memory
/// state. Does filesystem reads (sidecar + provenance + mtime) but no blocking
/// waits, so it is safe off the project-control-pane hot path.
pub fn durable_buffer_state(file: &std::path::Path, disk: &str) -> Option<BufferState> {
    let indicator = indicator_path(file);
    let divergence = agent_doc_debounce::live_buffer_diverges_from_content(&indicator, disk);
    buffer_state_from_divergence(divergence.as_ref())
}

/// Detect a divergent live buffer that is **provably a stale-behind committed
/// ancestor** of HEAD — the only case it is safe to auto-heal the editor from
/// disk instead of promoting the buffer.
///
/// Returns `Some(stale buffer snapshot)` iff ALL of these hold:
/// - the live buffer *diverges* from disk (the same classifier
///   [`durable_buffer_state`] gates on — so a suppressed / in-sync buffer is
///   never touched);
/// - the snapshot carries the **full buffer content** (a len/hash-only digest
///   proves *that* it diverged but not *what* it holds, so we cannot prove it is
///   a committed ancestor — `None`);
/// - `disk == git HEAD` for `file` (agent-doc just committed; the clean
///   post-commit case). This guarantees any matched ancestor is strictly older
///   than disk, so the buffer cannot hold unsaved-ahead operator work;
/// - the buffer's full content byte-matches the file's blob at one of the recent
///   commits reachable from HEAD — an actual previously-saved committed state.
///
/// A committed-ancestor blob is by definition a previously-saved state, never new
/// unsaved work. If any condition is unprovable (including *any* git error), we
/// return `None` and the caller falls through to the existing promote-or-disk
/// behavior unchanged. This never keys off timestamps or heuristics.
fn stale_behind_committed_buffer(
    file: &std::path::Path,
    disk: &str,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    // Only proceed when the classifier proves the buffer diverges from disk.
    let divergence =
        agent_doc_debounce::live_buffer_diverges_from_content(&indicator_path(file), disk)?;
    // Need the FULL buffer text to prove it equals a committed blob; a
    // len/hash-only digest cannot be proven a committed ancestor → fall through.
    let buf = divergence.content.clone()?;
    // Require the clean post-commit case: disk == HEAD. Any git error → None.
    let head = agent_doc_git_io::revision::show_rev(file, "HEAD")
        .ok()
        .flatten()?;
    if disk != head {
        return None;
    }
    // Look for a recent committed blob that byte-equals the buffer. Because
    // divergence already proved `buf != disk` and `disk == head`, any match here
    // is necessarily an OLDER committed version (never HEAD, never unsaved work).
    if content_matches_recent_committed_blob(file, &buf, 15) {
        return Some(divergence);
    }
    None
}

/// True iff `content` byte-matches the file's blob at one of the last `limit`
/// commits reachable from HEAD (a previously-committed state — never unsaved
/// work). Best-effort: any git error → `false`.
///
/// A committed blob is by definition a previously-saved, recoverable state, so a
/// match proves `content` holds no unsaved operator edits. This is the shared
/// safety predicate both the READ auto-heal ([`stale_behind_committed_buffer`])
/// and the WRITE/FINALIZE capability gate key off of. It never consults
/// timestamps.
pub fn content_matches_recent_committed_blob(
    file: &std::path::Path,
    content: &str,
    limit: usize,
) -> bool {
    let lines = match agent_doc_git_io::revision::recent_commit_lines(file, None, limit) {
        agent_doc_git_io::revision::RecentCommitLog::Lines(lines) => lines,
        _ => return false,
    };
    for line in lines {
        let Some(sha) = line.split_whitespace().next() else {
            continue;
        };
        if let Ok(Some(blob)) = agent_doc_git_io::revision::show_rev(file, sha)
            && blob == content
        {
            return true;
        }
    }
    false
}

/// Resolve the authoritative "current document" for a cycle read: reconcile the
/// on-disk `disk` content against the durable editor-buffer feed for `file`.
///
/// This is the single entry point rung 3 (`#rtwwire`) wires into the cycle read
/// sites (`preflight` / `write` / `session-check`) so the agent reads
/// newest-of(disk, editor buffer) instead of bare disk. Emits a grep-able
/// `realtime_doc_resolve` ops.log marker so a live edit-during-finalize run can
/// prove which source won.
///
/// Before the normal promote path, a provably **stale-behind committed** buffer
/// (see [`stale_behind_committed_buffer`]) is treated as disk-authoritative and
/// the editor is auto-healed from disk via the `refresh_content` IPC primitive —
/// so a JetBrains buffer left showing an older committed version (e.g. after a
/// `--force-disk` commit whose stale sidecar re-emitted with a fresh timestamp,
/// defeating `#pcp2`) no longer wrongly promotes into the guards and no manual
/// "Reload from Disk" is needed.
pub fn resolve_current_doc(file: &std::path::Path, disk: &str) -> Reconciliation {
    if let Some(stale) = stale_behind_committed_buffer(file, disk) {
        // Best-effort auto-heal: push disk content to the editor, keyed on the
        // STALE buffer hash/len as the precondition proof (mirrors the
        // `send_refresh_content` call shape in write/converge.rs). Any failure is
        // ignored — the resolution below still makes disk authoritative.
        if let Some(stale_content) = stale.content.as_deref() {
            let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
            let stale_hash = agent_doc_hash::content_hash(stale_content);
            let _ = agent_doc_ipc_io::send_refresh_content(
                &project_root,
                &indicator_path(file),
                disk,
                &stale_hash,
                stale_content.len(),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "realtime_doc_autoheal action=refresh_editor_from_disk reason=stale_behind_committed_ancestor file={} stale_len={} stale_hash={} target_len={}",
                    file.display(),
                    stale_content.len(),
                    &stale_hash[..stale_hash.len().min(12)],
                    disk.len(),
                ),
            );
        }
        // Disk is authoritative: guards must see disk/HEAD, not the stale buffer.
        return reconcile_current_doc(disk, None);
    }
    let buffer = durable_buffer_state(file, disk);
    let reconciliation = reconcile_current_doc(disk, buffer.as_ref());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "realtime_doc_resolve authority={} reason={} diverged={} file={}",
            reconciliation.authority.as_str(),
            reconciliation.reason,
            reconciliation.diverged,
            file.display(),
        ),
    );
    reconciliation
}

/// File-IPC delivery queued for one peer editor.
#[derive(Debug, Clone)]
pub struct BroadcastDelivery {
    pub editor_id: String,
    pub patch_id: String,
    pub patch_file: std::path::PathBuf,
    pub merged_len: usize,
    pub node_patch_count: usize,
    pub component_patch_count: usize,
}

#[derive(Debug, Clone)]
struct BroadcastDelta {
    patches: Vec<serde_json::Value>,
    node_patches: Vec<serde_json::Value>,
    frontmatter: Option<String>,
}

/// Whether an editor id refers to a live process (`#sqdrift` / `#fccreap2`).
///
/// JetBrains ids carry the owning pid (`jetbrains-<pid>-<uuid>`); a dead pid
/// means the IntelliJ instance is gone and its live-buffer sidecar is an orphan.
/// A dead editor must never be a broadcast origin or target: broadcasting to it
/// re-creates the dead-pid patch file the `#fccreap` reaper just cleared and
/// merges against its divergent stale buffer, which then leaks into the finalize
/// IPC-proof path as `live_prompt_drift_after_preflight`. Non-JetBrains ids (no
/// embedded pid) are conservatively treated as live so we never drop a peer we
/// cannot assess (mirrors the `#fccreap` dead-consumer liveness stance).
fn editor_id_is_live(editor_id: &str) -> bool {
    match jetbrains_editor_id_pid(editor_id) {
        Some(pid) => agent_doc_plugin_owner::plugin_owner_pid_is_live(pid),
        None => true,
    }
}

/// Broadcast one editor's new full-buffer report to every other open editor
/// sidecar for the same document.
///
/// This is the FFI-first production delivery rung for `#rtwbcast`: plugins only
/// report their visible buffer and consume targeted patch files. Rust owns the
/// merge, echo suppression, per-editor patch filename, and payload shape.
///
/// Dead-editor liveness filtering (`#sqdrift` / `#fccreap2`): a dead originator
/// cannot produce a fresh change, and a dead peer's sidecar is an orphan, so both
/// are skipped (and dead peers' orphan sidecars reaped). Without this, a pile of
/// closed-IntelliJ sidecars makes this fan out a storm of patches to dozens of
/// dead consumers — regenerating the reaped patch files and feeding stale buffers
/// into the finalize IPC-proof path.
pub fn broadcast_editor_change(
    file: &std::path::Path,
    originator_editor_id: &str,
    originator_content: &str,
) -> anyhow::Result<Vec<BroadcastDelivery>> {
    let originator_editor_id = originator_editor_id.trim();
    if originator_editor_id.is_empty() {
        return Ok(Vec::new());
    }
    // `#sqdrift`: a dead editor cannot originate a fresh change. If the
    // originator's IntelliJ process is gone, this is a stale replay/echo — skip the
    // whole broadcast so we never storm patches to (and merge stale buffers from)
    // dead peers.
    if !editor_id_is_live(originator_editor_id) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_broadcast_skipped file={} origin_editor_id={} reason=dead_origin_editor",
                file.display(),
                originator_editor_id
            ),
        );
        return Ok(Vec::new());
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(Vec::new());
    };
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.is_dir() {
        return Ok(Vec::new());
    }
    let disk = std::fs::read_to_string(&canonical).unwrap_or_default();
    // `#sqdrift`: drop dead-editor peers and reap their orphan live-buffer
    // sidecars. A closed IntelliJ leaves its sidecar behind; without this, every
    // such orphan becomes a broadcast target, the fan-out re-creates the dead-pid
    // patch file the reaper just cleared, and the merge against its divergent
    // stale buffer leaks into the finalize IPC-proof path.
    let mut reaped_dead_peers = 0usize;
    let peers: Vec<BroadcastPeer> = agent_doc_debounce::live_buffer_snapshots(&canonical_str)
        .into_iter()
        .filter_map(|snapshot| {
            let editor_id = snapshot.editor_id?;
            let content = snapshot.content?;
            Some(BroadcastPeer::new(editor_id, content))
        })
        .filter(|peer| {
            if editor_id_is_live(&peer.editor_id) {
                return true;
            }
            reaped_dead_peers += 1;
            if let Err(err) =
                agent_doc_debounce::clear_live_buffer_for_editor(&canonical_str, Some(&peer.editor_id))
            {
                eprintln!(
                    "[agent-doc] warning: failed to reap dead-editor live-buffer sidecar for {}: {err}",
                    peer.editor_id
                );
            }
            false
        })
        .collect();
    if reaped_dead_peers > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_broadcast_dead_peers_reaped file={} count={} reason=dead_editor_pid",
                file.display(),
                reaped_dead_peers
            ),
        );
    }
    let targets = compute_broadcast_plan(&disk, originator_editor_id, originator_content, &peers)?;
    let doc_hash = agent_doc_fs::document_state_hash(&canonical)?;
    let mut deliveries = Vec::new();
    for target in targets {
        let Some(delta) =
            broadcast_component_delta_for_peer(file, &target.merged, &target.editor_id, &peers)?
        else {
            continue;
        };
        let patch_id = uuid::Uuid::new_v4().to_string();
        let patch_file = patches_dir.join(format!(
            "{}.{}.json",
            doc_hash,
            sanitize_editor_id_for_filename(&target.editor_id)
        ));
        let peer_baseline = peers
            .iter()
            .find(|peer| peer.editor_id == target.editor_id)
            .map(|peer| peer.content.as_str())
            .unwrap_or("");
        let mut payload = serde_json::json!({
            "type": "patch",
            "file": canonical_str.clone(),
            "editor_id": target.editor_id.clone(),
            "origin_editor_id": originator_editor_id,
            "patch_id": patch_id.clone(),
            "patches": delta.patches,
            "node_patches": delta.node_patches,
            "unmatched": "",
            "baseline": peer_baseline,
            "baseline_hash": agent_doc_hash::content_hash(peer_baseline),
            "baseline_normalized_hash": agent_doc_hash::content_hash(
                &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(peer_baseline),
            ),
            "reposition_boundary": false,
        });
        let node_patch_count = payload
            .get("node_patches")
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0);
        let component_patch_count = payload
            .get("patches")
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0);
        if let Some(frontmatter) = delta.frontmatter {
            payload["frontmatter"] = serde_json::Value::String(frontmatter);
        }
        agent_doc_fs::write_atomic(
            &patch_file,
            serde_json::to_string_pretty(&payload)?.as_bytes(),
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_broadcast_queued file={} origin_editor_id={} target_editor_id={} patch_id={} merged_len={} node_patches={} component_patches={}",
                file.display(),
                originator_editor_id,
                payload
                    .get("editor_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("-"),
                patch_id,
                target.merged.len(),
                node_patch_count,
                component_patch_count,
            ),
        );
        deliveries.push(BroadcastDelivery {
            editor_id: payload
                .get("editor_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            patch_id,
            patch_file,
            merged_len: target.merged.len(),
            node_patch_count,
            component_patch_count,
        });
    }
    Ok(deliveries)
}

fn broadcast_component_delta_for_peer(
    file: &std::path::Path,
    merged: &str,
    peer_editor_id: &str,
    peers: &[BroadcastPeer],
) -> anyhow::Result<Option<BroadcastDelta>> {
    let Some(peer) = peers.iter().find(|peer| peer.editor_id == peer_editor_id) else {
        return Ok(None);
    };
    if peer.content == merged {
        return Ok(None);
    }
    let node_patches =
        agent_doc_ipc_protocol::build_ipc_node_patches_json(Some(&peer.content), Some(merged));
    let patches =
        agent_doc_document::component_patches::component_replace_patches(&peer.content, merged)?;
    let frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(merged)
        .filter(|merged_fm| {
            agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(&peer.content)
                != Some(*merged_fm)
        })
        .map(ToString::to_string);
    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "realtime_broadcast_skipped file={} target_editor_id={} reason=no_component_delta",
                file.display(),
                peer_editor_id
            ),
        );
        return Ok(None);
    }
    Ok(Some(BroadcastDelta {
        patches,
        node_patches,
        frontmatter,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Rung 2 (`#rtwfeed`) durable-feed bridge ──
    use agent_doc_debounce::LiveBufferSnapshot;

    fn snapshot(content: Option<&str>, generation: u128) -> LiveBufferSnapshot {
        let body = content.unwrap_or("");
        LiveBufferSnapshot {
            path: "doc.md".to_string(),
            len: body.len(),
            hash: agent_doc_hash::content_hash(body),
            timestamp_ms: generation,
            edit_epoch: 0,
            last_synced_epoch: 0,
            state_vector_b64: None,
            editor_id: None,
            editor_kind: None,
            editor_version: None,
            capabilities: Vec::new(),
            content: content.map(|c| c.to_string()),
            no_unsaved_operator_edits: false,
        }
    }

    #[test]
    fn buffer_state_from_divergence_none_when_no_divergence() {
        // Classifier suppressed (in sync / stale / agent's own write) → disk wins.
        assert!(buffer_state_from_divergence(None).is_none());
    }

    #[test]
    fn buffer_state_from_divergence_promotes_full_content() {
        let snap = snapshot(Some("## Queue\n- do [#a]\n- do [#rtwatch]\n"), 4242);
        let state = buffer_state_from_divergence(Some(&snap)).expect("full content promotes");
        assert!(state.dirty, "a proven divergence is unsaved-ahead-of-disk");
        assert_eq!(state.generation, 4242);
        assert!(state.content.contains("#rtwatch"));
    }

    #[test]
    fn buffer_state_from_divergence_falls_back_when_content_absent() {
        // A len/hash-only digest proves THAT the buffer diverged but not WHAT it
        // holds — we must not fabricate content, so disk stays authoritative.
        let snap = snapshot(None, 7);
        assert!(buffer_state_from_divergence(Some(&snap)).is_none());
    }

    /// Build a temp project with `.agent-doc/` and the document on disk. Returns
    /// the `TempDir` (keep alive), the file `PathBuf`, and the canonical path
    /// string — the sidecar must be recorded under the same canonical key the
    /// feed canonicalizes to, exactly as the live editor plugin reports it.
    fn temp_doc(disk: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, disk).unwrap();
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    #[test]
    fn durable_buffer_state_none_when_buffer_in_sync_with_disk() {
        let disk = "## Queue\n- do [#a]\n";
        let (_dir, file, canonical) = temp_doc(disk);
        // Editor reports the same text as disk: no unsaved edit ahead → disk wins.
        agent_doc_debounce::record_live_buffer_digest_content(&canonical, disk).unwrap();
        assert!(durable_buffer_state(&file, disk).is_none());
        // resolve_current_doc agrees, returns disk content.
        let r = resolve_current_doc(&file, disk);
        assert_eq!(r.authority, agent_doc_document_realtime::DocAuthority::Disk);
        assert_eq!(r.content, disk);
    }

    #[test]
    fn durable_buffer_state_wins_when_unsaved_buffer_ahead_of_disk() {
        // #queue-user-edit-overwrite: user typed a queue item in the editor that
        // disk does not have yet. The durable feed must surface the buffer so the
        // agent reads it instead of clobbering it.
        let disk = "## Queue\n- do [#a]\n";
        let buffer = "## Queue\n- do [#a]\n- do [#rtwatch]\n";
        let (_dir, file, canonical) = temp_doc(disk);
        agent_doc_debounce::record_live_buffer_digest_content(&canonical, buffer).unwrap();
        let state = durable_buffer_state(&file, disk).expect("unsaved buffer wins");
        assert_eq!(state.content, buffer);
        let r = resolve_current_doc(&file, disk);
        assert_eq!(
            r.authority,
            agent_doc_document_realtime::DocAuthority::EditorBuffer
        );
        assert!(r.content.contains("#rtwatch"));
    }

    #[test]
    fn durable_buffer_state_none_when_no_editor_feed() {
        // No sidecar recorded (no editor attached) → disk is the only source.
        let disk = "plain disk body\n";
        let (_dir, file, _canonical) = temp_doc(disk);
        assert!(durable_buffer_state(&file, disk).is_none());
        assert_eq!(
            resolve_current_doc(&file, disk).authority,
            agent_doc_document_realtime::DocAuthority::Disk
        );
    }

    #[test]
    fn visible_write_guard_blocks_when_editor_typing_active() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/typing")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "body\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);
        for _ in 0..50 {
            if agent_doc_debounce::is_typing_via_file(&doc_str, 60_000) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let err = guard_visible_write_idle_with_budget(&doc, "test_visible_write", 60_000, 0)
            .unwrap_err();

        assert!(err.to_string().contains("editor typing did not settle"));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_typing_defer_active_typing:test_visible_write"));
        assert!(log.contains("visible_write_deferred_active_typing"));
    }

    #[test]
    fn visible_write_guard_blocks_when_current_changed_after_merge() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

<!--
scratch
-->
";
        std::fs::write(&doc, expected).unwrap();
        std::fs::write(
            &doc,
            expected.replace("scratch", "scratch\nstill typing this line"),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_current_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("document changed after the response merge was computed")
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_current_changed"));
        assert!(log.contains("visible_write_deferred_current_changed"));
    }

    #[test]
    fn visible_write_guard_blocks_when_idle_editor_buffer_differs_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        let live_buffer = expected.replace(
            "<!-- /agent:exchange -->",
            "prompt typed but not saved\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            live_buffer.len(),
            &agent_doc_hash::content_hash(&live_buffer),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_live_buffer_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string().contains("visible editor buffer"),
            "expected live-buffer guard error: {err}"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), expected);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_live_buffer_changed"));
        assert!(log.contains("visible_write_deferred_live_buffer_changed"));
    }

    #[test]
    fn visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift() {
        // #nm1x: the editor reported a buffer that diverges from `expected` but
        // *matches the current on-disk content* (an independent document edit the
        // editor already saved). That is not a pending unsaved user edit, so the
        // guard must not fail closed — it reports the reconcilable DiskDrifted case
        // instead, letting the response re-merge against the fresh disk content.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        // Disk + editor both carry an independent queue edit not present in
        // `expected`; the editor digest equals disk (saved, no pending edit).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#sibling]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, &drifted).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            drifted.len(),
            &agent_doc_hash::content_hash(&drifted),
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_editor_matches_disk",
            expected,
            None,
        )
        .unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_disk"),
            "expected provenance-suppression log: {log}"
        );
        assert!(
            log.contains("source=test_editor_matches_disk"),
            "marker must identify the write source: {log}"
        );
        assert!(
            log.contains(&format!("expected_len={}", expected.len())),
            "marker must carry expected length: {log}"
        );
        assert!(
            log.contains(&format!(
                "expected_hash={}",
                agent_doc_hash::content_hash(expected)
            )),
            "marker must carry expected hash: {log}"
        );
        assert!(
            log.contains(&format!("disk_len={}", drifted.len())),
            "marker must carry disk length: {log}"
        );
        assert!(
            log.contains(&format!(
                "disk_hash={}",
                agent_doc_hash::content_hash(&drifted)
            )),
            "marker must carry disk hash: {log}"
        );
        assert!(
            log.contains(&format!("live_len={}", drifted.len())),
            "marker must carry live-buffer length: {log}"
        );
        assert!(
            log.contains(&format!(
                "live_hash={}",
                agent_doc_hash::content_hash(&drifted)
            )),
            "marker must carry live-buffer hash: {log}"
        );
        assert!(
            log.contains("live_ts="),
            "marker must carry live timestamp: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "must not record a fail-closed live-buffer block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_normalizes_transient_live_buffer_markers() {
        // The editor sidecar can lag only in transient agent-doc markers (for
        // example a regenerated boundary id). That is not operator text and must
        // not fail closed as a live-buffer edit.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:boundary:old -->
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        let live = expected.replace("agent:boundary:old", "agent:boundary:new");
        std::fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &live,
            "jetbrains-boundary-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_transient_marker",
            expected,
            None,
        )
        .expect("transient marker churn must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk still matches expected"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_disk"),
            "normalized transient match should be logged as disk-safe: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "transient marker churn must not trip the fail-closed block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_reports_clean_when_disk_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, expected).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_clean", expected, None).unwrap();
        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
    }

    #[test]
    fn visible_write_reconcile_accepts_live_buffer_matching_target() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
❯ Fix the repair retry
<!-- /agent:exchange -->
";
        let target = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: Fix the repair retry - gpt-5\n\nDone.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            target.len(),
            &agent_doc_hash::content_hash(&target),
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_live_buffer_matches_target",
            expected,
            Some(&target),
        )
        .unwrap();

        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            expected,
            "the guard only classifies proof; the caller still owns the disk write"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_target")
                && log.contains("source=test_live_buffer_matches_target"),
            "target-matching live-buffer proof should be logged:\n{log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "a target-matching live buffer must not trip the drift guard:\n{log}"
        );
    }

    #[test]
    fn visible_write_reconcile_reports_disk_drift_without_live_buffer_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        // Disk grew under us with a foreign agent-doc append (no live editor buffer
        // sidecar = no pending user edit), so the guard must report it as a
        // reconcilable drift rather than failing closed (#ipc-drift-visbuf-reconcile).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &drifted).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_drift", expected, None).unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
    }

    #[test]
    fn visible_write_reconcile_replica_churn_does_not_fail_closed() {
        // #falsetyping-guard: the editor buffer diverges from both `expected` and
        // the on-disk content because a `remoteCrdtApply` (CRDT-replica churn)
        // moved the buffer — NOT because the operator has unsaved edits. The plugin
        // stamps `no_unsaved_operator_edits = true`. Disk still equals the merge
        // baseline, so the guard must NOT fail closed with "buffer differs; save or
        // discard"; it routes to the reconcile path (Clean here) so the response
        // lands instead of wedging finalize/write.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        // Disk matches the merge baseline (`expected`). The editor buffer is ahead
        // with a remote replica's converged content that is neither `expected` nor
        // anything the operator typed.
        std::fs::write(&doc, expected).unwrap();
        let replica_ahead = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: from-another-replica\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &replica_ahead,
            "jetbrains-42-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            true,
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_replica_churn", expected, None)
                .expect("replica churn must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk matches the merge baseline, so replica churn reconciles Clean"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_replica_churn_reconcile"),
            "expected replica-churn reconcile marker: {log}"
        );
        assert!(
            log.contains("source=test_replica_churn"),
            "marker must identify the write source: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "replica churn must not trip the fail-closed live-buffer block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_committed_blob_buffer_does_not_fail_closed() {
        // A live editor buffer that equals a recent committed blob is stale
        // recovery/editor state, not new unsaved operator text. The write guard
        // should reconcile from disk/current instead of halting the hot path.
        let old = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old\n",
            "<!-- /agent:exchange -->\n",
        );
        let new = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: new\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc, canonical) = temp_git_doc(&[old, new]);
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &canonical,
            old,
            "jetbrains-stale-commit-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_committed_blob", new, None)
                .expect("committed-blob live buffer must not fail closed");
        assert!(
            matches!(outcome, VisibleWriteReconcile::Clean),
            "disk still matches the expected current document"
        );
        let log =
            std::fs::read_to_string(doc.parent().unwrap().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_committed_blob_reconcile"),
            "committed-blob reconcile marker should be logged: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "committed stale buffer must not trip the fail-closed block: {log}"
        );
    }

    #[test]
    fn visible_write_reconcile_replica_churn_with_disk_drift_remerges() {
        // #falsetyping-guard: replica churn AND a foreign disk append at the same
        // time. Provenance proves no unsaved operator edits, so instead of failing
        // closed the guard reports the reconcilable DiskDrifted case (re-merge the
        // response against fresh disk).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign-append\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &drifted).unwrap();
        let replica_ahead = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: from-another-replica\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &replica_ahead,
            "jetbrains-43-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            true,
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_replica_churn_drift",
            expected,
            None,
        )
        .expect("replica churn with disk drift must not fail closed");
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("visible_write_replica_churn_reconcile"));
    }

    #[test]
    fn visible_write_reconcile_genuine_operator_edit_still_fails_closed() {
        // #falsetyping-guard invariant: a genuine unsaved operator edit
        // (`no_unsaved_operator_edits = false`, the conservative default) must
        // STILL fail closed. Operator text is authoritative and must never be
        // dropped by a stale-merge write.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: x
<!-- /agent:exchange -->
";
        std::fs::write(&doc, expected).unwrap();
        let operator_typed = expected.replace(
            "<!-- /agent:exchange -->",
            "❯ operator is typing an unsaved question\n<!-- /agent:exchange -->",
        );
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
            &doc_str,
            &operator_typed,
            "jetbrains-44-test",
            "jetbrains",
            "0.2.205",
            &["operator_text_authority_v1"],
            false,
        )
        .unwrap();

        let err =
            guard_visible_write_reconcile_with_target(&doc, "test_operator_edit", expected, None)
                .expect_err("a genuine unsaved operator edit must fail closed");
        assert!(
            err.to_string().contains("visible editor buffer"),
            "expected fail-closed live-buffer guard error: {err}"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_deferred_live_buffer_changed"),
            "operator-edit divergence must record the fail-closed block: {log}"
        );
        assert!(
            !log.contains("visible_write_replica_churn_reconcile"),
            "operator-edit divergence must NOT be treated as replica churn: {log}"
        );
    }

    #[test]
    fn broadcast_editor_change_writes_targeted_peer_patch_file() {
        let disk = concat!(
            "# Doc\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#base] existing\n",
            "<!-- /agent:backlog -->\n"
        );
        let origin = disk.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-A] queued in editor A\n",
        );
        let peer = disk.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-B] queued in editor B\n",
        );
        let (_dir, file, canonical) = temp_doc(disk);
        std::fs::create_dir_all(file.parent().unwrap().join(".agent-doc/patches")).unwrap();

        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &origin,
            Some("editor-A"),
        )
        .unwrap();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &peer,
            Some("editor-B"),
        )
        .unwrap();

        let deliveries = broadcast_editor_change(&file, "editor-A", &origin).unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].editor_id, "editor-B");
        assert!(
            deliveries[0]
                .patch_file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".editor-B.json")
        );

        let payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&deliveries[0].patch_file).unwrap())
                .unwrap();
        assert_eq!(payload["editor_id"], "editor-B");
        assert_eq!(payload["origin_editor_id"], "editor-A");
        assert_eq!(payload["file"], canonical);
        assert_eq!(
            payload["baseline_hash"],
            agent_doc_hash::content_hash(&peer)
        );
        assert_eq!(
            payload["baseline_normalized_hash"],
            agent_doc_hash::content_hash(
                &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    &peer
                )
            )
        );
        assert_eq!(payload["patches"][0]["component"], "backlog");
        assert_eq!(payload["patches"][0]["op"], "replace");
        let body = payload["patches"][0]["content"].as_str().unwrap();
        assert!(body.contains("#edit-A"));
        assert!(body.contains("#edit-B"));
        let node_patches = payload["node_patches"].as_array().unwrap();
        assert!(
            node_patches
                .iter()
                .any(|patch| patch["component"] == "backlog"
                    && patch["op"] == "insert"
                    && patch["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("#edit-A"))),
            "targeted broadcast must carry node-keyed insert for peer apply:\n{payload:#?}"
        );
        assert_eq!(deliveries[0].node_patch_count, node_patches.len());
        assert_eq!(deliveries[0].component_patch_count, 1);
    }

    #[test]
    fn editor_id_is_live_filters_dead_jetbrains_pids_only() {
        let me = std::process::id();
        assert!(
            editor_id_is_live(&format!("jetbrains-{me}-abc-uuid")),
            "own live pid"
        );
        // A pid near the max is overwhelmingly likely dead (kill(pid,0) → ESRCH).
        assert!(
            !editor_id_is_live("jetbrains-2147483646-dead-uuid"),
            "dead high pid"
        );
        assert!(
            editor_id_is_live("vscode-123"),
            "non-jetbrains treated live"
        );
        assert!(
            editor_id_is_live("editor-A"),
            "no embedded pid treated live"
        );
        assert!(
            editor_id_is_live("jetbrains-notapid-uuid"),
            "malformed pid treated live"
        );
        assert_eq!(jetbrains_editor_id_pid("jetbrains-4242-uuid"), Some(4242));
        assert_eq!(jetbrains_editor_id_pid("vscode-1"), None);
        assert_eq!(jetbrains_editor_id_pid("jetbrains--uuid"), None);
    }

    #[test]
    fn broadcast_editor_change_skips_dead_origin() {
        let disk = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "base\n",
            "<!-- /agent:exchange -->\n"
        );
        let origin = disk.replace("base\n", "base\norigin edit\n");
        let peer = disk.replace("base\n", "base\npeer edit\n");
        let (_dir, file, canonical) = temp_doc(disk);
        std::fs::create_dir_all(file.parent().unwrap().join(".agent-doc/patches")).unwrap();
        // A live peer exists, but the originator's IntelliJ pid is dead.
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &peer,
            Some("editor-B"),
        )
        .unwrap();
        let deliveries =
            broadcast_editor_change(&file, "jetbrains-2147483646-dead", &origin).unwrap();
        assert!(
            deliveries.is_empty(),
            "a dead originator must not broadcast"
        );
    }

    // ── `#rtheal` — auto-heal a stale-behind committed editor buffer ──

    /// Build a temp git repo containing `doc.md`, apply each `commits` snapshot
    /// as its own commit in order, and return `(TempDir, file, canonical)` with
    /// the working tree at the LAST snapshot (== HEAD). The canonical path string
    /// keys the live-buffer sidecar, matching what the editor plugin reports.
    fn temp_git_doc(commits: &[&str]) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        let file = root.join("doc.md");
        for (i, body) in commits.iter().enumerate() {
            std::fs::write(&file, body).unwrap();
            git(&["add", "doc.md"]);
            git(&["commit", "-m", &format!("commit {i}")]);
        }
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    const HEAL_A: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] first\n",
        "- [ ] [#b] second\n",
        "- [ ] [#c] third\n",
        "<!-- /agent:backlog -->\n",
    );
    const HEAL_B: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] first\n",
        "- [ ] [#b] second\n",
        "- [ ] [#c] third\n",
        "- [ ] [#d] fourth\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: work — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn stale_behind_committed_buffer_heals_when_buffer_is_older_commit() {
        // disk == HEAD == B; the editor buffer still shows the committed ancestor A.
        let (_dir, file, canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        let disk = HEAL_B;
        agent_doc_debounce::record_live_buffer_digest_content(&canonical, HEAL_A).unwrap();

        // Detector proves the divergent buffer is a stale committed ancestor.
        let stale = stale_behind_committed_buffer(&file, disk)
            .expect("buffer holding committed ancestor A must be detected as stale-behind");
        assert_eq!(stale.content.as_deref(), Some(HEAL_A));

        // resolve_current_doc makes disk authoritative (content B, NOT the stale A).
        // The IPC refresh no-ops with no live editor — that is fine.
        let r = resolve_current_doc(&file, disk);
        assert_eq!(r.authority, agent_doc_document_realtime::DocAuthority::Disk);
        assert_eq!(r.content, disk);
        assert!(r.content.contains("#d"), "healed to disk B (has #d)");

        // An autoheal ops-log marker was emitted.
        let ops_log = file.parent().unwrap().join(".agent-doc/logs/ops.log");
        if let Ok(log) = std::fs::read_to_string(&ops_log) {
            assert!(
                log.contains("realtime_doc_autoheal")
                    && log.contains("reason=stale_behind_committed_ancestor"),
                "autoheal marker should be logged:\n{log}"
            );
        }
    }

    #[test]
    fn stale_behind_committed_buffer_never_heals_unsaved_ahead_edit() {
        // SAFETY INVARIANT: disk == HEAD == B, but the buffer holds a genuine
        // unsaved operator edit ("X") that matches NO commit. It must NEVER be
        // treated as stale-behind, and must still promote as today.
        let (_dir, file, canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        let disk = HEAL_B;
        let unsaved = format!("{HEAL_B}\n<!-- operator note -->\nGENUINELY UNSAVED X\n");
        agent_doc_debounce::record_live_buffer_digest_content(&canonical, &unsaved).unwrap();

        assert!(
            stale_behind_committed_buffer(&file, disk).is_none(),
            "an unsaved-ahead buffer matching no commit must not be treated stale-behind"
        );

        // Existing behavior preserved: the buffer wins and its unsaved content shows.
        let r = resolve_current_doc(&file, disk);
        assert_eq!(
            r.authority,
            agent_doc_document_realtime::DocAuthority::EditorBuffer,
            "unsaved operator work must still be promoted, never clobbered"
        );
        assert!(r.content.contains("GENUINELY UNSAVED X"));
    }

    #[test]
    fn stale_behind_committed_buffer_none_when_disk_not_head() {
        // Uncommitted disk changes: disk != HEAD. Conservative fall-through even
        // if the buffer happens to equal a commit.
        let (_dir, file, canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        let disk = concat!(
            "# Doc\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] first\n",
            "- [ ] [#b] second\n",
            "- [ ] [#c] third\n",
            "- [ ] [#d] fourth\n",
            "- [ ] [#e] uncommitted work on disk\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&file, disk).unwrap();
        // Buffer shows committed ancestor A, but disk != HEAD, so no heal.
        agent_doc_debounce::record_live_buffer_digest_content(&canonical, HEAL_A).unwrap();
        assert!(
            stale_behind_committed_buffer(&file, disk).is_none(),
            "disk != HEAD must conservatively fall through (no heal)"
        );
    }

    #[test]
    fn content_matches_recent_committed_blob_true_for_prior_commit_false_for_uncommitted() {
        // Two commits: A then B (HEAD). Both A and B are committed blobs; a string
        // that was never committed must not match.
        let (_dir, file, _canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        assert!(
            content_matches_recent_committed_blob(&file, HEAL_A, 15),
            "the older committed version A must be recognized as a committed blob"
        );
        assert!(
            content_matches_recent_committed_blob(&file, HEAL_B, 15),
            "the HEAD committed version B must be recognized as a committed blob"
        );
        assert!(
            !content_matches_recent_committed_blob(&file, "never committed content\n", 15),
            "content that matches no commit must return false"
        );
        // A tiny limit still finds the most recent commit (B) but can miss the
        // older one; the never-committed content stays false regardless.
        assert!(content_matches_recent_committed_blob(&file, HEAL_B, 1));
        assert!(!content_matches_recent_committed_blob(
            &file,
            "still never\n",
            1
        ));
    }

    #[test]
    fn content_matches_recent_committed_blob_false_outside_git_repo() {
        // No git repo (best-effort): any git error → false, never a panic.
        let (_dir, file, _canonical) = temp_doc("plain body\n");
        assert!(!content_matches_recent_committed_blob(
            &file,
            "plain body\n",
            15
        ));
    }

    #[test]
    fn stale_behind_committed_buffer_none_when_len_hash_only_snapshot() {
        // A len/hash-only digest (no full content) cannot be proven a committed
        // ancestor even though it diverges → None.
        let (_dir, file, canonical) = temp_git_doc(&[HEAL_A, HEAL_B]);
        let disk = HEAL_B;
        agent_doc_debounce::record_live_buffer_digest(
            &canonical,
            HEAL_A.len(),
            &agent_doc_hash::content_hash(HEAL_A),
        )
        .unwrap();
        assert!(
            stale_behind_committed_buffer(&file, disk).is_none(),
            "a content-absent digest cannot be proven a committed ancestor"
        );
    }

    #[test]
    fn broadcast_editor_change_drops_and_reaps_dead_peer() {
        let disk = concat!(
            "# Doc\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "base\n",
            "<!-- /agent:exchange -->\n"
        );
        let origin = disk.replace("base\n", "base\norigin edit\n");
        let dead_peer_buf = disk.replace("base\n", "base\ndead stale\n");
        let (_dir, file, canonical) = temp_doc(disk);
        std::fs::create_dir_all(file.parent().unwrap().join(".agent-doc/patches")).unwrap();
        let dead_id = "jetbrains-2147483646-deadpeer";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &dead_peer_buf,
            Some(dead_id),
        )
        .unwrap();
        assert!(
            agent_doc_debounce::live_buffer_snapshots(&canonical)
                .iter()
                .any(|s| s.editor_id.as_deref() == Some(dead_id)),
            "dead peer sidecar present before broadcast"
        );

        let deliveries = broadcast_editor_change(&file, "editor-A", &origin).unwrap();
        assert!(deliveries.is_empty(), "no delivery to a dead peer");
        assert!(
            !agent_doc_debounce::live_buffer_snapshots(&canonical)
                .iter()
                .any(|s| s.editor_id.as_deref() == Some(dead_id)),
            "dead peer orphan sidecar must be reaped"
        );
    }
}
