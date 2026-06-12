//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Read the ack-content sidecar file written by the plugin after apply.
/// Keyed by `patch_id` (same UUID the binary embedded in the patch payload).
/// Deletes the sidecar on success. Returns None if no sidecar present (old plugin).
pub(crate) fn read_ack_content_sidecar(project_root: &Path, patch_id: &str) -> Result<Option<String>> {
    let sidecar = project_root
        .join(".agent-doc/ack-content")
        .join(format!("{patch_id}.md"));
    if !sidecar.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("failed to read ack-content sidecar {sidecar:?}"))?;
    let _ = std::fs::remove_file(&sidecar);
    Ok(Some(content))
}

/// Remove any stale ipc-degraded marker left by older versions.
/// Extract `### Re:` response headings from a slice of `PatchBlock`s.
///
/// Used by the late-fallback gate to decide whether an "already committed"
/// cycle's state belongs to the incoming response (skip the apply) or to a
/// different operation that landed mid-turn (rotate the cycle and apply).
///
/// Only the leading `### Re: ...` line of each patch's content is considered.
/// Section bodies and subheadings are ignored so callers can compare against
/// HEAD content via a substring check without false positives from common
/// boilerplate. Returns the trimmed heading lines (without the trailing
/// newline) in order of appearance.
pub(crate) fn extract_response_headings_from_patches(patches: &[crate::template::PatchBlock]) -> Vec<String> {
    let mut out = Vec::new();
    for patch in patches {
        for line in patch.content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Re:") {
                out.push(trimmed.to_string());
                break;
            }
        }
    }
    out
}

/// Return `true` when every `### Re:` response heading carried in the
/// incoming patches is already present in the document's `HEAD` content.
///
/// Used inside the late-fallback gate (see `#adoc-compact-during-turn-response-loss`)
/// to distinguish:
/// - "cycle committed because this response already landed" (skip apply), and
/// - "cycle committed by an unrelated mid-turn operation, but the response
///   is still waiting to be written" (rotate the cycle, apply the patch).
///
/// Returns `true` when there are no headings to check (no patches), which
/// preserves the gate's previous conservative behavior for empty patch lists.
/// Returns `false` if `git show HEAD:<file>` fails — the caller treats that
/// the same as "not in HEAD" and rotates the cycle, which is fail-safe for
/// the mid-turn race.
pub(crate) fn patch_response_headings_already_in_head(
    file: &Path,
    patches: &[crate::template::PatchBlock],
) -> bool {
    let headings = extract_response_headings_from_patches(patches);
    if headings.is_empty() {
        return true;
    }
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let Some(head) = rc.head_content() else {
        return false;
    };
    headings.iter().all(|h| head.contains(h.as_str()))
}

pub(crate) fn cleanup_legacy_ipc_degraded(project_root: &Path) {
    let marker = project_root.join(".agent-doc/ipc-degraded");
    if marker.is_file()
        && let Err(e) = std::fs::remove_file(&marker)
    {
        eprintln!(
            "[write] WARNING: failed to remove legacy IPC degraded marker {}: {}",
            marker.display(),
            e
        );
    }
}

pub(crate) const IPC_DEWEDGE_TIMEOUT_THRESHOLD: u64 = 2;

pub(crate) fn ipc_dewedge_session_id(file: &Path) -> String {
    frontmatter::read_session_id(file).unwrap_or_else(|| "-".to_string())
}

pub(crate) fn ipc_dewedge_marker_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    let hash = snapshot::doc_hash(file)?;
    Ok(project_root
        .join(".agent-doc/ipc-degraded")
        .join(format!("{hash}.json")))
}

pub(crate) fn ipc_dewedge_marker_for_current_session(
    project_root: &Path,
    file: &Path,
) -> Result<Option<serde_json::Value>> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if !marker.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marker)
        .with_context(|| format!("failed to read IPC degraded marker {}", marker.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse IPC degraded marker {}", marker.display()))?;
    let marker_session = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    if marker_session != ipc_dewedge_session_id(file) {
        return Ok(None);
    }
    Ok(Some(value))
}

pub(crate) fn ipc_direct_disk_degraded(project_root: &Path, file: &Path) -> Result<bool> {
    let degraded = ipc_dewedge_marker_for_current_session(project_root, file)?
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !degraded {
        return Ok(false);
    }
    // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not a
    // permanent session verdict. Once marked degraded the write path skips the
    // socket, so it would otherwise never observe a recovered listener and would
    // stay disk-only until the session restarts. Re-probe listener liveness: if
    // the plugin's socket is accepting connections again, clear the latch and
    // resume the reliable plugin (IPC) path immediately. The probe only runs
    // while degraded (rare after the false-vote fixes), so it adds no cost to the
    // healthy path.
    if crate::ipc_socket::is_listener_active(project_root) {
        remove_ipc_dewedge_marker(project_root, file, "listener_recovered")?;
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn log_ipc_dewedge_direct_disk_skip(file: &Path, transport: &str) {
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_listener_degraded_direct_disk file={} transport={} reason=repeated_ack_timeout",
            file.display(),
            transport
        ),
    );
}

/// `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket means only the
/// plugin's *socket* listener is wedged. The file-IPC patch queue uses a
/// separate plugin file watcher that is very likely still alive, so a degraded
/// write routes through it (the plugin applies via the Document API) instead of
/// a raw disk write that manufactures an IDEA "File Cache Conflict". A direct
/// disk write becomes the true last resort, reached only when file IPC also
/// fails to deliver.
pub(crate) fn log_ipc_dewedge_prefer_file_ipc(file: &Path, transport: &str) {
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_socket_degraded_prefer_file_ipc file={} transport={} reason=repeated_ack_timeout disk_write=last_resort",
            file.display(),
            transport
        ),
    );
}

pub(crate) fn record_ipc_socket_ack_timeout(
    project_root: &Path,
    file: &Path,
    patch_id: Option<&str>,
    transport: &str,
) -> Result<bool> {
    cleanup_legacy_ipc_degraded(project_root);
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create IPC degraded marker directory {}",
                parent.display()
            )
        })?;
    }
    let prior = ipc_dewedge_marker_for_current_session(project_root, file)?;
    let prior_timeouts = prior
        .as_ref()
        .and_then(|value| value.get("consecutive_timeouts").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let consecutive_timeouts = prior_timeouts.saturating_add(1);
    let degraded = consecutive_timeouts >= IPC_DEWEDGE_TIMEOUT_THRESHOLD;
    let value = serde_json::json!({
        "session_id": ipc_dewedge_session_id(file),
        "consecutive_timeouts": consecutive_timeouts,
        "degraded": degraded,
        "last_patch_id": patch_id.unwrap_or("-"),
        "last_transport": transport,
    });
    atomic_write(&marker, &serde_json::to_string_pretty(&value)?)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_socket_ack_timeout_recorded file={} transport={} patch_id={} consecutive_timeouts={} degraded={}",
            file.display(),
            transport,
            patch_id.unwrap_or("-"),
            consecutive_timeouts,
            degraded
        ),
    );
    Ok(degraded)
}

pub(crate) fn is_socket_ack_timeout_error(err: &anyhow::Error) -> bool {
    // Duration-agnostic: the sender's ack timeout budget is configurable
    // (`IPC_ACK_TIMEOUT_SECS` in ipc_socket.rs), so match the stable prefix
    // rather than a hard-coded "(2s)".
    err.to_string().contains("IPC ack timeout")
}

pub(crate) fn remove_ipc_dewedge_marker(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if marker.exists() {
        std::fs::remove_file(&marker).with_context(|| {
            format!("failed to remove IPC degraded marker {}", marker.display())
        })?;
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_ack_timeouts_cleared file={} reason={}",
                file.display(),
                reason
            ),
        );
    }
    Ok(())
}

pub(crate) fn clear_ipc_socket_ack_timeouts(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let Some(value) = ipc_dewedge_marker_for_current_session(project_root, file)? else {
        return Ok(());
    };
    // A routine successful write clears accrued timeout votes, but it must NOT
    // clear a *degraded* latch on its own — degraded means the write path is
    // already on the disk/file-IPC fallback, so a "success" here is not proof
    // the socket listener recovered. The degraded latch is cleared only by a
    // proven-live listener re-probe (`#ipc-degrade-self-heal`, see
    // `ipc_direct_disk_degraded`).
    if value
        .get("degraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    remove_ipc_dewedge_marker(project_root, file, reason)
}

/// Poll for the ack-content sidecar with timeout.
///
/// The plugin writes the sidecar asynchronously after applying the patch.
/// Polling eliminates the old 200ms sleep race — we get the authoritative
/// post-apply content as soon as the plugin writes it, or fall back to
/// file read only after the timeout expires.
pub(crate) fn poll_ack_content_sidecar(
    project_root: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<String>> {
    let start = std::time::Instant::now();
    loop {
        match read_ack_content_sidecar(project_root, patch_id)? {
            Some(content) => return Ok(Some(content)),
            None if start.elapsed() >= timeout => return Ok(None),
            None => std::thread::sleep(poll_interval),
        }
    }
}

pub(crate) fn content_ours_with_pending_from_disk(file: &Path, content_ours: &str) -> String {
    match std::fs::read_to_string(file) {
        Ok(on_disk_content) => splice_pending_component(content_ours, &on_disk_content),
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to read {} while preserving pending mutations during normalization fallback: {}",
                file.display(),
                e
            );
            content_ours.to_string()
        }
    }
}

pub(crate) fn content_ours_merged_with_disk_edits(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
) -> String {
    let Some(base) = baseline else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    let Ok(on_disk_content) = std::fs::read_to_string(file) else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    if strip_boundary_for_dedup(&on_disk_content) == strip_boundary_for_dedup(content_ours) {
        return content_ours.to_string();
    }
    if response_already_in_current(base, content_ours, &on_disk_content) {
        eprintln!(
            "[write] normalization fallback: response delta already in current file; adopting current content"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_adopted_current_delta file={} delta=response_contained",
                file.display()
            ),
        );
        return on_disk_content;
    }

    let base_state = match snapshot::crdt_merge_base_state(file, base) {
        Ok(base) => base.state,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to load overlay CRDT merge base, falling back to baseline text: {}",
                e
            );
            crate::crdt::CrdtDoc::from_text(base).encode_state()
        }
    };
    match merge::merge_contents_crdt(Some(&base_state), content_ours, &on_disk_content) {
        Ok((merged, _)) => merged,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to merge current disk edits into normalization fallback: {}",
                e
            );
            content_ours_with_pending_from_disk(file, content_ours)
        }
    }
}

pub(crate) fn normalized_content_ours_fallback(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
    normalize_prefix_lines: &[String],
) -> String {
    let fallback = content_ours_merged_with_disk_edits(file, baseline, content_ours);
    let normalized = normalize_exchange_prefixes_for_targets(&fallback, normalize_prefix_lines);
    repair_duplicate_prompt_artifacts(
        &normalized,
        file,
        DuplicatePromptRepairOptions::new("normalization_fallback")
            .with_before(baseline)
            .preserving(baseline)
            .without_residue_guard(),
    )
    .map(|(repaired, _)| repaired)
    .unwrap_or(normalized)
}

pub(crate) fn repair_disk_from_normalization_fallback(file: &Path, fallback: &str) -> Result<()> {
    guard_visible_write_idle(file, "sidecar_normalization_fallback_repair")?;
    atomic_write(file, fallback).with_context(|| {
        format!(
            "failed to repair {} from normalized content_ours fallback",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_repaired_working_tree file={} bytes={}",
            file.display(),
            fallback.len()
        ),
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IpcSnapshotSource {
    AckContentSidecar,
    ContentOurs,
    FileRead,
}

impl IpcSnapshotSource {
    fn label(self) -> &'static str {
        match self {
            Self::AckContentSidecar => "ack_content_sidecar",
            Self::ContentOurs => "content_ours",
            Self::FileRead => "file_read",
        }
    }

    fn is_ack_content_proven(self) -> bool {
        matches!(self, Self::AckContentSidecar)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IpcDiskRepairReason {
    PrefixDivergence,
    IpcDedupe,
    PrefixDivergenceThenIpcDedupe,
}

impl IpcDiskRepairReason {
    fn label(self) -> &'static str {
        match self {
            Self::PrefixDivergence => "prefix_divergence",
            Self::IpcDedupe => "ipc_dedupe",
            Self::PrefixDivergenceThenIpcDedupe => "prefix_divergence_then_ipc_dedupe",
        }
    }

    fn redelivery_kind(self) -> FullContentRepairRedelivery {
        match self {
            Self::PrefixDivergence => FullContentRepairRedelivery::NormalizationFallback,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe => {
                FullContentRepairRedelivery::IpcDedupe
            }
        }
    }

    fn merge_with_ipc_dedupe(self) -> Self {
        match self {
            Self::PrefixDivergence => Self::PrefixDivergenceThenIpcDedupe,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe => self,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditorBadStateFingerprint {
    pub(crate) content: String,
    pub(crate) len: usize,
    pub(crate) hash: String,
}

impl EditorBadStateFingerprint {
    fn new(content: String) -> Self {
        let len = content.len();
        let hash = crate::ops_log::content_hash(&content);
        Self { content, len, hash }
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IpcRepairDecision {
    pub(crate) snapshot_content: String,
    pub(crate) snap_source: IpcSnapshotSource,
    pub(crate) disk_repair_reason: Option<IpcDiskRepairReason>,
    pub(crate) editor_bad_state: Option<EditorBadStateFingerprint>,
    pub(crate) normalize_prefix_lines: Vec<String>,
    pub(crate) redeliver_editor: bool,
}

impl IpcRepairDecision {
    pub(crate) fn ack_content(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::AckContentSidecar,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    pub(crate) fn content_ours(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    pub(crate) fn content_ours_prefix_fallback(
        snapshot_content: String,
        bad_state: String,
        normalize_prefix_lines: &[String],
    ) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: Some(IpcDiskRepairReason::PrefixDivergence),
            editor_bad_state: Some(EditorBadStateFingerprint::new(bad_state)),
            normalize_prefix_lines: normalize_prefix_lines.to_vec(),
            redeliver_editor: true,
        }
    }

    pub(crate) fn file_read(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::FileRead,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    pub(crate) fn apply_ipc_dedupe(
        mut self,
        snapshot_content: String,
        bad_state_before_dedupe: String,
    ) -> Self {
        self.snapshot_content = snapshot_content;
        self.disk_repair_reason = Some(match self.disk_repair_reason {
            Some(reason) => reason.merge_with_ipc_dedupe(),
            None => IpcDiskRepairReason::IpcDedupe,
        });
        if self.editor_bad_state.is_none() {
            self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state_before_dedupe));
        }
        self.redeliver_editor = self.editor_bad_state.is_some();
        self
    }

    fn ack_content_proven(&self) -> bool {
        self.snap_source.is_ack_content_proven()
    }

    fn replace_snapshot_with_content_ours_for_live_prompt_drift(&mut self, content_ours: &str) {
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.disk_repair_reason = None;
        self.editor_bad_state = None;
        self.normalize_prefix_lines.clear();
        self.redeliver_editor = false;
    }

    fn replace_snapshot_with_content_ours_for_prompt_duplication(
        &mut self,
        content_ours: &str,
        bad_state: String,
    ) {
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.disk_repair_reason = Some(IpcDiskRepairReason::IpcDedupe);
        self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state));
        self.normalize_prefix_lines.clear();
        self.redeliver_editor = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AlreadyAppliedSnapshotOutcome {
    Persisted,
    NeedsFileFallback,
}

pub(crate) fn guard_ipc_snapshot_adoption_against_live_prompt_drift(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let (Some(base), Some(ours)) = (baseline, content_ours) else {
        return false;
    };
    if !ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
        base,
        &decision.snapshot_content,
        ours,
    ) {
        return false;
    }

    let prior_source = decision.snap_source.label();
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::DocumentMutation,
            crate::flow::types::FlowStage::IpcSnapshotAdoption,
            crate::flow::types::FlowOutcome::Blocked,
        )
        .with_reason("live_prompt_drift_after_preflight"),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=live_prompt_drift_after_preflight candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "live_prompt_drift_after_preflight",
        "content_ours_snapshot_next_cycle",
        &format!(
            "snap_source={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    let _ = crate::cycle_state::record_ipc_snapshot_adoption_blocked(file);

    let candidate = decision.snapshot_content.clone();
    // #queue-user-edit-overwrite: a live queue deletion is authoritative — fold it
    // into content_ours first so both the fail-closed path and the #fintol2
    // forward-merge below reason about the user's reconciled queue.
    let queue_reconciled_ours = apply_live_queue_deletions_to_content_ours(base, &candidate, ours);
    if queue_reconciled_ours != ours {
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_content_ours_reconciled file={} source={} patch_id={} reason=live_queue_deletion_authoritative",
                file.display(),
                source,
                patch_id.unwrap_or("-")
            ),
        );
    }

    // #fintol2 — forward-merge tolerance for an independent concurrent edit. When
    // the user's concurrent edit is a DISJOINT, plain content edit outside
    // `exchange` (proven by `response_target_disjoint_from_user_edit`: confined
    // outside the response component, carrying no prompt/directive, and yielding a
    // conflict-free union that preserves both sides), commit that union so the
    // response lands AND the user's edit is preserved this cycle. Anything
    // prompt-/directive-bearing, in-`exchange`, or colliding returns false and
    // falls through to today's `content_ours_snapshot_next_cycle` carry-forward.
    if response_target_disjoint_from_user_edit(base, &queue_reconciled_ours, &candidate)
        && let Ok(union) = crate::merge::merge_contents(base, &queue_reconciled_ours, &candidate)
        && !union.contains("<<<<<<<")
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "live_prompt_drift_forward_merged file={} source={} patch_id={} candidate_len={} candidate_hash={} union_len={} union_hash={} reason=independent_concurrent_edit",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                candidate.len(),
                crate::ops_log::content_hash(&candidate),
                union.len(),
                crate::ops_log::content_hash(&union),
            ),
        );
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(&union);
        return true;
    }

    // #exchange-prompt-dropped-on-merge: persist the dropped user prompt lines
    // now, while the divergent candidate still carries them. The post-commit
    // session-check disk diff cannot win the race against an editor that
    // overwrites disk with the converged content_ours buffer, so the dropped
    // prompt guard reads this persisted evidence to fail closed instead.
    let dropped = dropped_prompt_lines_after_content_ours(base, &candidate, ours);
    if !dropped.is_empty() {
        if let Err(e) = crate::cycle_state::record_dropped_exchange_prompts(file, &dropped) {
            eprintln!(
                "[write] warning: failed to record dropped exchange prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "dropped_exchange_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped.len()
            ),
        );
    }
    // #queue-user-edit-overwrite: same silent-loss race for user-authored queue
    // edits. Record the dropped `do [#id]` queue lines now; session-check
    // filters them against committed HEAD (preserved or consumed → cleared,
    // silently deleted → fail closed).
    let dropped_queue = dropped_queue_prompt_lines_after_content_ours(
        base,
        &candidate,
        &queue_reconciled_ours,
    );
    if !dropped_queue.is_empty() {
        if let Err(e) = crate::cycle_state::record_dropped_queue_prompts(file, &dropped_queue) {
            eprintln!(
                "[write] warning: failed to record dropped queue prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "dropped_queue_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped_queue.len()
            ),
        );
    }
    decision.replace_snapshot_with_content_ours_for_live_prompt_drift(&queue_reconciled_ours);
    true
}

pub(crate) fn guard_ipc_snapshot_adoption_against_prompt_duplication(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let Some(ours) = content_ours else {
        return false;
    };
    let duplicate_count = user_prompt_count_growth(ours, &decision.snapshot_content);
    if duplicate_count == 0 {
        return false;
    }

    let prior_source = decision.snap_source.label();
    let bad_state = decision.snapshot_content.clone();
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::DocumentMutation,
            crate::flow::types::FlowStage::IpcSnapshotAdoption,
            crate::flow::types::FlowOutcome::Blocked,
        )
        .with_reason("prompt_duplication_in_ack_content"),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=prompt_duplication_in_ack_content duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "prompt_duplication_in_ack_content",
        "content_ours_snapshot_and_visible_repair",
        &format!(
            "snap_source={} duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    let _ = crate::cycle_state::record_ipc_snapshot_adoption_blocked(file);
    decision.replace_snapshot_with_content_ours_for_prompt_duplication(ours, bad_state);
    true
}

/// Emit a diagnostic for every IPC snapshot adoption that the two fail-closed
/// guards did NOT block. Blocked adoptions already log richly; allowed ones were
/// previously silent, so a corruption that slips through as "allowed" left no
/// trace. This symmetric `ipc_snapshot_adoption_allowed` line records the final
/// snapshot shape plus an independent drift/dup re-check (both must be benign on
/// an allowed path — a non-benign re-check here flags a guard-coverage gap).
pub(crate) fn log_ipc_snapshot_adoption_allowed(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &IpcRepairDecision,
    was_blocked: bool,
) {
    if was_blocked {
        return;
    }
    let drift_recheck = match (baseline, content_ours) {
        (Some(base), Some(ours)) => ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            base,
            &decision.snapshot_content,
            ours,
        ),
        _ => false,
    };
    let dup_recheck = content_ours
        .map(|ours| user_prompt_count_growth(ours, &decision.snapshot_content))
        .unwrap_or(0);
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_allowed file={} source={} patch_id={} snap_source={} snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={} drift_recheck={} dup_growth_recheck={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            content_ours.map(|o| o.len()).unwrap_or(0),
            content_ours
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            drift_recheck,
            dup_recheck,
        ),
    );
}

