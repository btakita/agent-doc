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

