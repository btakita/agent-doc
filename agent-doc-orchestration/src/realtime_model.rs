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
//! integration; all deterministic behavior in the binary), orchestration owns
//! only the durable editor-buffer feed and ops-log adapter. Cycle read sites
//! (`preflight.rs` / `write.rs` / `session_check.rs`) source current-doc through
//! [`resolve_current_doc`], which delegates to the focused pure policy.
//!
//! ## Evals
//! - `durable_buffer_state_none_when_buffer_in_sync_with_disk`
//! - `durable_buffer_state_wins_when_unsaved_buffer_ahead_of_disk`
//! - `durable_buffer_state_none_when_no_editor_feed`

use agent_doc_document_realtime::{BufferState, Reconciliation, reconcile_current_doc};

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

/// Resolve the authoritative "current document" for a cycle read: reconcile the
/// on-disk `disk` content against the durable editor-buffer feed for `file`.
///
/// This is the single entry point rung 3 (`#rtwwire`) wires into the cycle read
/// sites (`preflight` / `write` / `session-check`) so the agent reads
/// newest-of(disk, editor buffer) instead of bare disk. Emits a grep-able
/// `realtime_doc_resolve` ops.log marker so a live edit-during-finalize run can
/// prove which source won.
pub fn resolve_current_doc(file: &std::path::Path, disk: &str) -> Reconciliation {
    let buffer = durable_buffer_state(file, disk);
    let reconciliation = reconcile_current_doc(disk, buffer.as_ref());
    crate::ops_log::log_op(
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

/// The conflict-free CRDT union two open editors should converge on after one of
/// them edits a shared document, plus an echo-suppress signal for the originator.
#[derive(Debug, Clone)]
pub struct BroadcastMerge {
    /// The CRDT union of both editor buffers over the shared on-disk base.
    pub merged: String,
    /// `true` when the originating editor's buffer already equals `merged` (the
    /// peer contributed nothing new), so re-delivering the merge back to the
    /// originator would be a redundant self-echo and must be suppressed.
    pub originator_echo_suppressed: bool,
}

/// One peer editor buffer participating in a multi-editor broadcast merge.
#[derive(Debug, Clone)]
pub struct BroadcastPeer {
    pub editor_id: String,
    pub content: String,
}

impl BroadcastPeer {
    pub fn new(editor_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            editor_id: editor_id.into(),
            content: content.into(),
        }
    }
}

/// A target editor that should receive a merged document update.
#[derive(Debug, Clone)]
pub struct BroadcastTarget {
    pub editor_id: String,
    pub merged: String,
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

/// `#rtwbcast` Option C — the MERGE-ONLY multi-editor broadcast seam.
///
/// Given the shared on-disk `base` and two open editor buffers (`originator`
/// typed the change that triggered this broadcast; `peer` is another editor's
/// buffer), produce the conflict-free CRDT union both editors should converge on,
/// plus the originator echo-suppress flag. This is a pure function: no I/O, no
/// clock, and — deliberately — **no delivery**. It is the convergence math only.
///
/// Wiring the per-editor delivery channel (an `editor_id` on the FFI buffer-report
/// and patch JSON, per-editor live-buffer sidecars, per-peer patch files, and the
/// originator skip) is the separate, operator-gated `#rtwbcast-id`/`-deliver`
/// rungs in `plan-realtime-editor-watcher.md`: those touch the FFI ABI and both
/// editor plugins and require a two-live-IDE verify, so they are NOT landed here.
/// This seam only names and unit-tests the merge so the SimWorld two-editor
/// coverage exercises the same production convergence path the delivery rungs
/// will reuse.
pub fn compute_broadcast(
    base: &str,
    originator: &str,
    peer: &str,
) -> anyhow::Result<BroadcastMerge> {
    if text_delta_included(base, peer, originator) {
        return Ok(BroadcastMerge {
            merged: originator.to_string(),
            originator_echo_suppressed: true,
        });
    }
    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
    let (merged, _state) =
        agent_doc_merge::merge_contents_crdt(Some(&base_state), originator, peer)?;
    let originator_echo_suppressed = merged == originator;
    Ok(BroadcastMerge {
        merged,
        originator_echo_suppressed,
    })
}

fn text_delta_included(base: &str, changed: &str, candidate: &str) -> bool {
    if changed == base || changed == candidate {
        return true;
    }
    let base_counts = line_counts(base);
    let changed_counts = line_counts(changed);
    let candidate_counts = line_counts(candidate);
    let mut saw_delta = false;
    for line in base_counts.keys().chain(changed_counts.keys()) {
        let base_count = *base_counts.get(line).unwrap_or(&0);
        let changed_count = *changed_counts.get(line).unwrap_or(&0);
        let candidate_count = *candidate_counts.get(line).unwrap_or(&0);
        if changed_count != base_count {
            saw_delta = true;
        }
        if changed_count > base_count && candidate_count < changed_count {
            return false;
        }
        if changed_count < base_count && candidate_count > changed_count {
            return false;
        }
    }
    saw_delta
}

fn line_counts(text: &str) -> std::collections::BTreeMap<&str, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for line in text.split_inclusive('\n') {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

/// Production-shaped N-buffer broadcast planner.
///
/// The originator is never returned as a target. Each peer receives a CRDT union
/// of the originator buffer and that peer's current buffer over the shared disk
/// base, unless the peer already equals that merged result.
pub fn compute_broadcast_plan(
    base: &str,
    originator_editor_id: &str,
    originator: &str,
    peers: &[BroadcastPeer],
) -> anyhow::Result<Vec<BroadcastTarget>> {
    let mut targets = Vec::new();
    for peer in peers {
        if peer.editor_id == originator_editor_id {
            continue;
        }
        let merge = compute_broadcast(base, originator, &peer.content)?;
        if merge.merged == peer.content {
            continue;
        }
        targets.push(BroadcastTarget {
            editor_id: peer.editor_id.clone(),
            merged: merge.merged,
        });
    }
    Ok(targets)
}

/// Parse the owning process id from a JetBrains plugin editor id
/// (`jetbrains-<pid>-<uuid>`). Returns `None` for non-JetBrains editor ids
/// (e.g. `vscode-…`) or malformed ids — callers treat those as live.
fn jetbrains_editor_id_pid(editor_id: &str) -> Option<u32> {
    let rest = editor_id.strip_prefix("jetbrains-")?;
    let pid_str = rest.split('-').next()?;
    if pid_str.is_empty() || !pid_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid_str.parse::<u32>().ok()
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
        Some(pid) => crate::hooks::pid_is_live(pid),
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
        crate::ops_log::log_op(
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
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
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
        crate::ops_log::log_op(
            file,
            &format!(
                "realtime_broadcast_dead_peers_reaped file={} count={} reason=dead_editor_pid",
                file.display(),
                reaped_dead_peers
            ),
        );
    }
    let targets = compute_broadcast_plan(&disk, originator_editor_id, originator_content, &peers)?;
    let doc_hash = crate::snapshot::doc_hash(&canonical)?;
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
        crate::write::atomic_write_pub(&patch_file, &serde_json::to_string_pretty(&payload)?)?;
        crate::ops_log::log_op(
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
    let node_patches = crate::write::build_ipc_node_patches_json(Some(&peer.content), Some(merged));
    let patches = broadcast_convergence_patches(&peer.content, merged)?;
    let frontmatter = raw_frontmatter_yaml(merged)
        .filter(|merged_fm| raw_frontmatter_yaml(&peer.content) != Some(*merged_fm))
        .map(ToString::to_string);
    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        crate::ops_log::log_op(
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

fn broadcast_convergence_patches(
    before: &str,
    after: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let before_components = agent_doc_element::element::parse(before)?;
    let after_components = agent_doc_element::element::parse(after)?;
    let before_by_name: std::collections::HashMap<&str, &agent_doc_element::element::Component> =
        before_components
            .iter()
            .map(|component| (component.name.as_str(), component))
            .collect();
    let mut patches = Vec::new();
    for after_component in &after_components {
        let Some(before_component) = before_by_name.get(after_component.name.as_str()) else {
            continue;
        };
        let before_body = before_component.content(before);
        let after_body = after_component.content(after);
        if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(before_body)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                after_body,
            )
        {
            continue;
        }
        patches.push(serde_json::json!({
            "component": after_component.name,
            "content": after_body,
            "op": "replace",
        }));
    }
    Ok(patches)
}

fn raw_frontmatter_yaml(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn sanitize_editor_id_for_filename(editor_id: &str) -> String {
    let sanitized: String = editor_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "editor".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_broadcast_unions_two_editor_buffers_conflict_free() {
        // #rtwbcast Option C: two editors edit a shared base; the seam produces the
        // conflict-free union, and the originator is not echo-suppressed because the
        // peer contributed a genuine new edit.
        let base = "shared line one\nshared line two\n";
        let originator = "shared line one\noriginator edit\nshared line two\n";
        let peer = "shared line one\nshared line two\npeer edit\n";
        let result = compute_broadcast(base, originator, peer).unwrap();
        assert!(result.merged.contains("originator edit"));
        assert!(result.merged.contains("peer edit"));
        for marker in ["<<<<<<<", "=======", ">>>>>>>"] {
            assert!(
                !result.merged.contains(marker),
                "broadcast merge must be conflict-free; found `{marker}`"
            );
        }
        assert!(
            !result.originator_echo_suppressed,
            "a peer edit means the merge differs from the originator → no echo suppression"
        );
    }

    #[test]
    fn compute_broadcast_suppresses_echo_when_peer_unchanged() {
        // When the peer buffer equals the base (it made no edit), the merge equals
        // the originator's buffer, so re-delivering to the originator is a redundant
        // self-echo and is suppressed.
        let base = "shared line\n";
        let originator = "shared line\noriginator edit\n";
        let peer = base;
        let result = compute_broadcast(base, originator, peer).unwrap();
        assert_eq!(result.merged, originator);
        assert!(
            result.originator_echo_suppressed,
            "an unchanged peer must suppress the redundant echo back to the originator"
        );
    }

    #[test]
    fn compute_broadcast_rebroadcast_preserves_component_boundaries() {
        let base = concat!(
            "# Doc\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#base] existing\n",
            "<!-- /agent:backlog -->\n"
        );
        let editor_a = base.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-A] queued in editor A\n",
        );
        let editor_b = base.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-B] queued in editor B\n",
        );

        let merged = compute_broadcast(base, &editor_a, &editor_b)
            .unwrap()
            .merged;
        assert!(merged.contains("#edit-A"));
        assert!(merged.contains("#edit-B"));
        agent_doc_element::element::parse(&merged).unwrap();

        let rebroadcast = compute_broadcast(base, &merged, &editor_a).unwrap();
        assert_eq!(
            rebroadcast.merged, merged,
            "rebroadcasting an already-converged buffer to a stale peer must not re-merge component markers"
        );
        assert!(rebroadcast.originator_echo_suppressed);
        agent_doc_element::element::parse(&rebroadcast.merged).unwrap();
    }

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
    fn compute_broadcast_plan_skips_originator_and_unchanged_peer() {
        let base = "one\n";
        let origin = "one\norigin\n";
        let peer_changed = BroadcastPeer::new("peer-a", "one\npeer\n");
        let peer_unchanged = BroadcastPeer::new("peer-b", origin);
        let origin_peer = BroadcastPeer::new("origin", "one\norigin peer should skip\n");

        let targets = compute_broadcast_plan(
            base,
            "origin",
            origin,
            &[peer_changed, peer_unchanged, origin_peer],
        )
        .unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].editor_id, "peer-a");
        assert!(targets[0].merged.contains("origin"));
        assert!(targets[0].merged.contains("peer"));
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