/// #ipcfullprompt-recur2 — default-on forensic capture. The fail-closed snapshot
/// guards above protect what gets *committed*, but a full-document editor-side
/// IPC mutation (e.g. `PatchWatcher.setText`) can still corrupt the
/// editor-visible buffer — deleting or duplicating a previously-committed
/// `### Re:` response block — while the user types a live prompt. This records
/// every such occurrence to `ops.log` and preserves the candidate buffer, so the
/// bug (which is not reliably reproducible) is captured the next time it happens
/// without any manual editor debug opt-in. Detection only: it never changes the
/// adoption decision — the guards above own that.
///
/// `candidate` must be the live editor buffer as received (capture it before the
/// guards replace `decision.snapshot_content`).
pub(crate) fn log_ipcfullprompt_corruption_if_any(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    candidate: &str,
) {
    // Scaffold duplication is a self-check on the candidate (the full-tail
    // duplication shape — two `<!-- /agent:exchange -->` markers — captured live in
    // brandon-cinquegrana.md), so it runs even when no baseline is available.
    let mut findings = crate::ipc_corruption::detect_duplicated_scaffold(candidate);
    // Response-block delete/duplicate needs the prior committed baseline.
    if let Some(base) = baseline {
        findings.extend(crate::ipc_corruption::detect_response_block_corruption(
            base, candidate,
        ));
    }
    if findings.is_empty() {
        return;
    }
    let base = baseline.unwrap_or("");
    let summary = crate::ipc_corruption::summarize_findings(&findings);
    crate::ops_log::log_op(
        file,
        &format!(
            "ipcfullprompt_corruption_suspected file={} source={} patch_id={} candidate_len={} candidate_hash={} baseline_len={} baseline_hash={} {}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            candidate.len(),
            crate::ops_log::content_hash(candidate),
            base.len(),
            crate::ops_log::content_hash(base),
            summary,
        ),
    );
    preserve_ipcfullprompt_forensic(file, patch_id, base, candidate);
}

/// Best-effort: preserve the baseline + corrupted candidate buffers under
/// `.agent-doc/logs/ipcfullprompt/` so the exact corruption shape can be analyzed
/// later (the plan's Phase-1 "preserve the pre/post for one failing cycle").
/// Never panics or returns errors.
pub(crate) fn preserve_ipcfullprompt_forensic(
    file: &Path,
    patch_id: Option<&str>,
    baseline: &str,
    candidate: &str,
) {
    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(root) = crate::fs_util::find_project_root(&canonical) else {
        return;
    };
    let dir = root.join(".agent-doc/logs/ipcfullprompt");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = format!("{}-{}", ts, patch_id.unwrap_or("nopatch"));
    let _ = std::fs::write(dir.join(format!("{stem}.baseline.md")), baseline);
    let _ = std::fs::write(dir.join(format!("{stem}.candidate.md")), candidate);
}

/// Recover a divergent live buffer that dropped the assistant response: when the
/// socket reports `already_applied` but the live buffer diverged with the
/// response fragmented out of `exchange`, materialize `expected_response` back
/// into the buffer's `exchange` so the response is never silently lost
/// (`#mrhpcdrift2` zero-UNRECOVERED-drift guarantee). Returns `Some(current)`
/// unchanged when the response is already materialized (no duplication), and
/// `None` when the buffer has no parseable `exchange` to repair into.
pub fn materialize_response_in_current_exchange(
    current: &str,
    expected_response: &str,
) -> Option<String> {
    let response = response_materialization_probe_from_response(expected_response);
    if response.trim().is_empty() || response_materialized_in_content(&response, current) {
        return Some(current.to_string());
    }
    let components = component::parse(current).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let mut exchange_body = exchange.content(current).to_string();
    push_materialization_segment(&mut exchange_body, &response);
    Some(exchange.replace_content(current, &exchange_body))
}

pub(crate) fn persist_already_applied_socket_content_ours_snapshot(
    file: &Path,
    patch_id: &str,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    expected_response: &str,
) -> Result<AlreadyAppliedSnapshotOutcome> {
    let Some(ours) = content_ours else {
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_no_content_ours_snapshot file={} patch_id={}",
                file.display(),
                patch_id
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::Persisted);
    };

    let current = std::fs::read_to_string(file).ok();
    let mut repair_decision = IpcRepairDecision::content_ours(ours.to_string());
    if let Some(current) = current.as_deref()
        && strip_boundary_for_dedup(current) != strip_boundary_for_dedup(ours)
    {
        let response_present = response_materialized_in_content(expected_response, current)
            || baseline.is_some_and(|base| response_already_in_current(base, ours, current));
        let prompt_drift = baseline.is_some_and(|base| {
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(base, current, ours)
        });
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_live_buffer_diverged file={} patch_id={} response_present={} current_len={} current_hash={} content_ours_len={} content_ours_hash={} prompt_drift={}",
                file.display(),
                patch_id,
                response_present,
                current.len(),
                crate::ops_log::content_hash(current),
                ours.len(),
                crate::ops_log::content_hash(ours),
                prompt_drift
            ),
        );
        // #6cmx/#wy0y verification marker: an explicit, greppable record that the
        // operator typed into the document while finalize was writing (the live
        // buffer diverged from our content with prompt drift). `typed_delta_bytes`
        // is the live-vs-ours byte delta (their keystrokes); `response_present`
        // confirms the assistant response is still materialized in the buffer, so
        // grepping `finalize_typing_during_write` verifies a typing-during-finalize
        // run was exercised and whether the response survived intact.
        if prompt_drift {
            crate::ops_log::log_op(
                file,
                &format!(
                    "finalize_typing_during_write file={} patch_id={} typed_delta_bytes={} response_present={} resolution=content_ours_adopted",
                    file.display(),
                    patch_id,
                    current.len() as i64 - ours.len() as i64,
                    response_present
                ),
            );
        }

        if !response_present {
            if let Some(repaired_current) =
                materialize_response_in_current_exchange(current, expected_response)
            {
                log_ipc_proof_failure(
                    file,
                    "socket_already_applied",
                    Some(patch_id),
                    "disk_missing_response_probe",
                    "content_ours_snapshot_visible_response_repair",
                    &format!(
                        "response_sha256={} current_len={} current_hash={} repaired_len={} repaired_hash={}",
                        crate::ops_log::content_hash(expected_response),
                        current.len(),
                        crate::ops_log::content_hash(current),
                        repaired_current.len(),
                        crate::ops_log::content_hash(&repaired_current)
                    ),
                );
                guard_visible_write_idle_and_current(
                    file,
                    "socket_already_applied_missing_disk_response",
                    current,
                )?;
                atomic_write_pub(file, &repaired_current)?;
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_missing_disk_response_repaired file={} patch_id={} visible_len={} visible_hash={} content_ours_len={} content_ours_hash={}",
                        file.display(),
                        patch_id,
                        repaired_current.len(),
                        crate::ops_log::content_hash(&repaired_current),
                        ours.len(),
                        crate::ops_log::content_hash(ours)
                    ),
                );
            } else {
                log_ipc_proof_failure(
                    file,
                    "socket_already_applied",
                    Some(patch_id),
                    "disk_missing_response_probe",
                    "file_ipc_fallback",
                    &format!(
                        "response_sha256={} current_len={} current_hash={}",
                        crate::ops_log::content_hash(expected_response),
                        current.len(),
                        crate::ops_log::content_hash(current)
                    ),
                );
                return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
            }
        } else {
            repair_decision = IpcRepairDecision::file_read(current.to_string());
            if let Some(lines) = normalize_prefix_lines
                && !lines.is_empty()
            {
                let normalized = normalize_exchange_prefixes_for_targets(
                    &repair_decision.snapshot_content,
                    lines,
                );
                if normalized != repair_decision.snapshot_content {
                    repair_decision = IpcRepairDecision::content_ours_prefix_fallback(
                        normalized,
                        current.to_string(),
                        lines,
                    );
                }
            }

            let before_response_dedupe = repair_decision.snapshot_content.clone();
            let response_deduped =
                dedupe_consecutive_response_blocks(&repair_decision.snapshot_content, file);
            if response_deduped != repair_decision.snapshot_content {
                repair_decision =
                    repair_decision.apply_ipc_dedupe(response_deduped, before_response_dedupe);
            }

            let pre_dedupe_snap = repair_decision.snapshot_content.clone();
            let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
                file,
                baseline,
                &repair_decision.snapshot_content,
                "socket_already_applied_disk",
            )?;
            if dedupe_repair {
                repair_decision = repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
            } else {
                repair_decision.snapshot_content = effective_snap;
            }
        }
    }

    repair_ipc_decision_visible_state(file, &repair_decision, Some(patch_id))?;
    snapshot::save(file, &repair_decision.snapshot_content)?;
    let crdt_doc = crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
    snapshot::save_document_crdt(
        file,
        &crdt_doc.encode_state(),
        &repair_decision.snapshot_content,
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_socket_already_applied_snapshot file={} patch_id={} snap_source={} snap_len={} snap_hash={}",
            file.display(),
            patch_id,
            repair_decision.snap_source.label(),
            repair_decision.snapshot_content.len(),
            crate::ops_log::content_hash(&repair_decision.snapshot_content)
        ),
    );
    Ok(AlreadyAppliedSnapshotOutcome::Persisted)
}

pub(crate) fn normalization_prefix_observation_counts(
    content: &str,
    normalize_prefix_lines: &[String],
) -> (usize, usize) {
    let target_counts = normalization_target_counts(normalize_prefix_lines);
    let required = target_counts.values().sum();
    if required == 0 {
        return (0, 0);
    }

    let exchange = component::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_else(|| content.to_string());

    let mut observed_counts = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(&exchange, Some(&target_counts)) {
        let Some(stripped) = line.trim_end().strip_prefix("❯ ") else {
            continue;
        };
        if target_counts.contains_key(stripped) {
            *observed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    let observed = target_counts
        .iter()
        .map(|(target, required)| {
            observed_counts
                .get(target)
                .copied()
                .unwrap_or(0)
                .min(*required)
        })
        .sum();
    (required, observed)
}

pub(crate) fn duplicate_prompt_line_count(content: &str) -> usize {
    let exchange = component::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_else(|| content.to_string());

    let mut counts = std::collections::HashMap::<String, usize>::new();
    let mut duplicates = 0;
    for line in exchange_prompt_prefix_eligible_lines(&exchange, None) {
        let normalized = line
            .trim_end()
            .strip_prefix("❯ ")
            .unwrap_or(line.trim_end())
            .trim();
        if normalized.is_empty() {
            continue;
        }
        let count = counts.entry(normalized.to_string()).or_default();
        *count += 1;
        if *count > 1 {
            duplicates += 1;
        }
    }
    duplicates
}

pub(crate) fn ipc_repair_decision_from_sidecar(
    file: &Path,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    snap_content: String,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
) -> IpcRepairDecision {
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
        && !verify_sidecar_normalization(&snap_content, lines)
    {
        if let Some(ours) = content_ours {
            let bad_state = snap_content;
            let fallback = normalized_content_ours_fallback(file, baseline, ours, lines);
            let (required_prefix_count, observed_prefix_count) =
                normalization_prefix_observation_counts(&bad_state, lines);
            let duplicate_prompt_count = duplicate_prompt_line_count(&bad_state);
            eprintln!(
                "[write] sidecar normalization diverged — falling back to content_ours ({} bytes)",
                fallback.len()
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback file={} patch_id={} snap_source=content_ours reason=prefix_divergence bad_len={} bad_hash={} fallback_len={} fallback_hash={} required_prefix_count={} observed_prefix_count={} duplicate_prompt_count={}",
                    file.display(),
                    patch_id.unwrap_or("-"),
                    bad_state.len(),
                    crate::ops_log::content_hash(&bad_state),
                    fallback.len(),
                    crate::ops_log::content_hash(&fallback),
                    required_prefix_count,
                    observed_prefix_count,
                    duplicate_prompt_count
                ),
            );
            return IpcRepairDecision::content_ours_prefix_fallback(fallback, bad_state, lines);
        }

        eprintln!(
            "[write] sidecar normalization diverged but no content_ours available — using sidecar"
        );
    }

    IpcRepairDecision::ack_content(snap_content)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FullContentRepairRedelivery {
    NormalizationFallback,
    IpcDedupe,
}

impl FullContentRepairRedelivery {
    fn label(self) -> &'static str {
        match self {
            Self::NormalizationFallback => "sidecar_normalization_fallback",
            Self::IpcDedupe => "ipc_dedupe",
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] sidecar normalization fallback re-delivered to editor via full-content IPC"
            }
            Self::IpcDedupe => "[write] IPC duplicate-response repair re-delivered to editor",
        }
    }

    fn not_consumed_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] WARNING: sidecar normalization fallback repaired disk, but editor IPC repair was not consumed; reload the document if the editor view is stale"
            }
            Self::IpcDedupe => {
                "[write] WARNING: IPC duplicate-response repair updated disk, but editor IPC repair was not consumed; reload the document if the editor view is stale"
            }
        }
    }

    fn failed_message(self, error: &anyhow::Error) -> String {
        match self {
            Self::NormalizationFallback => format!(
                "[write] WARNING: sidecar normalization fallback repaired disk, but editor IPC repair failed: {}; reload the document if the editor view is stale",
                error
            ),
            Self::IpcDedupe => format!(
                "[write] WARNING: IPC duplicate-response repair updated disk, but editor IPC repair failed: {}; reload the document if the editor view is stale",
                error
            ),
        }
    }
}

pub(crate) fn redeliver_full_content_repair_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    kind: FullContentRepairRedelivery,
    source_patch_id: Option<&str>,
) -> bool {
    let current_content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[write] WARNING: {} editor repair skipped because {} could not be read: {}",
                kind.label(),
                file.display(),
                e
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=read_failed error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            return false;
        }
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "{}_editor_redelivery_proof file={} patch_id={} proof_source=bad_editor_state expected_len={} expected_hash={} current_len={} current_hash={} redeliver={}",
            kind.label(),
            file.display(),
            source_patch_id.unwrap_or("-"),
            expected_bad_state.len(),
            crate::ops_log::content_hash(expected_bad_state),
            current_content.len(),
            crate::ops_log::content_hash(&current_content),
            current_content == expected_bad_state
        ),
    );
    if current_content != expected_bad_state {
        eprintln!(
            "[write] {} editor repair skipped: visible buffer no longer matches the bad state",
            kind.label()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "{}_editor_redelivery_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={}",
                kind.label(),
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                crate::ops_log::content_hash(expected_bad_state),
                current_content.len(),
                crate::ops_log::content_hash(&current_content)
            ),
        );
        return false;
    }

    match try_ipc_full_content_response_fallback_from_source(
        file,
        repaired_content,
        expected_bad_state,
    ) {
        Ok(true) => {
            eprintln!("{}", kind.success_message());
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_redelivered_editor file={} patch_id={} bytes={} expected_bad_len={} expected_bad_hash={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len(),
                    expected_bad_state.len(),
                    crate::ops_log::content_hash(expected_bad_state)
                ),
            );
            true
        }
        Ok(false) => {
            eprintln!("{}", kind.not_consumed_message());
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_repair_not_consumed file={} patch_id={} bytes={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len()
                ),
            );
            false
        }
        Err(e) => {
            eprintln!("{}", kind.failed_message(&e));
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_repair_failed file={} patch_id={} error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            false
        }
    }
}

pub(crate) fn normalization_repair_candidate_matches(
    expected_bad_state: &str,
    repaired_content: &str,
    normalize_prefix_lines: &[String],
) -> bool {
    if normalize_prefix_lines.is_empty() {
        return false;
    }
    let normalized =
        normalize_exchange_prefixes_for_targets(expected_bad_state, normalize_prefix_lines);
    strip_boundary_for_dedup(&normalized) == strip_boundary_for_dedup(repaired_content)
}

pub(crate) fn normalization_repair_payload(
    canonical: &Path,
    patch_id: &str,
    normalize_prefix_lines: &[String],
    expected_bad_state: &str,
    include_type: bool,
) -> serde_json::Value {
    let proof =
        crate::flow::document_mutation::FullContentSourceProof::from_content(expected_bad_state);
    let mut payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
        "normalize_prefix_lines": normalize_prefix_lines,
        "expected_content_hash": proof.expected_content_hash,
        "expected_content_len": proof.expected_content_len,
    });
    if include_type {
        payload["type"] = serde_json::Value::String("patch".to_string());
    }
    payload
}

pub(crate) fn verify_normalization_repair_observed(
    file: &Path,
    project_root: &Path,
    patch_id: &str,
    repaired_content: &str,
    transport: &str,
) -> bool {
    let observed = match poll_ack_content_sidecar(
        project_root,
        patch_id,
        std::time::Duration::from_millis(200),
        std::time::Duration::from_millis(25),
    ) {
        Ok(Some(content)) => content,
        Ok(None) => std::fs::read_to_string(file).unwrap_or_default(),
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_ack_read_failed file={} patch_id={} transport={} error={}",
                    file.display(),
                    patch_id,
                    transport,
                    e
                ),
            );
            std::fs::read_to_string(file).unwrap_or_default()
        }
    };

    let observed_matches =
        strip_boundary_for_dedup(&observed) == strip_boundary_for_dedup(repaired_content);
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_observed file={} patch_id={} transport={} observed_len={} observed_hash={} expected_len={} expected_hash={} matched={}",
            file.display(),
            patch_id,
            transport,
            observed.len(),
            crate::ops_log::content_hash(&observed),
            repaired_content.len(),
            crate::ops_log::content_hash(repaired_content),
            observed_matches
        ),
    );
    observed_matches
}

pub(crate) fn try_ipc_normalization_repair_patch(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
) -> Result<bool> {
    if !normalization_repair_candidate_matches(
        expected_bad_state,
        repaired_content,
        normalize_prefix_lines,
    ) {
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_ineligible file={} patch_id={} skip=normalization_only_patch_not_equivalent normalize_targets={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                normalize_prefix_lines.len()
            ),
        );
        return Ok(false);
    }

    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} before normalization repair",
            file.display()
        )
    })?;
    if current_content != expected_bad_state {
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                crate::ops_log::content_hash(expected_bad_state),
                current_content.len(),
                crate::ops_log::content_hash(&current_content)
            ),
        );
        return Ok(false);
    }

    let canonical = file.canonicalize()?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patch_id = uuid::Uuid::new_v4().to_string();
    let payload = normalization_repair_payload(
        &canonical,
        &patch_id,
        normalize_prefix_lines,
        expected_bad_state,
        true,
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_attempt file={} patch_id={} source_patch_id={} normalize_targets={} expected_bad_len={} expected_bad_hash={} repaired_len={} repaired_hash={}",
            file.display(),
            patch_id,
            source_patch_id.unwrap_or("-"),
            normalize_prefix_lines.len(),
            expected_bad_state.len(),
            crate::ops_log::content_hash(expected_bad_state),
            repaired_content.len(),
            crate::ops_log::content_hash(repaired_content)
        ),
    );

    if crate::ipc_socket::is_listener_active(&project_root) {
        match crate::ipc_socket::send_message(&project_root, &payload) {
            Ok(Some(_)) => {
                if verify_normalization_repair_observed(
                    file,
                    &project_root,
                    &patch_id,
                    repaired_content,
                    "socket",
                ) {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=socket",
                            file.display(),
                            patch_id
                        ),
                    );
                    return Ok(true);
                }
                return Ok(false);
            }
            Ok(None) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=socket",
                        file.display(),
                        patch_id
                    ),
                );
            }
            Err(e) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} transport=socket error={}",
                        file.display(),
                        patch_id,
                        e
                    ),
                );
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(false);
    }

    let hash = snapshot::doc_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    let payload = normalization_repair_payload(
        &canonical,
        &patch_id,
        normalize_prefix_lines,
        expected_bad_state,
        false,
    );
    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;

    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !patch_file.exists() {
            if verify_normalization_repair_observed(
                file,
                &project_root,
                &patch_id,
                repaired_content,
                "file",
            ) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=file",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(true);
            }
            return Ok(false);
        }
        std::thread::sleep(poll_interval);
    }
    let _ = std::fs::remove_file(&patch_file);
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=file",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub(crate) fn redeliver_normalization_fallback_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
) -> bool {
    match try_ipc_normalization_repair_patch(
        file,
        repaired_content,
        expected_bad_state,
        normalize_prefix_lines,
        source_patch_id,
    ) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} error={}",
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
        }
    }

    redeliver_full_content_repair_to_editor(
        file,
        repaired_content,
        expected_bad_state,
        FullContentRepairRedelivery::NormalizationFallback,
        source_patch_id,
    )
}

pub(crate) fn repair_disk_from_ipc_dedupe(file: &Path, content: &str) -> Result<()> {
    guard_visible_write_idle(file, "ipc_dedupe_repair")?;
    atomic_write(file, content).with_context(|| {
        format!(
            "failed to repair {} after IPC duplicate-response dedupe",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_dedupe_repaired_working_tree file={} bytes={}",
            file.display(),
            content.len()
        ),
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn redeliver_ipc_dedupe_to_editor(file: &Path, content: &str, expected_bad_state: &str) -> bool {
    redeliver_full_content_repair_to_editor(
        file,
        content,
        expected_bad_state,
        FullContentRepairRedelivery::IpcDedupe,
        None,
    )
}

pub(crate) fn repair_ipc_decision_visible_state(
    file: &Path,
    decision: &IpcRepairDecision,
    patch_id: Option<&str>,
) -> Result<()> {
    let Some(reason) = decision.disk_repair_reason else {
        return Ok(());
    };
    let bad_len = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.len)
        .unwrap_or(0);
    let bad_hash = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.hash.as_str())
        .unwrap_or("-");
    let current = std::fs::read_to_string(file).ok();
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_repair_decision file={} patch_id={} snap_source={} repair_reason={} redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={} current_len={} current_hash={} normalize_targets={} duplicate_prompt_count={}",
            file.display(),
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            reason.label(),
            decision.redeliver_editor,
            bad_len,
            bad_hash,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            current.as_deref().map(str::len).unwrap_or(0),
            current
                .as_deref()
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            decision.normalize_prefix_lines.len(),
            duplicate_prompt_line_count(
                decision
                    .editor_bad_state
                    .as_ref()
                    .map(EditorBadStateFingerprint::content)
                    .unwrap_or(&decision.snapshot_content)
            )
        ),
    );

    if decision.redeliver_editor
        && let Some(expected_bad_state) = decision.editor_bad_state.as_ref()
        && match reason {
            IpcDiskRepairReason::PrefixDivergence => redeliver_normalization_fallback_to_editor(
                file,
                &decision.snapshot_content,
                expected_bad_state.content(),
                &decision.normalize_prefix_lines,
                patch_id,
            ),
            IpcDiskRepairReason::IpcDedupe | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe => {
                redeliver_full_content_repair_to_editor(
                    file,
                    &decision.snapshot_content,
                    expected_bad_state.content(),
                    reason.redelivery_kind(),
                    patch_id,
                )
            }
        }
    {
        return Ok(());
    }

    match reason {
        IpcDiskRepairReason::PrefixDivergence => {
            repair_disk_from_normalization_fallback(file, &decision.snapshot_content)
        }
        IpcDiskRepairReason::IpcDedupe | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe => {
            repair_disk_from_ipc_dedupe(file, &decision.snapshot_content)
        }
    }
}

pub fn dedupe_ipc_snapshot_content(
    file: &Path,
    before: Option<&str>,
    content: &str,
    source: &str,
) -> Result<(String, bool)> {
    let (deduped, report) = repair_duplicate_prompt_artifacts(
        content,
        file,
        DuplicatePromptRepairOptions::new(source)
            .with_before(before)
            .preserving(before),
    )?;
    let changed = deduped != content;
    if report.changed() {
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_snapshot_deduped file={} source={} before_commit=true",
                file.display(),
                source
            ),
        );
    }
    Ok((deduped, changed))
}

/// Result of an IPC write attempt, including the patch_id used.
///
/// The `patch_id` is returned so callers (e.g., `run_stream()` timeout fallback)
/// can reuse it for deduplication — the plugin tracks applied patch_ids and skips
/// duplicates, preventing double-apply when both socket and file IPC fire.
pub struct IpcResult {
    /// Whether the plugin successfully consumed the patch.
    pub success: bool,
    /// The patch_id used for this write attempt. Reuse in fallback writes
    /// so the plugin can deduplicate.
    pub patch_id: String,
    /// True when IPC was intentionally skipped because the current cycle has
    /// already reached the terminal committed state.
    pub skipped_committed_cycle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIpcRepositionResult {
    Queued,
    DeferredExistingPatch,
    Unavailable,
}

/// Remove leftover fallback patch files for a document after closeout commits.
/// Prevents late file-watcher or plugin recovery from re-applying a stale patch
/// to an already-committed document.
pub fn cleanup_fallback_patch_files(file: &Path) {
    crate::flow::closeout::cleanup_fallback_patch_files(file);
}

/// Check if the current cycle for `file` is already in Committed phase.
/// Returns `Some(cycle_id)` if committed, `None` if no cycle or cycle is open.
pub(crate) fn cycle_already_committed(file: &Path) -> Option<String> {
    crate::flow::closeout::cycle_already_committed(file)
}

pub(crate) fn write_claimed_patch_sentinel(project_root: &Path, patch_id: &str) {
    crate::flow::closeout::write_claimed_patch_sentinel(project_root, patch_id);
}

pub(crate) fn existing_patch_is_reposition_only(payload: &serde_json::Value) -> bool {
    payload
        .get("reposition_boundary")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        && payload
            .get("patches")
            .and_then(|value| value.as_array())
            .is_none_or(|patches| patches.is_empty())
        && payload
            .get("unmatched")
            .and_then(|value| value.as_str())
            .is_none_or(|unmatched| unmatched.trim().is_empty())
        && payload
            .get("fullContent")
            .and_then(|value| value.as_str())
            .is_none_or(|content| content.is_empty())
}

pub fn queue_file_ipc_reposition_boundary(
    file: &Path,
    boundary_id: Option<&str>,
    normalize_prefix_lines: &[String],
) -> Result<FileIpcRepositionResult> {
    let canonical = file.canonicalize()?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(FileIpcRepositionResult::Unavailable);
    }

    let hash = snapshot::doc_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    if patch_file.exists() {
        let existing = std::fs::read_to_string(&patch_file).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(payload) if existing_patch_is_reposition_only(&payload) => {}
            Ok(_) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "file_ipc_reposition_deferred_existing_patch file={} patch_file={}",
                        file.display(),
                        patch_file.display()
                    ),
                );
                return Ok(FileIpcRepositionResult::DeferredExistingPatch);
            }
            Err(e) => {
                eprintln!(
                    "[commit] replacing unreadable file IPC reposition patch {}: {}",
                    patch_file.display(),
                    e
                );
            }
        }
    }

    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
    });
    if let Some(boundary_id) = boundary_id {
        payload["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
    }
    if !normalize_prefix_lines.is_empty() {
        payload["normalize_prefix_lines"] = serde_json::Value::Array(
            normalize_prefix_lines
                .iter()
                .map(|line| serde_json::Value::String(line.clone()))
                .collect(),
        );
    }

    // #late-ipc-patch-duplicate-stall: tag the queued file patch with the cycle
    // id + a baseline content hash of the doc it targets so a LATE applier (the
    // plugin's PatchWatcher, or the supervisor IPC listener) can fence a
    // superseded patch — drop a patch whose cycle already committed or whose
    // baseline no longer matches the live doc instead of blindly re-applying it
    // minutes late and re-materializing a duplicate response block. The
    // write-side guard in `try_ipc` already rejects a fresh send for an
    // already-committed cycle; this carries the same generation token on the
    // durable file patch so the asynchronous apply side can make the identical
    // decision. (Plan: tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md.)
    if let Ok(Some(cs)) = crate::cycle_state::load(file) {
        payload["cycle_id"] = serde_json::Value::String(cs.cycle_id);
    }
    if let Ok(live) = std::fs::read_to_string(file) {
        payload["baseline_hash"] = serde_json::Value::String(crate::debounce::content_hash(&live));
    }

    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "file_ipc_reposition_queued file={} patch_file={} patch_id={}",
            file.display(),
            patch_file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
    );
    eprintln!(
        "[commit] file IPC reposition patch queued: {}",
        patch_file.display()
    );
    Ok(FileIpcRepositionResult::Queued)
}

/// Attempt to write via IPC (socket-first, file-based fallback).
///
/// First tries socket IPC via `ipc_socket::send_message()` for lowest latency.
/// Falls back to file-based IPC (JSON patch in `.agent-doc/patches/`) if socket
/// is unavailable. Returns `IpcResult` with success flag and the patch_id used.
///
/// When `reuse_patch_id` is provided, that ID is used instead of generating a new
/// one. This ensures the plugin can deduplicate when the same logical write is
/// retried via the timeout fallback path.
#[allow(clippy::too_many_arguments)]
pub fn try_ipc(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    reuse_patch_id: Option<&str>,
) -> Result<IpcResult> {
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patch_id = reuse_patch_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let ipc_before_content = std::fs::read_to_string(file).ok();

    // Guard: if the cycle is already committed, reject the patch to prevent
    // a late fallback from re-dirtying the document.
    //
    // Exception (#adoc-compact-during-turn-response-loss): when a binary-owned
    // commit lands mid-turn (for example a JetBrains-initiated
    // `agent-doc compact exchange` between this turn's preflight and finalize),
    // the cycle state's `Committed` phase belongs to that other operation —
    // not to the response we are about to apply. Detect that case by checking
    // whether the response headings carried in the incoming patches are
    // already present in HEAD. If they are, the gate is correct (skip).
    // If they are not, the "committed" cycle is unrelated to this response:
    // rotate the cycle state to start fresh and let the patch flow continue.
    if let Some(ref cycle_id) = cycle_already_committed(file) {
        let response_in_head = patch_response_headings_already_in_head(file, patches);
        if !response_in_head {
            eprintln!(
                "[write] mid-turn cycle rotation detected for {}: cycle {} marked committed \
                 but the incoming response heading(s) are absent from HEAD — starting a fresh \
                 cycle instead of rejecting (see #adoc-compact-during-turn-response-loss)",
                file.display(),
                cycle_id
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "mid_turn_cycle_rotation file={} prior_cycle={} patch_id={} action=fresh_cycle",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            let snapshot_content = crate::snapshot::load(file)?;
            let file_content_for_state = std::fs::read_to_string(file).ok();
            let _ = crate::cycle_state::start_preflight(
                file,
                snapshot_content.as_deref(),
                file_content_for_state.as_deref(),
            );
        } else {
            eprintln!(
                "[write] rejecting late fallback patch: cycle {} already committed for {}",
                cycle_id,
                file.display()
            );
            log_closeout_guard(
                file,
                crate::flow::types::FlowStage::TerminalGuard,
                crate::flow::types::FlowOutcome::Blocked,
                crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "late_fallback_patch_rejected file={} cycle_id={} patch_id={} reason=already_committed",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            cleanup_fallback_patch_files(file);
            return Ok(IpcResult {
                success: false,
                patch_id,
                skipped_committed_cycle: true,
            });
        }
    }

    // Clean up any legacy degraded marker from older versions
    cleanup_legacy_ipc_degraded(&project_root);

    // `#ipc-degraded-prefers-file-ipc`: when the socket listener is latched
    // degraded, do NOT jump straight to a raw disk write. Skip only the (wedged)
    // socket attempt and let the write fall through to the file-IPC patch queue
    // below — the plugin's file watcher still applies it via the Document API,
    // so a degraded session never manufactures an IDEA "File Cache Conflict".
    // The disk write becomes the true last resort, reached by the caller only
    // when file IPC also fails to deliver (`success: false`).
    let socket_degraded = ipc_direct_disk_degraded(&project_root, file)?;
    if socket_degraded {
        eprintln!(
            "[write] IPC socket degraded for {} — preferring file-IPC patch queue (disk write is last resort)",
            file.display()
        );
        log_ipc_dewedge_prefer_file_ipc(file, "try_ipc");
    }

    // Try socket IPC first (lower latency, no inotify) unless the socket is
    // latched degraded — in that case the file-IPC patch queue below is the
    // reliable plugin path.
    if !socket_degraded && crate::ipc_socket::is_listener_active(&project_root) {
        // Seed the boundary from patch_id so the socket patch and any later file /
        // run_stream fallback rebuild share an IDENTICAL boundary — otherwise a
        // late socket apply + file apply land the response twice
        // (#finalize-visible-buffer-ipc-timeout-race).
        let ipc_patches_json = build_ipc_patches_json(
            file,
            patches,
            unmatched,
            normalize_prefix_lines,
            Some(&patch_id),
        )?;
        let ipc_node_patches_json =
            build_ipc_node_patches_json(baseline.or(ipc_before_content.as_deref()), content_ours);
        // When unmatched content was synthesized into a patch (no explicit patch blocks),
        // don't also send it as "unmatched" — the plugin would apply both and duplicate.
        let effective_unmatched_socket = if patches.is_empty() && !ipc_patches_json.is_empty() {
            eprintln!(
                "[write] synthesis consumed unmatched content — clearing from socket payload (prevent double-apply)"
            );
            ""
        } else {
            unmatched.trim()
        };
        let mut socket_payload = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": ipc_patches_json,
            "node_patches": ipc_node_patches_json,
            "unmatched": effective_unmatched_socket,
            "baseline": baseline.unwrap_or(""),
            "reposition_boundary": true,
        });
        socket_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
        if let Ok(Some(ref cs)) = crate::cycle_state::load(file) {
            socket_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
        }
        if let Some(yaml) = frontmatter_yaml {
            socket_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
        }
        if let Some(lines) = normalize_prefix_lines
            && !lines.is_empty()
        {
            socket_payload["normalize_prefix_lines"] = serde_json::Value::Array(
                lines
                    .iter()
                    .map(|l| serde_json::Value::String(l.clone()))
                    .collect(),
            );
            if ipc_patches_json.is_empty()
                && let Some(ours) = content_ours
                && full_content_ipc_scope_allows(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                )
            {
                log_full_content_ipc_disabled(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                );
            }
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_attempt file={} hash={} patch_id={} patches={} ipc_patches={} unmatched_len={} effective_unmatched_len={} baseline_len={} normalize_targets={} unmatched_marker_count={}",
                file.display(),
                hash,
                patch_id,
                patches.len(),
                ipc_patches_json.len(),
                unmatched.trim().len(),
                effective_unmatched_socket.len(),
                baseline.map(str::len).unwrap_or(0),
                normalize_prefix_lines.map(|lines| lines.len()).unwrap_or(0),
                patchback_marker_count_outside_code(unmatched)
            ),
        );
        // Pre-write fallback patch file before socket send. If socket delivery
        // succeeds but sidecar ack times out, the file watcher can recover the
        // response from this file. patch_id dedup prevents double-apply when
        // both socket and file watcher fire. Overwrites any stale content.
        let fallback_patch_file = {
            let patches_dir = project_root.join(".agent-doc/patches");
            if patches_dir.exists() {
                let path = patches_dir.join(format!("{}.json", hash));
                match serde_json::to_string_pretty(&socket_payload) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&path, &json) {
                            eprintln!(
                                "[write] WARNING: failed to write fallback patch file: {}",
                                e
                            );
                            None
                        } else {
                            eprintln!("[write] fallback patch file pre-written for recovery");
                            Some(path)
                        }
                    }
                    Err(e) => {
                        eprintln!("[write] WARNING: failed to serialize fallback patch: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        };
        match crate::ipc_socket::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC patch delivered");
                clear_ipc_socket_ack_timeouts(&project_root, file, "socket_ack")?;
                // Poll for ack-content sidecar (written by plugin after apply).
                let sidecar = poll_ack_content_sidecar(
                    &project_root,
                    &patch_id,
                    std::time::Duration::from_millis(200),
                    std::time::Duration::from_millis(25),
                )?;
                if let Some(snap_content) = sidecar {
                    let mut repair_decision = ipc_repair_decision_from_sidecar(
                        file,
                        Some(&patch_id),
                        baseline,
                        snap_content,
                        content_ours,
                        normalize_prefix_lines,
                    );

                    let pre_dedupe_snap = repair_decision.snapshot_content.clone();
                    let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
                        file,
                        ipc_before_content.as_deref(),
                        &repair_decision.snapshot_content,
                        repair_decision.snap_source.label(),
                    )?;
                    if dedupe_repair {
                        repair_decision =
                            repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
                    } else {
                        repair_decision.snapshot_content = effective_snap;
                    }
                    // Capture the live editor buffer before the guards replace it,
                    // so the #ipcfullprompt forensic detector sees the candidate.
                    let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
                    let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &mut repair_decision,
                    );
                    let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        content_ours,
                        &mut repair_decision,
                    );
                    log_ipc_snapshot_adoption_allowed(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &repair_decision,
                        drift_fired || dup_fired,
                    );
                    log_ipcfullprompt_corruption_if_any(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        &ipcfullprompt_candidate,
                    );

                    let expected_response = response_materialization_probe(patches, unmatched);
                    if !ipc_response_materialized_or_fallback(
                        file,
                        "socket_ack_content",
                        &expected_response,
                        &repair_decision.snapshot_content,
                    ) {
                        repair_partial_response_materialization_before_fallback(
                            file,
                            "socket_ack_content",
                            &expected_response,
                        )?;
                        return Ok(IpcResult {
                            success: false,
                            patch_id,
                            skipped_committed_cycle: false,
                        });
                    }

                    eprintln!(
                        "[write] snapshot from {} ({} bytes)",
                        repair_decision.snap_source.label(),
                        repair_decision.snapshot_content.len()
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_ack_content file={} patch_id={} snap_source={} sidecar_len={} sidecar_hash={} disk_len={} disk_hash={}",
                            file.display(),
                            patch_id,
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len(),
                            crate::ops_log::content_hash(&repair_decision.snapshot_content),
                            ipc_before_content.as_deref().map(str::len).unwrap_or(0),
                            ipc_before_content
                                .as_deref()
                                .map(crate::ops_log::content_hash)
                                .unwrap_or_else(|| "-".to_string())
                        ),
                    );
                    if let Some(ref path) = fallback_patch_file {
                        let _ = std::fs::remove_file(path);
                    }
                    repair_ipc_decision_visible_state(file, &repair_decision, Some(&patch_id))?;
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_delivered file={} snap_source={} snap_len={}",
                            file.display(),
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len()
                        ),
                    );
                    if let Some(before) = ipc_before_content.as_deref() {
                        log_exchange_write_diagnostic(
                            file,
                            "try_ipc_socket",
                            "socket_ipc",
                            Some(&patch_id),
                            baseline,
                            before,
                            &repair_decision.snapshot_content,
                            patches,
                            unmatched,
                        );
                    }
                    if let Err(e) = snapshot::save(file, &repair_decision.snapshot_content) {
                        eprintln!(
                            "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                             Commit will auto-recover via divergence detection.",
                            e
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "snapshot_save_failed_after_ipc file={} error={}",
                                file.display(),
                                e
                            ),
                        );
                    } else {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "snapshot_saved_socket_ipc file={} snap_len={}",
                                file.display(),
                                repair_decision.snapshot_content.len()
                            ),
                        );
                        let crdt_doc =
                            crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
                        if let Err(e) = snapshot::save_document_crdt(
                            file,
                            &crdt_doc.encode_state(),
                            &repair_decision.snapshot_content,
                        ) {
                            eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                        }
                    }
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                // `#ipc-degrade-false-vote`: the socket already returned a
                // delivery ack (`Ok(Some(_ack))`), so the plugin received the
                // patch and is applying it through the Document API — only the
                // *content sidecar* was slow. The plugin writes that sidecar
                // after `saveDocument`, which can lag well past the poll budget
                // while the EDT is busy or the user is typing. A slow sidecar is
                // NOT a listener timeout: it must not vote toward the de-wedge
                // degrade threshold and must not latch this session to disk-only.
                // Recover the snapshot through the file-IPC patch queue below
                // (still the plugin path) so a confirmed-but-slow delivery never
                // manufactures a raw foreign disk write — the source of IDEA
                // "File Cache Conflict". Genuine transport failures still vote in
                // the `Err(timeout)` arm.
                eprintln!(
                    "[write] socket delivered but content sidecar was slow — recovering snapshot via file-IPC fallback (no degrade vote)"
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_sidecar_slow_no_degrade file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                if fallback_patch_file.is_some() {
                    eprintln!("[write] fallback patch file left for file watcher recovery");
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_sidecar_timeout file={} — falling back to disk write",
                        file.display()
                    ),
                );
                log_ipc_proof_failure(
                    file,
                    "socket_ipc",
                    Some(&patch_id),
                    "no_ack_content_sidecar",
                    "direct_write_fallback",
                    "ack_content_timeout=true",
                );
                if let Some(ref cycle_id) = cycle_already_committed(file) {
                    eprintln!(
                        "[write] socket IPC fallback: cycle {} already committed — skipping file IPC",
                        cycle_id
                    );
                    log_closeout_guard(
                        file,
                        crate::flow::types::FlowStage::TerminalGuard,
                        crate::flow::types::FlowOutcome::Blocked,
                        crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_sidecar_timeout_skip_file_fallback file={} cycle_id={} reason=already_committed",
                            file.display(),
                            cycle_id
                        ),
                    );
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: false,
                        patch_id,
                        skipped_committed_cycle: true,
                    });
                }
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no ack — falling back to file IPC");
            }
            Err(e) if crate::ipc_socket::is_already_applied_error(&e) => {
                // The plugin detected the response body is already present
                // in the live buffer and chose not to re-apply it. Re-writing
                // through the file-IPC fallback would create a duplicate
                // response. Treat as success and skip the fallback.
                // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
                // Phase 2.
                eprintln!(
                    "[write] socket IPC reported already_applied: {} — skipping file IPC fallback (response already in live buffer)",
                    e
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_skip_file_fallback file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                let expected_response = response_materialization_probe(patches, unmatched);
                if persist_already_applied_socket_content_ours_snapshot(
                    file,
                    &patch_id,
                    baseline,
                    content_ours,
                    normalize_prefix_lines,
                    &expected_response,
                )? == AlreadyAppliedSnapshotOutcome::Persisted
                {
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                eprintln!(
                    "[write] socket already_applied could not prove the response on disk — falling back to file IPC"
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_fallback_to_file_ipc file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
            }
            Err(e) => {
                eprintln!(
                    "[write] socket IPC failed: {} — falling back to file IPC",
                    e
                );
                if is_socket_ack_timeout_error(&e) {
                    let degraded = record_ipc_socket_ack_timeout(
                        &project_root,
                        file,
                        Some(&patch_id),
                        "socket_ipc",
                    )?;
                    if degraded {
                        // `#ipc-degraded-prefers-file-ipc`: the socket just
                        // latched degraded, but the plugin's file watcher is a
                        // separate transport that is very likely still alive.
                        // Fall through to the file-IPC patch queue below instead
                        // of skipping straight to a raw disk write — the plugin
                        // applies the queued patch via the Document API, so this
                        // degraded write never manufactures a File Cache Conflict.
                        // Disk write stays the true last resort (file-IPC timeout).
                        eprintln!(
                            "[write] IPC socket degraded for {} after repeated socket ack timeouts — falling back to file-IPC patch queue (disk write is last resort)",
                            file.display()
                        );
                        log_ipc_dewedge_prefer_file_ipc(file, "socket_ipc_timeout");
                    }
                }
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

    if let Some(ref cycle_id) = cycle_already_committed(file) {
        eprintln!(
            "[write] file IPC fallback: cycle {} already committed — skipping patch write",
            cycle_id
        );
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "file_ipc_fallback_skip file={} cycle_id={} reason=already_committed",
                file.display(),
                cycle_id
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: true,
        });
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Build patches using shared helper (same logic as socket path). Seed the
    // boundary from patch_id so a later file/fallback rebuild reuses the same
    // boundary (#finalize-visible-buffer-ipc-timeout-race).
    let ipc_patches = build_ipc_patches_json(
        file,
        patches,
        unmatched,
        normalize_prefix_lines,
        Some(&patch_id),
    )?;
    let ipc_node_patches =
        build_ipc_node_patches_json(baseline.or(ipc_before_content.as_deref()), content_ours);

    // Same dedup guard as socket path: don't send unmatched when it was synthesized into a patch.
    let effective_unmatched_file = if patches.is_empty() && !ipc_patches.is_empty() {
        ""
    } else {
        unmatched.trim()
    };

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "node_patches": ipc_node_patches,
        "unmatched": effective_unmatched_file,
        "baseline": baseline.unwrap_or(""),
        "reposition_boundary": true,
    });
    ipc_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
    if let Ok(Some(ref cs)) = crate::cycle_state::load(file) {
        ipc_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
    }

    if let Some(yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
    }
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
    {
        ipc_payload["normalize_prefix_lines"] = serde_json::Value::Array(
            lines
                .iter()
                .map(|l| serde_json::Value::String(l.clone()))
                .collect(),
        );
        if ipc_patches.is_empty()
            && let Some(ours) = content_ours
            && full_content_ipc_scope_allows(
                file,
                FullContentIpcMode::ResponseFallback,
                &patch_id,
                ours,
                ipc_before_content.as_deref(),
                ipc_before_content.as_deref(),
            )
        {
            log_full_content_ipc_disabled(
                file,
                FullContentIpcMode::ResponseFallback,
                &patch_id,
                ours,
                ipc_before_content.as_deref(),
                ipc_before_content.as_deref(),
            );
        }
    }

    // Log IPC write details for debugging cross-contamination
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_write_attempt file={} hash={} patches={} ipc_patches={} unmatched_len={}",
            file.display(),
            hash,
            patches.len(),
            ipc_patches.len(),
            unmatched.trim().len()
        ),
    );

    // Warn when unmatched content exists but no IPC patches were synthesized —
    // this means content will be silently dropped by the plugin
    if ipc_patches.is_empty() && !unmatched.trim().is_empty() {
        eprintln!(
            "[write] WARNING: {} bytes of unmatched content with no IPC patches — content will be dropped. \
             Does the target file have template components (<!-- agent:exchange -->)?",
            unmatched.trim().len()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_unmatched_content_dropped file={} unmatched_len={}",
                file.display(),
                unmatched.trim().len()
            ),
        );
    }

    // Defense-in-depth dedupe gate for the file-IPC fallback when delivering
    // a response patch. When the plugin already applied the response via a
    // prior socket retry whose ack-write was slow, applying the same response
    // patch through file IPC would land a duplicate `### Re:` heading on top
    // of the live buffer.
    //
    // The socket-IPC path catches this via `ipc_socket::is_already_applied_error`
    // when the plugin sends `{"type":"ack","status":"error","reason":"already_applied"}`.
    // Until every plugin emits that ack (`#ipcpluginalready`), the file-IPC
    // fallback hash-compares response-patch outcomes against the current file:
    // if applying the response patches to the current file is a structural
    // no-op (boundary markers excluded), skip the write so the duplicate
    // cannot land.
    //
    // Scope: only response-bearing patches (contain at least one `### Re:`
    // heading). Pure prompt/component patches fall through to the existing
    // path, which has its own no-ack guard for unacknowledged live-edit IPC.
    //
    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 2 (remaining) / `[#ipcfilehashskip]`.
    if !patches.is_empty()
        && patches
            .iter()
            .any(|patch| patch.content.contains("### Re:"))
        && let Ok(current) = std::fs::read_to_string(file)
        && let Ok(after_apply) = crate::template::apply_patches(&current, patches, "", file)
        && strip_boundary_for_dedup(&after_apply) == strip_boundary_for_dedup(&current)
    {
        eprintln!(
            "[write] file IPC fallback: patches already present in live buffer — skipping file IPC write (defense-in-depth dedupe)"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "file_ipc_fallback_skip_already_applied file={} patch_id={} patches={}",
                file.display(),
                patch_id,
                patches.len()
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: true,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

    let success = write_ipc_and_poll(
        &patch_file,
        &ipc_payload,
        file,
        ipc_patches.len(),
        IpcPollOptions {
            content_ours,
            normalize_prefix_lines,
            project_root: &project_root,
            guard_committed_cycle: true,
        },
    )?;
    Ok(IpcResult {
        success,
        patch_id,
        skipped_committed_cycle: false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullContentIpcMode {
    /// Late fallback repair for an agent response. Must not dirty an already
    /// committed cycle.
    ResponseFallback,
    /// Operator-owned replacement such as Compact Exchange. This is a new
    /// document mutation even when the previous response cycle is committed.
    OperatorMutation,
}

/// Disabled full-document editor IPC path.
///
/// This function intentionally never emits socket or file IPC payloads. It
/// keeps the terminal committed-cycle cleanup guard and diagnostic logging so
/// callers can fall back to the guarded disk/snapshot path without handing the
/// editor a whole-document replacement.
#[allow(dead_code)]
pub fn try_ipc_full_content(file: &Path, content: &str) -> Result<bool> {
    try_ipc_full_content_with_mode(file, content, FullContentIpcMode::ResponseFallback, None)
}

pub(crate) fn try_ipc_full_content_response_fallback_from_source(
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        file,
        content,
        FullContentIpcMode::ResponseFallback,
        Some(source_content),
    )
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation(file: &Path, content: &str) -> Result<bool> {
    try_ipc_full_content_with_mode(file, content, FullContentIpcMode::OperatorMutation, None)
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation_from_source(
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        file,
        content,
        FullContentIpcMode::OperatorMutation,
        Some(source_content),
    )
}

pub(crate) fn full_content_source_label(mode: FullContentIpcMode) -> &'static str {
    match mode {
        FullContentIpcMode::ResponseFallback => "response_fallback",
        FullContentIpcMode::OperatorMutation => "compact_exchange",
    }
}

pub(crate) fn log_full_content_ipc_disabled(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) {
    let source = full_content_source_label(mode);
    eprintln!(
        "[write] full-content IPC disabled for {}: falling back to guarded disk path",
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "full_content_ipc_disabled file={} source={} patch_id={} reason=disabled_by_default target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            target_content.len(),
            crate::ops_log::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
}

pub(crate) fn frontmatter_mode_is_explicit_template(mode: &str) -> bool {
    matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "template" | "stream"
    )
}

pub(crate) fn content_declares_template_frontmatter(content: &str) -> bool {
    frontmatter::parse(content).ok().is_some_and(|(fm, _)| {
        fm.format == Some(frontmatter::AgentDocFormat::Template)
            || fm
                .mode
                .as_deref()
                .is_some_and(frontmatter_mode_is_explicit_template)
    })
}

pub(crate) fn content_has_agent_components(content: &str) -> bool {
    component::parse(content)
        .ok()
        .is_some_and(|components| !components.is_empty())
}

pub(crate) fn full_content_ipc_scope_rejection_reason(contents: &[Option<&str>]) -> Option<&'static str> {
    for content in contents.iter().flatten() {
        if content_declares_template_frontmatter(content) {
            return Some("template_frontmatter");
        }
        if content_has_agent_components(content) {
            return Some("agent_component_markers");
        }
    }
    None
}

pub(crate) fn full_content_ipc_scope_allows(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) -> bool {
    let reason = full_content_ipc_scope_rejection_reason(&[
        Some(target_content),
        source_content,
        current_content,
    ]);
    let Some(reason) = reason else {
        return true;
    };

    let source = full_content_source_label(mode);
    eprintln!(
        "[write] full-content IPC skipped for {}: {} is not eligible for whole-document editor replacement",
        file.display(),
        reason
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "full_content_ipc_scope_rejected file={} source={} patch_id={} scope={} target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            reason,
            target_content.len(),
            crate::ops_log::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    false
}

pub(crate) fn try_ipc_full_content_with_mode(
    file: &Path,
    content: &str,
    mode: FullContentIpcMode,
    source_content: Option<&str>,
) -> Result<bool> {
    let _canonical = file.canonicalize()?;
    let before_content = std::fs::read_to_string(file).ok();
    let effective_source_content = match (mode, source_content) {
        (FullContentIpcMode::ResponseFallback, None) => Some(content),
        _ => source_content,
    };
    let patch_id = uuid::Uuid::new_v4().to_string();

    if mode == FullContentIpcMode::ResponseFallback
        && let Some(ref cycle_id) = cycle_already_committed(file)
    {
        eprintln!(
            "[write] full-content IPC skipped: cycle {} already committed for {}",
            cycle_id,
            file.display()
        );
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "late_fallback_patch_rejected file={} cycle_id={} patch_id=full_content reason=already_committed transport=full_content",
                file.display(),
                cycle_id
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(false);
    }

    if !full_content_ipc_scope_allows(
        file,
        mode,
        &patch_id,
        content,
        effective_source_content,
        before_content.as_deref(),
    ) {
        return Ok(false);
    }

    log_full_content_ipc_disabled(
        file,
        mode,
        &patch_id,
        content,
        effective_source_content,
        before_content.as_deref(),
    );
    Ok(false)
}

pub(crate) struct IpcPollOptions<'a> {
    content_ours: Option<&'a str>,
    normalize_prefix_lines: Option<&'a [String]>,
    project_root: &'a Path,
    guard_committed_cycle: bool,
}

/// Send a reposition-only IPC signal to the plugin.
///
/// No content changes — just tells the plugin to move the boundary marker
/// to the end of the exchange component. Used by `commit()` to keep the
/// boundary at end-of-exchange without writing to the working tree
/// (which would cause keystroke loss if the user is typing).
///
/// Returns `true` if the plugin consumed the signal, `false` on timeout
/// or if no plugin is active.
pub fn try_ipc_reposition_boundary(file: &Path) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = resolve_ipc_project_root(&canonical);
    cleanup_legacy_ipc_degraded(&project_root);
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            eprintln!(
                "[commit] IPC reposition skipped for {}: listener degraded for this session",
                file.display()
            );
            log_ipc_dewedge_direct_disk_skip(file, "reposition");
            return false;
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[commit] IPC reposition degradation check failed (non-fatal): {}",
                e
            );
        }
    }
    let snapshot_doc = crate::snapshot::load(file).ok().flatten();
    let working_doc = std::fs::read_to_string(file).ok();
    let boundary_id = snapshot_doc
        .as_deref()
        .and_then(|doc| find_boundary_id(doc, "exchange"))
        .or_else(|| {
            working_doc
                .as_deref()
                .and_then(|doc| find_boundary_id(doc, "exchange"))
        });
    let normalize_prefix_lines = match (snapshot_doc.as_deref(), working_doc.as_deref()) {
        (Some(committed), Some(working)) => {
            extract_post_commit_normalization_targets(committed, working)
        }
        _ => vec![],
    };

    if !crate::ipc_socket::is_listener_active(&project_root) {
        return match queue_file_ipc_reposition_boundary(
            file,
            boundary_id.as_deref(),
            &normalize_prefix_lines,
        ) {
            Ok(FileIpcRepositionResult::Queued) => true,
            Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
            Ok(FileIpcRepositionResult::Unavailable) => false,
            Err(e) => {
                eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                false
            }
        };
    }

    let result = if normalize_prefix_lines.is_empty() {
        crate::ipc_socket::send_reposition(
            &project_root,
            &canonical.to_string_lossy(),
            boundary_id.as_deref(),
            true, // preserve (HEAD) in editor buffer
        )
    } else {
        let mut message = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": [],
            "unmatched": "",
            "reposition_boundary": true,
            "preserve_head": true,
            "normalize_prefix_lines": normalize_prefix_lines.clone(),
        });
        if let Some(boundary_id) = boundary_id.as_deref() {
            message["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
        }
        crate::ipc_socket::send_message(&project_root, &message).map(|_| true)
    };

    match result {
        Ok(true) => {
            if let Err(e) =
                clear_ipc_socket_ack_timeouts(&project_root, file, "reposition_socket_ack")
            {
                eprintln!(
                    "[commit] IPC reposition timeout clear failed (non-fatal): {}",
                    e
                );
            }
            if normalize_prefix_lines.is_empty() {
                eprintln!("[commit] IPC reposition boundary signal sent");
            } else {
                eprintln!(
                    "[commit] IPC prefix repair + boundary signal sent ({} lines)",
                    normalize_prefix_lines.len()
                );
            }
            true
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no ack (non-fatal)");
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            if is_socket_ack_timeout_error(&e) {
                match record_ipc_socket_ack_timeout(&project_root, file, None, "reposition") {
                    Ok(true) => {
                        eprintln!(
                            "[commit] IPC listener degraded for {} after repeated reposition ack timeouts",
                            file.display()
                        );
                        log_ipc_dewedge_direct_disk_skip(file, "reposition_timeout");
                        cleanup_fallback_patch_files(file);
                        return false;
                    }
                    Ok(false) => {}
                    Err(record_err) => eprintln!(
                        "[commit] IPC reposition timeout record failed (non-fatal): {}",
                        record_err
                    ),
                }
            }
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
        }
    }
}

/// Write an IPC patch file and poll for plugin ACK (file deletion).
///
/// Returns `Ok(true)` if consumed, `Ok(false)` on timeout.
pub(crate) fn write_ipc_and_poll(
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    options: IpcPollOptions<'_>,
) -> Result<bool> {
    let before_content = std::fs::read_to_string(doc_file).ok();
    let patch_id_for_diagnostics = payload.get("patch_id").and_then(|value| value.as_str());
    // Atomic write of patch file
    atomic_write(patch_file, &serde_json::to_string_pretty(payload)?)?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patch_count
    );

    // Poll for ACK (plugin deletes file after applying)
    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if options.guard_committed_cycle
            && let Some(ref cycle_id) = cycle_already_committed(doc_file)
        {
            eprintln!(
                "[write] IPC poll skipped: cycle {} already committed for {}",
                cycle_id,
                doc_file.display()
            );
            log_closeout_guard(
                doc_file,
                crate::flow::types::FlowStage::TerminalGuard,
                crate::flow::types::FlowOutcome::Blocked,
                crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
            );
            crate::ops_log::log_op(
                doc_file,
                &format!(
                    "file_ipc_poll_skip file={} cycle_id={} reason=already_committed",
                    doc_file.display(),
                    cycle_id
                ),
            );
            cleanup_fallback_patch_files(doc_file);
            return Ok(false);
        }
        if !patch_file.exists() {
            // Plugin consumed the patch — poll for ack-content sidecar (authoritative
            // post-apply snapshot). Falls back to file read after timeout.
            let patch_id = payload
                .get("patch_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (current_on_disk, mut repair_decision, ack_content_proven) = if !patch_id.is_empty()
            {
                match poll_ack_content_sidecar(
                    options.project_root,
                    patch_id,
                    std::time::Duration::from_millis(500),
                    std::time::Duration::from_millis(25),
                ) {
                    Ok(Some(content)) => {
                        let baseline = payload
                            .get("baseline")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty());
                        let decision = ipc_repair_decision_from_sidecar(
                            doc_file,
                            Some(patch_id),
                            baseline,
                            content,
                            options.content_ours,
                            options.normalize_prefix_lines,
                        );
                        if decision.snap_source == IpcSnapshotSource::AckContentSidecar {
                            eprintln!(
                                "[write] snapshot from ack-content sidecar ({} bytes)",
                                decision.snapshot_content.len()
                            );
                        }
                        let ack_content_proven = decision.ack_content_proven();
                        let snapshot_content = decision.snapshot_content.clone();
                        (snapshot_content, decision, ack_content_proven)
                    }
                    _ => {
                        eprintln!(
                            "[write] snapshot from file read (ack-content sidecar not available after 500ms)"
                        );
                        let content = std::fs::read_to_string(doc_file).unwrap_or_default();
                        let decision = IpcRepairDecision::file_read(content.clone());
                        (content, decision, false)
                    }
                }
            } else {
                eprintln!("[write] snapshot from file read (no patch_id for sidecar lookup)");
                let content = std::fs::read_to_string(doc_file).unwrap_or_default();
                let decision = IpcRepairDecision::file_read(content.clone());
                (content, decision, false)
            };
            let baseline_content = payload
                .get("baseline")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !baseline_content.is_empty() && current_on_disk == baseline_content {
                // File on disk hasn't changed — plugin likely failed to apply the patch.
                // Don't save snapshot with content that was never applied.
                eprintln!(
                    "[write] IPC patch consumed but file unchanged on disk — plugin may have failed to apply. Falling back to disk write."
                );
                return Ok(false);
            }

            if let Some(full_content) = payload
                .get("fullContent")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                && current_on_disk != full_content
            {
                eprintln!(
                    "[write] IPC full-content patch consumed but final content does not match payload — falling back to disk write."
                );
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "full_content_ipc_post_apply_mismatch file={} expected_len={} actual_len={}",
                        doc_file.display(),
                        full_content.len(),
                        current_on_disk.len()
                    ),
                );
                return Ok(false);
            }

            // Verify patch content is present in the file (catches partial application).
            // Check that at least one non-empty patch's content appears in the result.
            let patch_list = payload.get("patches").and_then(|v| v.as_array());
            if let Some(patches) = patch_list {
                let has_content_patch = patches.iter().any(|p| {
                    let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    !content.trim().is_empty()
                });
                if has_content_patch {
                    let any_present = patches.iter().any(|p| {
                        let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        if content.trim().is_empty() {
                            return true;
                        }
                        // Check first meaningful line of content appears in file
                        content
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .is_none_or(|first_line| current_on_disk.contains(first_line.trim()))
                    });
                    if !any_present {
                        eprintln!(
                            "[write] IPC patch consumed but response content not found in file — plugin may have partially failed. Falling back to disk write."
                        );
                        return Ok(false);
                    }
                }
            }
            let expected_response = response_materialization_probe_from_ipc_payload(payload);
            if !ipc_response_materialized_or_fallback(
                doc_file,
                "file_ipc",
                &expected_response,
                &current_on_disk,
            ) {
                repair_partial_response_materialization_before_fallback(
                    doc_file,
                    "file_ipc",
                    &expected_response,
                )?;
                return Ok(false);
            }
            if file_ipc_consumed_without_live_exchange_ack(
                doc_file,
                "file_ipc",
                Some(patch_id),
                payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty()),
                before_content.as_deref(),
                &current_on_disk,
                ack_content_proven,
            ) {
                return Ok(false);
            }

            // Plugin applied the patch — update snapshot as actual post-write disk state.
            // `current_on_disk` is from ack-content sidecar when available, or 200ms file read.
            // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
            let pre_dedupe_content = repair_decision.snapshot_content.clone();
            let (snap_content, dedupe_repair) = dedupe_ipc_snapshot_content(
                doc_file,
                before_content.as_deref(),
                &repair_decision.snapshot_content,
                repair_decision.snap_source.label(),
            )?;
            if dedupe_repair {
                repair_decision =
                    repair_decision.apply_ipc_dedupe(snap_content, pre_dedupe_content);
            } else {
                repair_decision.snapshot_content = snap_content;
            }
            if file_ipc_consumed_without_live_exchange_ack(
                doc_file,
                "file_ipc",
                Some(patch_id),
                payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty()),
                before_content.as_deref(),
                &repair_decision.snapshot_content,
                ack_content_proven,
            ) {
                return Ok(false);
            }
            let file_baseline = payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());
            // Capture the live editor buffer before the guards replace it, so the
            // #ipcfullprompt forensic detector sees the candidate.
            let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
            let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                options.content_ours,
                &mut repair_decision,
            );
            let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
                doc_file,
                "file_ipc",
                Some(patch_id),
                options.content_ours,
                &mut repair_decision,
            );
            log_ipc_snapshot_adoption_allowed(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                options.content_ours,
                &repair_decision,
                drift_fired || dup_fired,
            );
            log_ipcfullprompt_corruption_if_any(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                &ipcfullprompt_candidate,
            );
            repair_ipc_decision_visible_state(doc_file, &repair_decision, Some(patch_id))?;
            crate::ops_log::log_op(
                doc_file,
                &format!(
                    "ipc_file_delivered file={} snap_len={}",
                    doc_file.display(),
                    repair_decision.snapshot_content.len()
                ),
            );
            if let Some(before) = before_content.as_deref() {
                let patch_id = payload.get("patch_id").and_then(|value| value.as_str());
                let baseline = payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty());
                let payload_patches: Vec<template::PatchBlock> = payload
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                let name = item
                                    .get("component")
                                    .or_else(|| item.get("name"))
                                    .and_then(|value| value.as_str())?;
                                let content =
                                    item.get("content").and_then(|value| value.as_str())?;
                                Some(template::PatchBlock::new(name, content))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let unmatched = payload
                    .get("unmatched")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                log_exchange_write_diagnostic(
                    doc_file,
                    "write_ipc_and_poll",
                    "file_ipc",
                    patch_id,
                    baseline,
                    before,
                    &repair_decision.snapshot_content,
                    &payload_patches,
                    unmatched,
                );
            }
            if let Err(e) = snapshot::save(doc_file, &repair_decision.snapshot_content) {
                eprintln!(
                    "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                     Commit will auto-recover via divergence detection.",
                    e
                );
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "snapshot_save_failed_after_ipc file={} error={}",
                        doc_file.display(),
                        e
                    ),
                );
            } else {
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "snapshot_saved_file_ipc file={} snap_len={}",
                        doc_file.display(),
                        repair_decision.snapshot_content.len()
                    ),
                );
                let crdt_doc = crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
                if let Err(e) = snapshot::save_document_crdt(
                    doc_file,
                    &crdt_doc.encode_state(),
                    &repair_decision.snapshot_content,
                ) {
                    eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                }
                eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            }
            return Ok(true);
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — clean up unconsumed patch file
    eprintln!(
        "[write] IPC timeout ({}s) — falling back to direct write",
        timeout.as_secs()
    );
    log_ipc_proof_failure(
        doc_file,
        "file_ipc",
        patch_id_for_diagnostics,
        "no_ack",
        "direct_write_fallback",
        &format!(
            "timeout_secs={} patch_file={}",
            timeout.as_secs(),
            patch_file.display()
        ),
    );
    let _ = std::fs::remove_file(patch_file);
    Ok(false)
}

/// Apply `❯ ` prefix to lines in `content` that appear in `normalize_prefix_lines`.
///
/// Bakes normalization into patch content before IPC delivery so the plugin
/// receives already-prefixed lines. The plugin runs normalization *before*
/// applying patches, so it cannot normalize lines the patch is about to append.
pub(crate) fn normalize_patch_content(content: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return content.to_string();
    }
    let mut remaining = normalization_target_counts(prefix_lines);
    let mut result = String::with_capacity(content.len() + 2 * prefix_lines.len());
    for line in content.lines() {
        let bare = line
            .trim_end()
            .strip_prefix("\u{276f} ")
            .unwrap_or(line.trim_end());
        if crate::diff::line_looks_like_plain_response_after_prompt(bare) {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if !line.starts_with("\u{276f} ")
            && let Some(remaining_count) = remaining.get_mut(bare)
            && *remaining_count > 0
        {
            result.push_str("\u{276f} ");
            *remaining_count -= 1;
        }
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - 1);
    }
    result
}

pub(crate) fn normalization_target_counts(
    prefix_lines: &[String],
) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for line in prefix_lines {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_default() += 1;
    }
    counts
}

/// Build the IPC patches JSON array (shared between socket and file-based paths).
///
/// Reads the document to find boundary IDs, filters frontmatter patches,
/// synthesizes exchange patches for unmatched content.
///
/// When `normalize_prefix_lines` is provided, applies `❯ ` prefix to matching
/// lines inside each patch's content so newly-appended lines already carry the
/// prefix. (The plugin runs normalization *before* applying patches, so it
/// cannot normalize lines that the patch is about to append.)
pub(crate) fn build_ipc_patches_json(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    normalize_prefix_lines: Option<&[String]>,
    boundary_seed: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
    let summary = file.file_stem().and_then(|s| s.to_str());
    // #finalize-visible-buffer-ipc-timeout-race: when a stable seed (the IPC
    // patch_id) is supplied, derive a deterministic boundary so this write's
    // socket / file / fallback rebuilds all carry the SAME boundary. Without it,
    // each rebuild minted a fresh random boundary and the plugin appended the
    // response a second time, doubling the editor buffer.
    let current_doc = match boundary_seed {
        Some(seed) => {
            let bid = agent_doc_core::id::boundary_id_from_seed_with_summary(seed, summary);
            template::reposition_boundary_to_end_clean_with_summary_and_id(
                &raw_doc,
                Some(&bid),
                summary,
            )
        }
        None => template::reposition_boundary_to_end_clean_with_summary(&raw_doc, summary),
    };

    let mut ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter(|p| p.name != "frontmatter")
        .map(|p| {
            let content = match normalize_prefix_lines {
                Some(prefix_lines)
                    if !prefix_lines.is_empty() && is_append_mode_component(&p.name) =>
                {
                    normalize_patch_content(&p.content, prefix_lines)
                }
                _ => p.content.clone(),
            };
            let mut patch_json = serde_json::json!({
                "component": p.name,
                "content": content,
                "op": if is_append_mode_component(&p.name) {
                    "append"
                } else {
                    "replace"
                },
            });
            if let Some(bid) = find_boundary_id(&current_doc, &p.name) {
                patch_json["boundary_id"] = serde_json::Value::String(bid.clone());
                patch_json["node_id"] = serde_json::Value::String(bid);
            } else if is_append_mode_component(&p.name) {
                patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
            }
            patch_json
        })
        .collect();

    let effective_unmatched = unmatched.trim().to_string();
    if ipc_patches.is_empty() && !effective_unmatched.is_empty() {
        // Dedup guard: parse components once, check before synthesizing.
        let parsed_comps = crate::component::parse(&current_doc).unwrap_or_default();
        for target in &["exchange", "output"] {
            // Skip synthesis if the content already exists in the target component.
            // This makes the write idempotent even when called twice with the same content.
            let already_present = parsed_comps.iter().any(|c| {
                c.name == *target && {
                    let body = &current_doc[c.open_end..c.close_start];
                    body.contains(effective_unmatched.as_str())
                }
            });
            if already_present {
                eprintln!(
                    "[write] dedup: content already present in {} — skipping synthesis",
                    target
                );
                break;
            }
            if let Some(bid) = find_boundary_id(&current_doc, target) {
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines)
                        if !prefix_lines.is_empty() && is_append_mode_component(target) =>
                    {
                        normalize_patch_content(&effective_unmatched, prefix_lines)
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (boundary {})",
                    target,
                    &bid[..8.min(bid.len())]
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
                    "op": "append",
                    "boundary_id": bid,
                    "node_id": bid,
                }));
                break;
            } else if is_append_mode_component(target) {
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines) if !prefix_lines.is_empty() => {
                        normalize_patch_content(&effective_unmatched, prefix_lines)
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (ensure_boundary)",
                    target
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
                    "op": "append",
                    "ensure_boundary": true,
                }));
                break;
            }
        }
    }

    Ok(ipc_patches)
}

pub(crate) fn build_ipc_node_patches_json(before: Option<&str>, after: Option<&str>) -> Vec<Value> {
    let (Some(before), Some(after)) = (before, after) else {
        return Vec::new();
    };
    if before == after {
        return Vec::new();
    }

    let mut component_names = BTreeSet::new();
    for component in agent_doc_markdown_ast::overlay::components(before)
        .into_iter()
        .chain(agent_doc_markdown_ast::overlay::components(after))
    {
        if !component.items.is_empty() {
            component_names.insert(component.name);
        }
    }

    let mut node_patches = Vec::new();
    for component in component_names {
        let before_nodes =
            agent_doc_markdown_ast::mutations::item_nodes(before, &component).unwrap_or_default();
        let after_nodes =
            agent_doc_markdown_ast::mutations::item_nodes(after, &component).unwrap_or_default();
        if before_nodes.is_empty() && after_nodes.is_empty() {
            continue;
        }

        let before_by_key: HashMap<&str, _> = before_nodes
            .iter()
            .map(|node| (node.node_key.as_str(), node))
            .collect();
        let after_by_key: HashMap<&str, _> = after_nodes
            .iter()
            .map(|node| (node.node_key.as_str(), node))
            .collect();

        for node in &before_nodes {
            if !after_by_key.contains_key(node.node_key.as_str()) {
                node_patches.push(serde_json::json!({
                    "component": component.as_str(),
                    "node_key": node.node_key.as_str(),
                    "op": "remove",
                    "content": ipc_node_source(before, node),
                }));
            }
        }

        for (index, node) in after_nodes.iter().enumerate() {
            if before_by_key.contains_key(node.node_key.as_str()) {
                continue;
            }
            let mut patch = serde_json::json!({
                "component": component.as_str(),
                "node_key": node.node_key.as_str(),
                "op": "insert",
                "content": ipc_node_source(after, node),
            });
            if let Some(anchor) = previous_existing_node_key(&after_nodes[..index], &before_by_key)
            {
                patch["after"] = Value::String(anchor);
            } else if let Some(anchor) =
                next_existing_node_key(&after_nodes[index + 1..], &before_by_key)
            {
                patch["before"] = Value::String(anchor);
            }
            node_patches.push(patch);
        }

        for node in &before_nodes {
            let Some(after_node) = after_by_key.get(node.node_key.as_str()) else {
                continue;
            };
            let before_source = ipc_node_source(before, node);
            let after_source = ipc_node_source(after, after_node);
            if before_source == after_source {
                continue;
            }
            let op = if !node.item.struck && after_node.item.struck {
                "strike"
            } else if node.item.struck && !after_node.item.struck {
                "unstrike"
            } else {
                "replace"
            };
            node_patches.push(serde_json::json!({
                "component": component.as_str(),
                "node_key": node.node_key.as_str(),
                "op": op,
                "content": after_source,
            }));
        }

        let before_shared = before_nodes
            .iter()
            .filter(|node| after_by_key.contains_key(node.node_key.as_str()))
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        let after_shared = after_nodes
            .iter()
            .filter(|node| before_by_key.contains_key(node.node_key.as_str()))
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        if before_shared != after_shared {
            for (index, node_key) in after_shared.iter().enumerate() {
                if before_shared.get(index).copied() == Some(*node_key) {
                    continue;
                }
                let mut patch = serde_json::json!({
                    "component": component.as_str(),
                    "node_key": *node_key,
                    "op": "move",
                });
                if let Some(anchor) = after_shared[..index].last() {
                    patch["after"] = Value::String((*anchor).to_string());
                } else if let Some(anchor) = after_shared.get(index + 1) {
                    patch["before"] = Value::String((*anchor).to_string());
                }
                node_patches.push(patch);
            }
        }
    }

    node_patches
}

pub(crate) fn ipc_node_source(
    source: &str,
    node: &agent_doc_markdown_ast::mutations::MutationItemNode,
) -> String {
    source
        .get(node.item.start_byte..node.item.end_byte)
        .unwrap_or(&node.item.raw)
        .to_string()
}

pub(crate) fn previous_existing_node_key(
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    existing: &HashMap<&str, &agent_doc_markdown_ast::mutations::MutationItemNode>,
) -> Option<String> {
    nodes
        .iter()
        .rev()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone())
}

pub(crate) fn next_existing_node_key(
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    existing: &HashMap<&str, &agent_doc_markdown_ast::mutations::MutationItemNode>,
) -> Option<String> {
    nodes
        .iter()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone())
}


#[cfg(test)]
mod ack_content_snapshot_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ack_content_sidecar_read() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "test-patch-abc123";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        let sidecar = ack_dir.join(format!("{patch_id}.md"));
        std::fs::write(&sidecar, "applied content from plugin").unwrap();

        let result = read_ack_content_sidecar(&project_root, patch_id).unwrap();
        assert_eq!(result, Some("applied content from plugin".to_string()));
        assert!(!sidecar.exists(), "sidecar should be deleted after read");
    }

    #[test]
    fn test_poll_sidecar_present_immediately() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-immediate";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        std::fs::write(ack_dir.join(format!("{patch_id}.md")), "immediate content").unwrap();

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("immediate content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_appears_after_delay() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-delayed";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();

        // Spawn a thread that writes the sidecar after 50ms using atomic
        // rename to avoid the poll reading a partially-written file.
        let sidecar_path = ack_dir.join(format!("{patch_id}.md"));
        let tmp_path = ack_dir.join(format!("{patch_id}.md.tmp"));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::write(&tmp_path, "delayed content").unwrap();
            std::fs::rename(&tmp_path, &sidecar_path).unwrap();
        });

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("delayed content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_timeout() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-timeout";

        // Don't create the sidecar — poll should timeout
        std::fs::create_dir_all(project_root.join(".agent-doc/ack-content")).unwrap();

        let start = std::time::Instant::now();
        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(25),
        )
        .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "should wait at least the timeout"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(300),
            "should not wait much longer than timeout"
        );
    }

    #[test]
    fn normalization_fallback_uses_content_ours_when_sidecar_missing_prefix() {
        // When the sidecar is missing a ❯ prefix expected by normalize_prefix_lines,
        // try_ipc must fall back to content_ours for the snapshot (#jbpfx2).
        // Simulates the IntelliJ exact-match failure: plugin wrote sidecar without
        // the ❯ prefix, so content_ours (binary's authoritative state) is used.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\n<!-- agent:boundary:test-bnd-001 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");

        // content_ours has the ❯ prefix — binary's authoritative state
        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n❯ do #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
        let normalize_prefix_lines = vec!["do #jbpfx2".to_string()];

        // Simulate plugin: reads patch_id, writes sidecar WITHOUT prefix (bug), ACKs
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            // Write sidecar WITHOUT ❯ prefix (plugin failure)
                            let bad_sidecar = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        // Snapshot must use content_ours (has ❯ prefix), NOT the sidecar
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #jbpfx2"),
            "snapshot must use content_ours with ❯ prefix; got: {}",
            snap
        );
    }

    #[test]
    fn normalization_fallback_repairs_bare_content_ours_prompt_prefix() {
        // Regression for #bppfxstrip: if sidecar verification rejects the plugin
        // snapshot, the content_ours fallback must still apply normalize_prefix_lines
        // before saving the snapshot.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines =
            vec!["do #bppfxstrip. spec-test-build-install-commit-push".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "content_ours fallback must be normalized before snapshot save; got: {}",
            snap
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "content_ours fallback must repair the working tree before commit; got: {}",
            disk
        );
    }

    #[test]
    fn normfallback_records_repaired_working_tree_when_sidecar_strips_prompt_prefix() {
        // Regression for #normfallback: the observed ops-log signal should be
        // backed by deterministic coverage. A plugin sidecar that drops a
        // required prompt prefix must be rejected, and the binary fallback must
        // repair the live file before any commit can capture the stripped form.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("agent-doc-bugs2.md");
        let original = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: #normfallback — gpt-5\n\nCovered.",
        );
        let content_ours = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec!["do [#normfallback]".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do [#normfallback]"),
            "snapshot must use the normalized fallback rather than the stripped sidecar: {snap}"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do [#normfallback]"),
            "working tree must be repaired to match the normalized fallback: {disk}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback")
                && ops_log.contains("reason=prefix_divergence"),
            "ops log should record why the primary sidecar snapshot was rejected:\n{ops_log}"
        );
        assert!(
            ops_log.contains("sidecar_normalization_fallback_repaired_working_tree"),
            "ops log should record the explicit working-tree repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivers_narrow_patch_before_full_content() {
        // A disk-only fallback can leave an editor buffer stale. If the rejected
        // editor state differs only by prompt-prefix normalization, the repair
        // should converge the editor with a narrow normalization patch.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-diverge. spec-test-build-install-commit-push".to_string()];

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let listener_repair_payloads = seen_repair_payloads.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(full_content) = v.get("fullContent").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(&listener_doc, full_content);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                let patches_empty = v
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .is_none_or(|patches| patches.is_empty());
                if patches_empty
                    && let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                        value.as_array().map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                    })
                {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    let _ = std::fs::write(&listener_doc, repaired);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                Some(serde_json::json!({"type": "ack"}).to_string())
            });
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let result = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch"),
        );
        assert!(result, "narrow normalization repair should be delivered");

        assert!(
            call_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "fallback should send a narrow IPC repair"
        );
        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one narrow repair payload"
        );
        assert!(
            repair_payloads[0].get("fullContent").is_none(),
            "eligible prefix repair should avoid fullContent payloads: {}",
            repair_payloads[0]
        );
        assert_eq!(
            repair_payloads[0]["normalize_prefix_lines"][0],
            "do #sidecar-diverge. spec-test-build-install-commit-push"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-diverge. spec-test-build-install-commit-push"),
            "editor narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor"),
            "ops log should record the narrow editor repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_file_ipc_queues_narrow_patch_before_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-file. spec-test-build-install-commit-push".to_string()];
        let patch_hash = snapshot::doc_hash(&doc).unwrap();
        let patch_file = agent_doc_dir
            .join("patches")
            .join(format!("{patch_hash}.json"));

        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let watcher_doc = doc.clone();
        let watcher_patch_file = patch_file.clone();
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_repair_payloads = seen_repair_payloads.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                if !watcher_patch_file.exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                let payload_text = match std::fs::read_to_string(&watcher_patch_file) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                watcher_repair_payloads
                    .lock()
                    .unwrap()
                    .push(payload.clone());
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap()
                    .to_string();
                let lines = payload
                    .get("normalize_prefix_lines")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let current = std::fs::read_to_string(&watcher_doc).unwrap();
                let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                std::fs::write(&watcher_doc, &repaired).unwrap();
                std::fs::write(watcher_ack_dir.join(format!("{patch_id}.md")), repaired).unwrap();
                std::fs::remove_file(&watcher_patch_file).unwrap();
                return true;
            }
            false
        });

        let result = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch-file"),
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(result, "file IPC narrow normalization repair should apply");

        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one file IPC repair payload"
        );
        let payload = &repair_payloads[0];
        assert!(
            payload.get("fullContent").is_none(),
            "eligible file IPC prefix repair should avoid fullContent payloads: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["reposition_boundary"], true);
        assert_eq!(payload["preserve_head"], true);
        assert_eq!(
            payload["normalize_prefix_lines"][0],
            "do #sidecar-file. spec-test-build-install-commit-push"
        );
        assert_eq!(payload["expected_content_len"], bad_state.len());
        assert_eq!(
            payload["expected_content_hash"],
            crate::ops_log::content_hash(bad_state)
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-file. spec-test-build-install-commit-push"),
            "file IPC narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor")
                && ops_log.contains("transport=file"),
            "ops log should record the file IPC narrow editor repair:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_redelivered_editor"),
            "file IPC normalization-only repair should not fall back to fullContent:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivery_skips_when_bad_state_is_stale() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        let live_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
live prompt typed after sidecar fallback
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &["do #stale. spec-test-build-install-commit-push".to_string()],
            Some("source-patch"),
        );

        assert!(
            !delivered,
            "normalization fallback redelivery must skip stale bad-state proof"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repair_skipped")
                && ops_log.contains("skip=stale_bad_state")
                && ops_log.contains("sidecar_normalization_fallback_editor_redelivery_skipped"),
            "stale proof skip should be logged for narrow and full-content fallback:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_dedupes_already_applied_editor_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let editor_already_applied = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, editor_already_applied).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #duppb. spec-test-build-install-commit-push".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: #duppb — gpt-5").count(),
            1,
            "fallback full-content repair must not redeliver duplicate responses: {fallback}"
        );
        assert!(fallback.contains("❯ do #duppb. spec-test-build-install-commit-push"));
    }

    #[test]
    fn normalization_fallback_adopts_ack_content_response_delta_before_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #ackdelta
<!-- agent:boundary:base -->
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ackdelta
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:ours -->
<!-- /agent:exchange -->
";
        let disk_after_ack_content = "\
<!-- agent:exchange patch=append -->
do #ackdelta
while typing next prompt
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:current -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, disk_after_ack_content).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #ackdelta".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: ack delta — gpt-5").count(),
            1,
            "ack-content normalization fallback must not replay an already-applied response: {fallback}"
        );
        assert!(
            fallback.contains("while typing next prompt"),
            "ack-content fallback should preserve concurrent disk edits: {fallback}"
        );
        assert!(
            fallback.contains("❯ do #ackdelta"),
            "ack-content fallback should still normalize the prompt prefix: {fallback}"
        );
    }

    #[test]
    fn normalization_fallback_splices_pending_mutations_from_disk() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let on_disk_with_pending = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#keepme] Preserve pending add from disk
<!-- /agent:backlog -->
";
        std::fs::write(&doc, on_disk_with_pending).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let normalize_prefix_lines = vec!["do #splpend".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #splpend"),
            "snapshot must preserve normalized prompt prefix; got: {}",
            snap
        );
        assert!(
            snap.contains("- [ ] [#keepme] Preserve pending add from disk"),
            "snapshot must preserve pending mutations from disk during normalization fallback; got: {}",
            snap
        );
    }

    #[test]
    fn normalization_fallback_preserves_concurrent_comment_deletion() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #commentdel
agent response
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        let normalize_prefix_lines = vec!["do #commentdel".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
agent response
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #commentdel"),
            "normalization fallback must still repair the prompt prefix: {disk}"
        );
        assert!(
            !disk.contains("The tmux focus should be snappy."),
            "normalization fallback must not restore a concurrently deleted scratch comment: {disk}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("The tmux focus should be snappy."),
            "snapshot must also respect the concurrent comment deletion: {snap}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_requires_duplicate_occurrences() {
        let sidecar = "\
---
session: test
---

<!-- agent:exchange -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ follow-up
spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec![
            "do [#dup]. Are repeated presets handled?".to_string(),
            "spec-test-build-install-commit-push".to_string(),
            "follow-up".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        assert!(
            !verify_sidecar_normalization(sidecar, &normalize_prefix_lines),
            "one earlier prefixed preset line must not mask a later bare duplicate"
        );
    }

    #[test]
    fn extract_post_commit_normalization_targets_preserves_duplicate_missing_lines() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ Why follow up?
❯ spec-test-build-install-commit-push
<!-- agent:boundary:committed -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert_eq!(
            targets,
            vec!["spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_repairs_late_duplicate_occurrence() {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("spec-test-build-install-commit-push")],
        );

        assert_eq!(
            repaired
                .matches("❯ spec-test-build-install-commit-push")
                .count(),
            2,
            "repair should prefix the later bare duplicate without losing the earlier one"
        );
        assert!(
            !repaired.contains("\n❯ ❯ spec-test-build-install-commit-push"),
            "repair must not double-prefix existing matches"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_verification_lists() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

Verification:
- Passed focused tests:
  - `cargo test normalize_prefix`
- `cargo test` is still red on a pre-existing failure.
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "- Passed focused tests:".to_string(),
                "  - `cargo test normalize_prefix`".to_string(),
                "- `cargo test` is still red on a pre-existing failure.".to_string(),
                "do #verfpfx. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired.contains("Verification:\n- Passed focused tests:\n  - `cargo test normalize_prefix`\n- `cargo test` is still red on a pre-existing failure."),
            "assistant verification list must stay unprefixed:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #verfpfx. spec-test-build-install-commit-push\n"),
            "real prompt after the response boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ - Passed focused tests:")
                && !repaired.contains("\n❯   - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ - `cargo test` is still red on a pre-existing failure."),
            "assistant list items must not receive prompt prefixes:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_requires_targeted_prompt_start_after_response() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Why did this keep happening?
spec-test-build-install-commit-push
<!-- agent:boundary:previous -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "spec-test-build-install-commit-push".to_string(),
                "do #spfxnorm. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired
                .contains("\nWhy did this keep happening?\nspec-test-build-install-commit-push\n"),
            "assistant question and preset-looking prose must stay bare:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #spfxnorm. spec-test-build-install-commit-push\n"),
            "real prompt after the boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ spec-test-build-install-commit-push\n"),
            "a stale target inside assistant prose must not be enough to start repair:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_commit_label() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired =
            normalize_exchange_prefixes_for_targets(working, &[String::from("Commit / push:")]);

        assert!(
            repaired.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_later_assistant_commit_label_after_stale_target()
     {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

Commit / push:
- `old-sha`
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `new-sha`
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("Commit / push:"), String::from("- `old-sha`")],
        );

        assert!(
            repaired.contains("\nCommit / push:\n- `new-sha`\n"),
            "later assistant commit label/list must stay bare:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n- `new-sha`\n"),
            "later assistant commit label must not become a prompt:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_treats_prefixed_response_heading_as_assistant_boundary()
     {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#done]. spec-test-build-install-commit-push
❯ ### Re: #done — gpt-5

Implemented.

Verification:
- `cargo test normalize_prefix`

Commit / push:
- `abc123`
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "Implemented.".to_string(),
                "Verification:".to_string(),
                "- `cargo test normalize_prefix`".to_string(),
                "Commit / push:".to_string(),
                "- `abc123`".to_string(),
            ],
        );

        assert!(
            repaired.contains("\n❯ ### Re: #done — gpt-5\n\nImplemented.\n"),
            "prefixed response heading must still start an assistant block:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Implemented.")
                && !repaired.contains("\n❯ Verification:")
                && !repaired.contains("\n❯ - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ Commit / push:")
                && !repaired.contains("\n❯ - `abc123`"),
            "assistant response body after a prefixed heading must not be prompt-prefixed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_patch_content_skips_assistant_commit_label() {
        let patch = "\
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
";

        let normalized = normalize_patch_content(patch, &[String::from("Commit / push:")]);

        assert!(
            normalized.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{normalized}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_commit_label() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            !targets.iter().any(|target| target == "Commit / push:"),
            "assistant evidence label must not become a prefix repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_prose_before_next_heading() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

❯ The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5 (HEAD)

Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "a stale prefixed assistant sentence must not become a repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_markdown_lists() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
❯ - keep this bullet bare
❯   - keep this nested bullet bare
❯ 1. keep this ordered bullet bare
### Re: options — gpt-5
Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5 (HEAD)
Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "stale prefixed markdown list items must not become repair targets: {targets:?}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_rejects_assistant_list_prefix_substitute() {
        let sidecar = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Verification:
❯ - Passed focused tests:
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        assert!(
            !verify_sidecar_normalization(sidecar, &["- Passed focused tests:".to_string()]),
            "a prefixed assistant list item must not satisfy prompt-prefix sidecar verification"
        );
    }
}

#[cfg(test)]
mod late_fallback_patch_guard_tests {
    use super::{
        IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource, WriteFlags,
        cleanup_fallback_patch_files, cycle_already_committed, recover_dedupe_only_drift,
        recover_empty_response_for_strict_closeout, redeliver_ipc_dedupe_to_editor,
        repair_ipc_decision_visible_state, try_ipc, try_ipc_full_content,
        try_ipc_full_content_operator_mutation_from_source,
    };
    use crate::snapshot;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn doc_in_agent_doc_project(tmp: &TempDir, content: &str) -> std::path::PathBuf {
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        fs::write(&doc, content).unwrap();
        doc
    }

    struct TsiftDuplicateContentFixture {
        bad_state_before_live_typing: &'static str,
        repaired_snapshot: &'static str,
        live_buffer_after_typing: &'static str,
    }

    fn tsift_md_duplicate_content_corruption_fixture() -> TsiftDuplicateContentFixture {
        TsiftDuplicateContentFixture {
            bad_state_before_live_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-bad -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            repaired_snapshot: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-repaired -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            live_buffer_after_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "The duplicate content corrupt document bug happened on tsift.md as I was tying in a prompt. ",
                "What are #next-steps to ensure full-document IPC is not over-eager? #next-steps\n",
                "<!-- agent:boundary:tsift-live -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
        }
    }

    #[test]
    fn ipc_repair_decision_records_prefix_fallback_bad_state() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "fixed snapshot".to_string(),
            "bad editor state".to_string(),
            &["bad editor state".to_string()],
        );

        assert_eq!(decision.snapshot_content, "fixed snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(decision.redeliver_editor);
        let bad_state = decision
            .editor_bad_state
            .as_ref()
            .expect("prefix fallback should capture bad editor state");
        assert_eq!(bad_state.content(), "bad editor state");
        assert_eq!(bad_state.len, "bad editor state".len());
        assert_eq!(
            bad_state.hash,
            crate::ops_log::content_hash("bad editor state")
        );
        assert_eq!(decision.normalize_prefix_lines, vec!["bad editor state"]);
    }

    #[test]
    fn ipc_repair_decision_preserves_original_bad_state_when_dedupe_follows_prefix_fallback() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "prefix fallback with duplicate response".to_string(),
            "visible sidecar before fallback".to_string(),
            &["visible sidecar before fallback".to_string()],
        )
        .apply_ipc_dedupe(
            "deduped snapshot".to_string(),
            "prefix fallback with duplicate response".to_string(),
        );

        assert_eq!(decision.snapshot_content, "deduped snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("combined repair should keep original bad editor proof")
                .content(),
            "visible sidecar before fallback"
        );
    }

    #[test]
    fn cycle_already_committed_returns_none_when_no_state() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("nonexistent.md");
        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cycle_already_committed_returns_some_for_committed_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let result = cycle_already_committed(&doc);
        assert!(result.is_some(), "should return Some for committed cycle");
    }

    #[test]
    fn cycle_already_committed_returns_none_for_open_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cleanup_fallback_patch_files_removes_patch_and_writes_sentinel() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        let patch_content = serde_json::json!({
            "patch_id": "test-patch-123",
            "type": "patch",
        });
        fs::write(
            &patch_path,
            serde_json::to_string_pretty(&patch_content).unwrap(),
        )
        .unwrap();
        assert!(patch_path.exists());

        cleanup_fallback_patch_files(&doc);

        assert!(
            !patch_path.exists(),
            "fallback patch file should be removed"
        );
        let sentinel = tmp
            .path()
            .join(".agent-doc/claimed-patches")
            .join("test-patch-123");
        assert!(sentinel.exists(), "claimed sentinel should be written");
    }

    #[test]
    fn cleanup_fallback_patch_files_noop_when_no_patch() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        cleanup_fallback_patch_files(&doc);
    }

    #[test]
    fn try_ipc_marks_committed_cycle_skip_as_not_consumed() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "late-patch-123"}).to_string(),
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "late response");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            None,
            None,
            None,
            Some("current-patch-456"),
        )
        .unwrap();

        assert!(
            !result.success,
            "committed-cycle IPC skip must not look like a consumed write"
        );
        assert_eq!(result.patch_id, "current-patch-456");
        assert!(
            result.skipped_committed_cycle,
            "caller must be able to stop terminal fallback handling"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/late-patch-123")
                .exists(),
            "removed stale patch should be claimed so watchers cannot replay it"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=current-patch-456"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("ipc_write_consumed"),
            "terminal skip must not be logged as an IPC consume"
        );
    }

    #[test]
    fn full_content_ipc_skips_committed_cycle_before_socket_or_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "full-content-stale"}).to_string(),
        )
        .unwrap();

        let result = try_ipc_full_content(&doc, "stale full-content repair").unwrap();

        assert!(!result, "committed-cycle full-content IPC must be skipped");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "full-content IPC must not dirty an already committed cycle"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale full-content fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/full-content-stale")
                .exists(),
            "removed full-content fallback patch should be claimed"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=full_content"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("socket_full_content"),
            "full-content socket diagnostic must not be emitted after committed-cycle skip"
        );
    }

    #[test]
    fn full_content_operator_ipc_is_disabled_before_source_buffer_delivery() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted when the disk buffer already contains live drift"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must not overwrite live prompt drift"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "disabled full-content path must not hand a patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=compact_exchange"),
            "disabled full-content path should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn full_content_operator_ipc_rejects_late_post_exchange_scratch_comment() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let prompt = "The full-document IPC scratch comment was typed below exchange after target computation. #spec-test-build-install-commit-push";
        let source = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = source.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let target = source.replace(
            "### Re: previous — gpt-5\n\nDone.\n",
            "### Session Summary\n\nCompacted.\n",
        );
        fs::write(&doc, &live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, &target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted after a late post-exchange scratch edit"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must preserve the live scratch comment"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "scope/source guards must not hand a full-content patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("source=compact_exchange"),
            "component-scope rejection should be logged before source-buffer proof:\n{ops_log}"
        );
    }

    #[test]
    fn response_fallback_full_content_is_disabled_before_socket_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let fallback = "before\n";
        let live = "before\nlive prompt typed after fallback was computed\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result = try_ipc_full_content(&doc, fallback).unwrap();

        assert!(
            !result,
            "stale response fallback full-content IPC must be skipped before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale response fallback full-content payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale response fallback must not overwrite live prompt drift"
        );
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_full_content_redelivery_is_disabled() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, bad_state).unwrap();

        let seen_payload = Arc::new(Mutex::new(None::<serde_json::Value>));
        let listener_seen = seen_payload.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                *listener_seen.lock().unwrap() = Some(payload.clone());
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(!delivered, "full-content redelivery is disabled");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            bad_state,
            "disabled full-content redelivery must not mutate the editor-visible file"
        );
        assert!(
            seen_payload.lock().unwrap().is_none(),
            "listener should not receive a disabled full-content payload"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled redelivery should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_redelivery_skips_when_bad_state_is_stale() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let live_state = "before\nlive prompt typed after repair planning\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(
            !delivered,
            "redelivery must be skipped when the visible bad-state proof is stale"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_state,
            "stale redelivery must not overwrite live content"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale redelivery skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn template_ipc_dedupe_repair_uses_disk_not_full_content_redelivery() {
        let tmp = TempDir::new().unwrap();
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let doc = doc_in_agent_doc_project(&tmp, bad_state);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let decision = IpcRepairDecision::file_read(bad_state.to_string())
            .apply_ipc_dedupe(repaired.to_string(), bad_state.to_string());
        repair_ipc_decision_visible_state(&doc, &decision, Some("source-patch")).unwrap();

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "component-scoped template repairs must not send socket fullContent payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            repaired,
            "template duplicate repair should fall back to guarded disk repair"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter")
                && ops_log.contains("ipc_dedupe_repaired_working_tree"),
            "template fullContent rejection and disk repair should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn tsift_md_duplicate_content_fixture_skips_stale_full_document_redelivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let fixture = tsift_md_duplicate_content_corruption_fixture();
        let doc = tmp.path().join("tasks/software/tsift.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, fixture.live_buffer_after_typing).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(
            &doc,
            fixture.repaired_snapshot,
            fixture.bad_state_before_live_typing,
        );

        assert!(
            !delivered,
            "tsift.md fixture must skip full-document redelivery when the visible buffer changed after repair planning"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "stale tsift.md repair proof must be rejected before any socket fullContent payload"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fixture.live_buffer_after_typing,
            "live tsift.md prompt text typed after repair planning must remain untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_proof")
                && ops_log.contains("redeliver=false")
                && ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale tsift.md fixture should log proof and skip diagnostics:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_is_disabled_before_payload_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt typed during compact\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "disabled full-content path should reject before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale full-content payloads"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), live);
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(ops_log.contains("full_content_ipc_disabled"));

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_disabled_path_does_not_save_snapshot() {
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let target = "after\n";
        fs::write(&doc, source).unwrap();

        let root = tmp.path().to_path_buf();
        let listener_root = root.clone();
        let ack_root = root.clone();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload.get("patch_id")?.as_str()?;
                let ack_dir = ack_root.join(".agent-doc/ack-content");
                fs::create_dir_all(&ack_dir).ok()?;
                fs::write(ack_dir.join(format!("{patch_id}.md")), "wrong\n").ok()?;
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        std::thread::sleep(Duration::from_millis(100));
        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "socket full-content IPC must be disabled before payload delivery"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "mismatched socket ack-content must not become the saved snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "socket mismatch rejection must leave disk content untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(&root));
        drop(server);
    }

    fn init_git_repo(root: &Path) {
        use std::process::Command;
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
    }

    fn git_commit_file(root: &Path, rel: &str, content: &str, msg: &str) {
        use std::process::Command;
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    fn head_count(root: &Path) -> usize {
        use std::process::Command;
        let out = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn recover_dedupe_only_drift_commits_when_file_matches_dedupe_of_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        // Simulate what `agent-doc dedupe` produced: file + snapshot both equal
        // the deduped form, HEAD still holds the duplicate.
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        assert_ne!(
            deduped, duplicated,
            "test setup: duplicated content must actually dedupe"
        );
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let head_before = head_count(root);
        let recovered =
            recover_dedupe_only_drift(&doc).expect("dedupe-only drift recovery should succeed");
        assert!(
            recovered,
            "file matching dedupe(HEAD) must be recognized as a dedupe-only drift"
        );

        // Commit landed through the binary path.
        let head_after = head_count(root);
        assert_eq!(
            head_after,
            head_before + 1,
            "dedupe-only recovery must produce exactly one new commit"
        );
        let head_content = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_content.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "snapshot must hold the deduped response (boundary markers may differ from disk)"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_file_matches_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let clean = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", clean, "add clean");
        let doc = root.join("session.md");
        crate::snapshot::save(&doc, clean).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "no drift between file and HEAD should not trigger dedupe-only recovery"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_drift_is_not_a_dedupe_outcome() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let original = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", original, "add original");
        let doc = root.join("session.md");

        // Working tree differs from HEAD by an arbitrary user edit, not by
        // dedupe. Recovery must refuse so we don't auto-commit unrelated drift.
        let user_edit = original.replace("Implemented.", "Implemented and tested.");
        fs::write(&doc, &user_edit).unwrap();
        crate::snapshot::save(&doc, &user_edit).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "arbitrary working-tree drift must not be auto-committed as a dedupe recovery"
        );
    }

    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 4 + Phase 5 regression coverage. Exercises the full
    // `agent-doc dedupe` → `agent-doc write --commit` (empty stdin) recovery
    // path through the strict-closeout entry point that the four `run` /
    // `stream` / `write` call sites use.
    #[test]
    fn recover_empty_response_for_strict_closeout_lands_dedupe_only_drift_through_binary_commit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let strict = WriteFlags {
            strict_closeout: true,
            ..Default::default()
        };
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &strict)
            .expect("strict-closeout empty-stdin path should recognize dedupe-only drift");
        assert!(
            recovered,
            "empty stdin + strict closeout + dedupe-only drift must commit through the binary path"
        );
        assert_eq!(
            head_count(root),
            head_before + 1,
            "exactly one new commit should land via the dedupe recovery wrapper"
        );

        let head_after = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
    }

    #[test]
    fn recover_empty_response_for_strict_closeout_refuses_when_not_strict_closeout() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let lenient = WriteFlags::default();
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &lenient).unwrap();
        assert!(
            !recovered,
            "non-strict empty-stdin path must not silently auto-commit dedupe drift"
        );
        assert_eq!(
            head_count(root),
            head_before,
            "non-strict path should not produce a commit"
        );
    }
}

#[cfg(test)]
mod submodule_patch_routing_tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper: run a git command in `dir` with isolated user.name/email so the
    /// command works in CI environments that lack global git config. Asserts
    /// the command succeeds and prints stderr on failure.
    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git command failed to spawn");
        assert!(
            out.status.success(),
            "git {:?} failed: stderr={}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn resolve_ipc_project_root_uses_nearest_agent_doc_for_submodule_file() {
        // Build a parent+submodule layout. Verify that a document inside the
        // submodule resolves to the SUBMODULE's .agent-doc/ root, not the
        // superproject. This matches the IDE plugin's resolveRootFor logic so
        // ack-content paths agree between Rust and Kotlin.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap a "remote" submodule repo with one committed file.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule under src/submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc — the IDE plugin registers it as a root.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/patches")).unwrap();

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, submodule_root,
            "submodule file must resolve to submodule root (nearest .agent-doc/) to match IDE plugin routing"
        );

        // The superproject must NOT be returned — ack-content would diverge.
        assert_ne!(
            project_root, parent,
            "must not return the superproject — ack-content written at submodule root would not be found"
        );
    }

    #[test]
    fn resolve_ipc_project_root_ignores_agent_doc_outside_git_toplevel() {
        let outer_dir = TempDir::new().unwrap();
        let outer = outer_dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(outer.join(".agent-doc/patches")).unwrap();

        let nested = outer.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init"]);
        let doc = nested.join("session.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, nested,
            "a parent .agent-doc outside the current git toplevel must not capture IPC routing"
        );
    }

    #[test]
    fn required_closeout_fails_when_parent_submodule_pointer_commit_fails() {
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );
        git(&parent, &["commit", "-m", "add submodule"]);

        let submodule_root = parent.join("src/submodule");
        git(
            &submodule_root,
            &["config", "user.email", "test@example.com"],
        );
        git(&submodule_root, &["config", "user.name", "Test"]);
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/state/cycles")).unwrap();

        let doc = submodule_root.join("session.md");
        let initial = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(&submodule_root, &["add", "session.md"]);
        git(&submodule_root, &["commit", "-m", "add doc"]);
        git(&parent, &["add", "src/submodule"]);
        git(&parent, &["commit", "-m", "record doc commit"]);

        let parent_git_dir = Command::new("git")
            .current_dir(&parent)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(parent_git_dir.status.success());
        let parent_git_dir = PathBuf::from(String::from_utf8_lossy(&parent_git_dir.stdout).trim());
        std::fs::write(parent_git_dir.join("index.lock"), "held by test").unwrap();

        let updated = initial.replace(
            "<!-- /agent:exchange -->\n",
            "### Re: reply — gpt-5\nbody\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, &updated).unwrap();
        crate::snapshot::save(&doc, &updated).unwrap();

        let err = super::complete_required_closeout(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("parent submodule pointer is not committed"),
            "strict closeout should name the missing parent layer, got: {message}"
        );
        assert!(
            message.contains("agent-doc commit"),
            "strict closeout should prescribe the idempotent commit recovery, got: {message}"
        );
        assert!(
            crate::git::submodule_pointer_drift(&doc).unwrap().is_some(),
            "parent gitlink should remain stale when parent commit fails"
        );
    }

    // Note: a "not in git repo" fallback test is intentionally omitted because
    // /tmp tempdirs are typically nested inside the developer's checkout (the
    // agent-doc workspace itself is a git repo), so `git rev-parse
    // --show-toplevel` from `/tmp/...` walks up into the source tree. The
    // fallback path is exercised in production by non-git workspaces.

    /// Helper: start a fake socket listener that ACKs every message.
    /// Returns a handle that keeps the listener alive until dropped.
    fn start_fake_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                // Write ack-content sidecar so poll_ack_content_sidecar succeeds
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let content = if !file_path.is_empty() {
                    let file = Path::new(file_path);
                    let before = std::fs::read_to_string(file).unwrap_or_default();
                    let patches = v
                        .get("patches")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| {
                                    let name = item
                                        .get("component")
                                        .or_else(|| item.get("name"))
                                        .and_then(|value| value.as_str())?;
                                    let content =
                                        item.get("content").and_then(|value| value.as_str())?;
                                    Some(crate::template::PatchBlock::new(name, content))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let unmatched = v
                        .get("unmatched")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let after = crate::template::apply_patches(&before, &patches, unmatched, file)
                        .unwrap_or(before);
                    let _ = std::fs::write(file, &after);
                    after
                } else {
                    String::new()
                };
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    fn start_already_applied_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&root, |_msg| {
                Some(
                    serde_json::json!({
                        "type": "ack",
                        "status": "error",
                        "reason": "already_applied"
                    })
                    .to_string(),
                )
            });
        })
    }

    fn start_fixed_ack_content_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                if let Some(file_path) = v.get("file").and_then(|f| f.as_str()) {
                    let _ = std::fs::write(file_path, &ack_content);
                }
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &ack_content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    /// Helper: wait for the socket listener to become connectable (up to 1s).
    fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }

    #[test]
    fn try_ipc_routes_to_submodule_root_not_superproject() {
        // Verify that try_ipc routes patches to the SUBMODULE's own .agent-doc/
        // root, not the superproject. The submodule has its own .agent-doc/ so
        // the IDE plugin's resolveRootFor and Rust's find_project_root both
        // return the submodule root, keeping ack-content paths in sync.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap "remote" submodule repo with one commit.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc/ — mirrors the real boost-client layout.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener on the SUBMODULE root (not the parent).
        let _listener = start_fake_listener(&submodule_root);
        wait_for_listener(&submodule_root);

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "test response");

        // try_ipc should route to the submodule's socket listener and succeed.
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the submodule root"
        );

        // Verify the parent did NOT get the patch file.
        let parent_patches = parent.join(".agent-doc/patches");
        assert!(
            !parent_patches.exists(),
            "parent should NOT receive patch files — submodule routes to its own .agent-doc/"
        );
    }

    #[test]
    fn try_ipc_routes_to_git_toplevel_for_non_submodule() {
        // Verify that try_ipc routes patches to the git toplevel (not a
        // superproject) when the document lives in a plain git repo. This
        // exercises the git_toplevel_at path (step 2 in resolve_ipc_project_root).
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();

        // Initialize a plain git repo (not a submodule of anything).
        git(&root, &["init"]);
        std::fs::write(root.join("README.md"), "root").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "init"]);

        // Create .agent-doc structure.
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener.
        let _listener = start_fake_listener(&root);
        wait_for_listener(&root);

        // Create a document in a subdirectory.
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "response");

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the git toplevel"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_adopts_disk_when_response_is_present() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let live_already_applied_with_user_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "User typed the next prompt while finalize was running.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, live_already_applied_with_user_edit).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("already-applied-patch"),
        )
        .unwrap();

        assert!(
            result.success,
            "already_applied socket ack is a consumed editor write"
        );
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(live_already_applied_with_user_edit),
            "already_applied must adopt disk content when it contains the response plus live user edits"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_already_applied_with_user_edit,
            "live editor content should remain the committed snapshot candidate"
        );
        assert!(
            !crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "safe disk adoption must not leave a later snapshot-absorb block"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_skip_file_fallback")
                && log.contains("ipc_socket_already_applied_live_buffer_diverged")
                && log.contains("ipc_socket_already_applied_snapshot")
                && log.contains("snap_source=file_read"),
            "already_applied disk adoption should be auditable:\n{log}"
        );
        // #6cmx/#wy0y: this scenario IS typing-during-finalize (live buffer has a
        // user edit beyond our content), so it must emit the explicit verification
        // marker with the response intact — one greppable line proving completion.
        assert!(
            log.contains("prompt_drift=true"),
            "user-edit divergence is a prompt-drift case:\n{log}"
        );
        assert!(
            log.contains("finalize_typing_during_write") && log.contains("response_present=true"),
            "typing-during-finalize must log finalize_typing_during_write with response_present:\n{log}"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_dedupes_duplicate_response_before_snapshot() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_live_buffer = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, duplicated_live_buffer).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("already-applied-duplicate"),
        )
        .unwrap();

        assert!(result.success);
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied snapshot must dedupe duplicate response headings: {snap}"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied disk repair must converge with deduped snapshot: {disk}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_dedupe_repaired_working_tree")
                && log.contains("ipc_socket_already_applied_snapshot"),
            "dedupe repair should be logged:\n{log}"
        );
    }

    #[test]
    fn already_applied_socket_missing_disk_response_repairs_visible_without_file_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let stale_disk_with_live_prompt = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ Follow-up typed while closeout saved\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired_visible = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ Follow-up typed while closeout saved\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        fs::write(&doc, stale_disk_with_live_prompt).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &doc,
            "already-applied-missing",
            Some(baseline),
            Some(content_ours),
            None,
            "### Re: Please reply — gpt-5\n\nAnswered.\n",
        )
        .unwrap();

        assert_eq!(outcome, AlreadyAppliedSnapshotOutcome::Persisted);
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "missing disk response must keep the committed snapshot at agent-owned content_ours"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            repaired_visible,
            "visible repair must add the response without deleting the live follow-up prompt"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_missing_disk_response_repaired")
                && log.contains("recovery=content_ours_snapshot_visible_response_repair")
                && !log.contains("ipc_socket_already_applied_fallback_to_file_ipc"),
            "missing-response already_applied must not reapply through file IPC:\n{log}"
        );
    }

    #[test]
    fn socket_ack_content_prompt_duplication_uses_content_ours_and_repairs_visible_buffer() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let agent_doc_dir = root.join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();

        let doc = root.join("doc.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:before -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:ours -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_ack_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:bad -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let _listener = start_fixed_ack_content_listener(&root, duplicated_ack_content.to_string());
        wait_for_listener(&root);

        let patch =
            crate::template::PatchBlock::new("exchange", "### Re: Production key — gpt-5\n\nDone.");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("duplicated-ack-content"),
        )
        .unwrap();

        assert!(
            result.success,
            "IPC delivery should remain successful while snapshot adoption falls back"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "duplicated ack-content must not become the committed snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content_ours,
            "visible duplicated ack-content should be repaired from the guarded response image"
        );
        assert!(
            crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "later commit stages must not absorb the rejected duplicate sidecar"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=prompt_duplication_in_ack_content")
                && log.contains("duplicate_prompt_count=1")
                && log.contains("ipc_dedupe_repaired_working_tree"),
            "duplicate sidecar rejection and visible repair should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=prompt_duplication_in_ack_content")
                && log.contains("recovery=content_ours_snapshot_and_visible_repair"),
            "duplicate prompt ACK should name its failed invariant and recovery:\n{log}"
        );
    }

    #[test]
    fn cleanup_legacy_ipc_degraded_removes_marker() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let marker = root.join(".agent-doc/ipc-degraded");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists());
        cleanup_legacy_ipc_degraded(root);
        assert!(!marker.exists(), "legacy marker should be removed");
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_removes_new_prompt_target_only() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );
        let final_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "### Re: backlog prompt — gpt-5\n\n",
            "Committed and pushed.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, final_content)
                .unwrap()
                .expect("prompt target should be cleaned");

        assert!(cleaned.contains("### Re: backlog prompt — gpt-5"));
        assert!(cleaned.contains("- [x] [#keep1] Keep this tracked item"));
        assert!(!cleaned.contains("commit + push uncommitted files"));
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_preserves_non_prompt_backlog_edits() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "- [ ] [#new1] Added tracked item\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, current).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary tracked backlog additions are not prompt cleanup targets"
        );
    }

    #[test]
    fn response_already_in_current_detects_plugin_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin applied the response AND user added an edit
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User added this line.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "should detect plugin-applied response"
        );
    }

    #[test]
    fn response_already_in_current_rejects_partial_line_overlap() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "a shared response body line is not proof that the response delta was applied"
        );
    }

    #[test]
    fn response_already_in_current_accepts_normalized_delta_with_bare_prompt() {
        let base = "\
<!-- agent:exchange patch=append -->
do #ipcd
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ipcd
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
do #ipcd
while typing next prompt
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "the response hunk should be detected even when prompt-prefix normalization differs"
        );
    }

    #[test]
    fn response_already_in_current_false_when_not_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin did NOT apply — only user edits present
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User typed something new.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should not detect when plugin hasn't applied"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_exchange() {
        let base = "No components here.";
        let content_ours = "No components here either.";
        let content_current = "Still no components.";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should return false when no exchange components"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_changes() {
        let base = "\
<!-- agent:exchange patch=append -->
Same content.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, base, base),
            "should return false when ours equals base"
        );
    }

    #[test]
    fn adopt_current_response_without_duplication_rejects_partial_line_overlap() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: timeout fallback — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let adopted = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            None,
            "### Re: timeout fallback — gpt-5\nDone.\n",
        )
        .unwrap();

        assert!(
            adopted.is_none(),
            "socket-timeout fallback must not adopt current content from a partial line overlap"
        );
    }

    #[test]
    fn adopt_current_response_without_duplication_repairs_bare_prompt_prefix() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
❯ do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let repaired = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            Some(snapshot),
            "### Re: #scpd retry — gpt-5\n\nImplemented.\n",
        )
        .unwrap()
        .expect("response should be adopted from current");

        assert!(repaired.contains("❯ do #scpd. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #scpd. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #scpd retry — gpt-5").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_bare_prompt_prefix_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
### Re: #dupfx — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        assert!(repaired.contains("❯ do #dupfx. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #dupfx. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #dupfx — gpt-5").count(), 1);
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leaked_marker() {
        // CRDT merge corruption: first non-empty line of the response body
        // got a leading `❯ `. The repair must strip it without touching real
        // user prompts elsewhere in the exchange.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — opus-4-7

❯ Landed Phase 1 only this cycle. Item stays open.

#### Details

`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked ❯ on response body first line must be stripped");
        assert!(
            repaired.contains("\nLanded Phase 1 only this cycle. Item stays open.\n"),
            "stripped response body should start with the original prose, got:\n{repaired}"
        );
        assert!(
            !repaired.contains("❯ Landed"),
            "leaked ❯ must be removed, got:\n{repaired}"
        );
        // User prompt before the response heading is preserved.
        assert!(repaired.contains("❯ do #respfx. spec-test-build-install-commit-push"));
        // Heading and subsequent body lines are untouched.
        assert!(repaired.contains("### Re: #respfx — opus-4-7"));
        assert!(repaired.contains("#### Details"));
        assert!(repaired.contains("`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`."));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leading_run() {
        // Repair adoption can see every response paragraph prefixed when the
        // stale snapshot already had the response heading but not the body.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #leading-run. spec-test-build-install-commit-push
### Re: #leading-run — gpt-5

❯ First response paragraph.

❯ Second response paragraph.
❯ - Proof line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leading response-body prompt markers must be stripped");

        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(!repaired.contains("❯ First response paragraph."));
        assert!(!repaired.contains("❯ Second response paragraph."));
        assert!(!repaired.contains("❯ - Proof line."));
        assert!(repaired.contains("❯ do #leading-run. spec-test-build-install-commit-push"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_skips_when_clean() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #clean. spec-test-build-install-commit-push
### Re: #clean — opus-4-7

Landed cleanly.
<!-- /agent:exchange -->
";
        let result = strip_prompt_prefix_from_response_body_first_lines(content);
        assert!(
            result.is_none(),
            "clean document must not trigger the strip path"
        );
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_preserves_inner_prompt_like_lines() {
        // A `❯ ` appearing AFTER the first body line — e.g. quoted user input
        // inside the response prose — must be preserved. Only the leaked
        // first-line marker is stripped.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #inner. spec-test-build-install-commit-push
### Re: #inner — opus-4-7

❯ first line gets stripped

The user said:
❯ this quoted line stays
because it is not the first body line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked first-line ❯ must be stripped");
        assert!(repaired.contains("\nfirst line gets stripped\n"));
        assert!(!repaired.contains("❯ first line gets stripped"));
        // Inner `❯ ` is preserved — it is part of the response body text.
        assert!(repaired.contains("❯ this quoted line stays"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_handles_multiple_re_blocks() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #a
### Re: #a — opus-4-7

❯ first response

❯ do #b
### Re: #b — opus-4-7

❯ second response
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("multiple leaks must be stripped");
        assert!(repaired.contains("\nfirst response\n"));
        assert!(repaired.contains("\nsecond response\n"));
        assert!(!repaired.contains("❯ first response"));
        assert!(!repaired.contains("❯ second response"));
        // User prompts between blocks preserved.
        assert!(repaired.contains("❯ do #a"));
        assert!(repaired.contains("❯ do #b"));
    }

    #[test]
    fn normalize_final_template_content_removes_adjacent_duplicate_response_blocks() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let duplicated = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
<!-- /agent:exchange -->
";

        let repaired = normalize_final_template_content(
            &doc,
            baseline,
            Some(baseline),
            None,
            duplicated,
            None,
        )
        .expect("duplicate response repair should succeed");

        assert_eq!(
            repaired.matches("### Re: #duppb — gpt-5").count(),
            1,
            "closeout normalization must remove adjacent duplicate response blocks: {repaired}"
        );
        assert!(repaired.contains("Verification:\n- `cargo test`"));
    }

    #[test]
    fn normalize_final_template_content_scrubs_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let merged = base
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );

        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains("❯ The duplicate content corrupting document"),
            "live prompt should remain in exchange and be normalized:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "duplicate post-exchange prompt text should be scrubbed from comments:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"),
            "backlog scaffold should remain intact:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_baseline_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let merged = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: sqlitedb graph performance next steps — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired =
            normalize_final_template_content(&doc, &base, Some(&base), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "baseline-owned post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_current_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The html comment below this document's agent:exchange close tag had content that I put into it. This should not happen. #spec-test-build-install-commit-push";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let before_current = base.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let merged = before_current.replace(
            "<!-- /agent:exchange -->",
            "### Re: scratch comment preservation — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired = normalize_final_template_content(
            &doc,
            &base,
            Some(&base),
            Some(&before_current),
            &merged,
            None,
        )
        .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "current visible post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_preserves_unique_post_exchange_html_comment_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "do #visible. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("Keep this unrelated scratch note hidden."),
            "unique scratch comments must stay outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_scrubs_answered_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "### Re: backlog update and duplicate prompt corruption — gpt-5\n",
            "Implemented.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("### Re: backlog update and duplicate prompt corruption"),
            "answered exchange turn should remain:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "answered duplicate prompt text should be scrubbed from the HTML comment:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "answered duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_repairs_duplicate_exchange_close_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->

### Re: #xguard — gpt-5

Implemented.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] keep me
<!-- /agent:backlog -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: #xguard — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "response should be restored inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_response_before_prompt_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
Can you preserve the second paragraph too?
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
### Re: timeout fallback — gpt-5

Done.
<!-- agent:boundary:new -->
Can you preserve the second paragraph too?
<!-- /agent:exchange -->
";
        let response = "### Re: timeout fallback — gpt-5\n\nDone.\n";

        let repaired = normalize_final_template_content(
            &doc,
            base,
            Some(snapshot),
            None,
            merged,
            Some(response),
        )
        .unwrap();

        let prompt_tail = repaired
            .find("Can you preserve the second paragraph too?")
            .unwrap();
        let response_heading = repaired.find("### Re: timeout fallback").unwrap();
        let boundary = repaired.find("<!-- agent:boundary:").unwrap();
        let close = repaired.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            prompt_tail < response_heading,
            "prompt tail must move before response:\n{repaired}"
        );
        assert!(
            response_heading < boundary && boundary < close,
            "boundary must close the repaired response turn:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_repairs_duplicate_scaffold_close() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "❯ keep this prompt\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc_path)
            .expect("pure duplicated scaffold should be repaired");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        assert!(repaired.contains("❯ keep this prompt"));
    }

    #[test]
    fn normalize_template_structure_rejects_duplicate_scaffold_with_user_text() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        fs::write(&doc_path, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc_path).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }
}
