//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn stale_snapshot_reset_drift(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<(usize, usize)> {
    let snapshot_clean = strip_boundary_for_dedup(snapshot_doc);
    let current_clean = strip_boundary_for_dedup(current_doc);
    let snapshot_len = snapshot_clean.len();
    let current_len = current_clean.len();

    if snapshot_len <= current_len + STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES {
        return None;
    }
    if current_len as f64 / snapshot_len as f64 >= STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO {
        return None;
    }
    if crate::git::classify_safe_out_of_band_agent_doc_mutation(&snapshot_clean, &current_clean)
        .is_some()
    {
        return None;
    }

    Some((snapshot_len, current_len))
}

pub fn guard_no_stale_snapshot_reset_drift(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> Result<()> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(());
    };
    if let Ok(Some(cleaned)) =
        crate::template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(());
    }
    let Some((snapshot_len, current_len)) = stale_snapshot_reset_drift(snapshot_doc, current_doc)
    else {
        return Ok(());
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "stale_snapshot_reset_drift_blocked file={} phase={} snap_len={} file_len={}",
            file.display(),
            phase,
            snapshot_len,
            current_len
        ),
    );
    anyhow::bail!(
        "refusing {phase} for {}: snapshot is {} bytes but the visible file is {} bytes, which looks like a manual cleanup with stale snapshot/CRDT state. Reset the sidecars from the current file before writing: `agent-doc reset --from-current {}`",
        file.display(),
        snapshot_len,
        current_len,
        file.display()
    );
}

/// `#exch-intermix`: discriminator for the benign `live_prompt_drift_after_preflight`
/// closeout wedge. After the IPC drift guard adopts `content_ours` as the snapshot
/// (`guard_ipc_snapshot_adoption_against_live_prompt_drift`), that snapshot
/// (baseline + the `### Re:` response) is LARGER than the fragmented visible file,
/// so the next commit fails closed via `guard_no_stale_snapshot_reset_drift`
/// ("looks like a manual cleanup"). The response is not lost — it is intact in the
/// adopted snapshot — but the cycle is wedged and the operator must hand-recover.
///
/// This returns true only when that drift is provably the benign IPC/boundary
/// pattern: every prompt-bearing user line on disk is already owned by the
/// snapshot, so adopting the snapshot drops NO user content. It returns false
/// (fail closed) the instant the visible file carries a prompt target or a queue
/// `do [#id]` the snapshot lacks — genuine post-preflight user typing must never
/// be auto-recovered away. This is the airtight safety gate for automatic data
/// mutation, so it is intentionally conservative: it is baseline-free and only
/// trusts direct snapshot↔disk containment.
pub fn live_prompt_drift_auto_recovery_safe(snapshot: &str, file_content: &str) -> bool {
    // Must be the wedge shape: snapshot meaningfully larger than the visible file.
    // (Boundary markers are stripped before the length comparison, so `(HEAD)` /
    // guard annotations cannot manufacture or mask the wedge.)
    if stale_snapshot_reset_drift(snapshot, file_content).is_none() {
        return false;
    }
    // Any prompt-bearing PromptTarget present on disk but absent from the snapshot
    // is a genuine user prompt that adopting `content_ours` would drop — fail closed.
    let disk_only_changes = prompt_bearing_user_changes_between(snapshot, file_content);
    if disk_only_changes
        .iter()
        .any(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
    {
        return false;
    }
    // Same containment check for the queue component: a user-added `do [#id]` on
    // disk that the snapshot does not contain must not be silently dropped.
    let snapshot_queue_counts =
        queue_prompt_counts(&queue_prompt_texts(&queue_component_text(snapshot)));
    let file_queue_prompts = queue_prompt_texts(&queue_component_text(file_content));
    let mut seen: HashMap<String, usize> = HashMap::new();
    for prompt in file_queue_prompts {
        let count = seen.entry(prompt.clone()).or_insert(0);
        *count += 1;
        if *count > queue_prompt_count(&snapshot_queue_counts, &prompt) {
            return false;
        }
    }
    true
}

/// `#exch-intermix-falsedrop`: true when a recorded dropped prompt is still
/// present in the snapshot auto-recovery would adopt — as an active line, a
/// struck/consumed queue item (`~~…~~`), or echoed in a `### Re:` heading — so
/// adopting the snapshot loses nothing. The drift-time dropped-prompt record
/// compares the divergent IPC candidate against `content_ours` and therefore
/// false-positives on prompts that `content_ours` consumed or preserved; this
/// containment check reconciles those against the snapshot text. Returns false
/// only when the prompt text genuinely does not appear in the snapshot (real
/// user-content loss → fail closed). Strike markers are stripped from both sides
/// so a consumed item still matches its recorded prompt text.
pub(crate) fn snapshot_contains_dropped_prompt(snapshot: &str, prompt: &str) -> bool {
    let stripped = prompt.replace("~~", "");
    let needle = stripped.trim();
    if needle.is_empty() {
        return true;
    }
    snapshot.replace("~~", "").contains(needle)
}

/// `#exch-intermix`: auto-recover the benign `live_prompt_drift_after_preflight`
/// closeout wedge instead of stranding the response and forcing a manual
/// `git checkout HEAD` / `reset --from-current --preserve-session` / `finalize
/// --force-disk` recovery. When the IPC drift guard fired this cycle
/// (`content_ours` adopted as snapshot) and the snapshot is provably a superset of
/// the visible file's user content, write the snapshot to the working-tree file
/// (the `--force-disk` half of the manual recovery, with agent write-provenance)
/// so the commit boundary can stage the full response. Returns the recovered file
/// content on success (the caller must refresh its `file_content`), or `None` when
/// no recovery applies — leaving the existing fail-closed guard to handle it.
///
/// Because this is automatic data mutation it is intentionally narrow and fails
/// closed on any doubt:
/// - the cycle must carry the `ipc_snapshot_adoption_blocked` flag (the drift
///   guard ran this cycle and adopted `content_ours`),
/// - no dropped exchange/queue prompts may have been recorded this cycle (those
///   are the genuine-user-content-loss class `session-check` fails closed on),
/// - the current on-disk file must pass `live_prompt_drift_auto_recovery_safe`
///   (no disk-only user prompt the snapshot lacks).
pub fn try_auto_recover_live_prompt_drift(
    file: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(cycle) = crate::cycle_state::load(file)? else {
        return Ok(None);
    };
    if !cycle.ipc_snapshot_adoption_blocked {
        return Ok(None);
    }
    // #exch-intermix-falsedrop: a recorded dropped exchange/queue prompt only
    // represents real user-content loss when it is genuinely ABSENT from the
    // snapshot auto-recovery would adopt. A queue item consumed (struck) this
    // cycle, or a user prompt `content_ours` preserved, is recorded as "dropped"
    // by the drift-time candidate-vs-`content_ours` heuristic yet still survives
    // in the snapshot — adopting it loses nothing. Only bail when a dropped
    // prompt is missing from the snapshot; the snapshot↔disk containment gate
    // below stays authoritative for current on-disk content.
    let dropped_missing_from_snapshot = cycle
        .dropped_exchange_prompts
        .iter()
        .chain(cycle.dropped_queue_prompts.iter())
        .any(|prompt| !snapshot_contains_dropped_prompt(snapshot, prompt));
    if dropped_missing_from_snapshot {
        return Ok(None);
    }
    if !live_prompt_drift_auto_recovery_safe(snapshot, file_content) {
        return Ok(None);
    }

    let ipc_project_root = file
        .canonicalize()
        .ok()
        .map(|c| resolve_ipc_project_root_pub(&c));
    let ipc_listener_active = ipc_project_root
        .as_deref()
        .map(crate::ipc_socket::is_listener_active)
        .unwrap_or(false);

    if let Some(project_root) = ipc_project_root.as_deref()
        && ipc_listener_active
    {
        match try_editor_converge_live_prompt_drift(file, project_root, snapshot, file_content) {
            Ok(Some(recovered)) => {
                log_live_prompt_drift_auto_recovered(
                    file,
                    snapshot,
                    file_content,
                    true,
                    "editor_ipc",
                );
                crate::flow::proof::log_flow_event(
                    file,
                    crate::flow::types::FlowEvent::new(
                        crate::flow::types::FlowName::DocumentMutation,
                        crate::flow::types::FlowStage::IpcSnapshotAdoption,
                        crate::flow::types::FlowOutcome::Completed,
                    )
                    .with_reason("live_prompt_drift_auto_recovered"),
                );
                eprintln!(
                    "[commit] auto-recovered live_prompt_drift wedge for {} via editor IPC convergence ({} bytes)",
                    file.display(),
                    snapshot.len()
                );
                return Ok(Some(recovered));
            }
            Ok(None) => {}
            Err(err) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_error file={} error={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
    }

    if ipc_listener_active {
        crate::ops_log::log_op(
            file,
            &format!(
                "[jbstalecache] auto_recovery_disk_write_blocked file={} snap_len={} reason=editor_ipc_unconfirmed",
                file.display(),
                snapshot.len()
            ),
        );
        return Ok(None);
    }

    atomic_write(file, snapshot).with_context(|| {
        format!(
            "live_prompt_drift auto-recover write for {}",
            file.display()
        )
    })?;
    log_live_prompt_drift_auto_recovered(
        file,
        snapshot,
        file_content,
        ipc_listener_active,
        "disk_fallback",
    );
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::DocumentMutation,
            crate::flow::types::FlowStage::IpcSnapshotAdoption,
            crate::flow::types::FlowOutcome::Completed,
        )
        .with_reason("live_prompt_drift_auto_recovered"),
    );
    eprintln!(
        "[commit] auto-recovered live_prompt_drift wedge for {} — wrote adopted snapshot ({} bytes) to disk so the response lands instead of failing closed as a manual cleanup",
        file.display(),
        snapshot.len()
    );
    Ok(Some(snapshot.to_string()))
}

pub(crate) fn log_live_prompt_drift_auto_recovered(
    file: &Path,
    snapshot: &str,
    file_content: &str,
    ipc_listener_active: bool,
    transport: &str,
) {
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_drift_auto_recovered file={} snap_len={} file_len={} snap_hash={} ipc_listener_active={} transport={}",
            file.display(),
            snapshot.len(),
            file_content.len(),
            crate::ops_log::content_hash(snapshot),
            ipc_listener_active,
            transport
        ),
    );
}

pub(crate) fn try_editor_converge_live_prompt_drift(
    file: &Path,
    project_root: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let patches = live_prompt_drift_convergence_patches(file_content, snapshot)?;
    let frontmatter = live_prompt_drift_convergence_frontmatter(file_content, snapshot);
    if patches.is_empty() && frontmatter.is_none() {
        crate::ops_log::log_op(
            file,
            &format!(
                "[jbstalecache] editor_convergence_skipped file={} skip=no_component_or_frontmatter_delta",
                file.display()
            ),
        );
        return Ok(None);
    }

    let canonical = file.canonicalize()?;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical.to_string_lossy(),
        "patches": patches,
        "node_patches": [],
        "unmatched": "",
        "baseline": file_content,
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    if let Ok(Some(ref cycle)) = crate::cycle_state::load(file) {
        payload["cycle_id"] = serde_json::Value::String(cycle.cycle_id.clone());
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "[jbstalecache] editor_convergence_attempt file={} patch_id={} patches={} frontmatter={} snap_hash={}",
            file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-"),
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
            crate::ops_log::content_hash(snapshot)
        ),
    );

    match crate::ipc_socket::send_message(project_root, &payload) {
        Ok(Some(_ack)) => {
            let patch_id = payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let sidecar = poll_ack_content_sidecar(
                project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_no_ack_content file={} patch_id={} action=block_external_disk_write",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(None);
            };
            if crate::git::normalize_transient_agent_doc_markers(&recovered)
                == crate::git::normalize_transient_agent_doc_markers(snapshot)
            {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                Ok(Some(recovered))
            } else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_ack_mismatch file={} patch_id={} recovered_len={} snap_len={} action=block_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        snapshot.len()
                    ),
                );
                Ok(None)
            }
        }
        Ok(None) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_no_ack file={} action=block_external_disk_write",
                    file.display()
                ),
            );
            Ok(None)
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_send_failed file={} error={} action=block_external_disk_write",
                    file.display(),
                    err
                ),
            );
            Ok(None)
        }
    }
}

/// `#w42v`: converge a compacted document through the editor instead of a direct
/// disk write that diverges from the open JB buffer (`File Cache Conflict`).
///
/// Mirrors the `#q7jm` live_prompt_drift convergence: when a JB IPC listener is
/// active, send component `op:replace` patches for the changed components
/// (`exchange`, etc.) and verify the editor's ack content matches the compacted
/// target. Returns `Ok(true)` when converged via editor IPC (the caller skips
/// the disk write), `Ok(false)` only when no listener is running and a disk
/// fallback is safe, and `Err` when a running listener cannot prove the editor
/// mutation. The error is intentional: direct disk writes behind a running
/// JetBrains plugin are the File Cache Conflict source this guard prevents.
/// `#fcc0`/`#w42v`: converge a full-document write through the editor IPC when a
/// JB listener is active, returning `true` when the editor buffer has been
/// converged to `target` (no disk write needed) and `false` when no listener is
/// running and the caller may fall back to a disk write.
///
/// When a listener is active this computes the component-scoped delta between
/// `current_content` and `target` and applies it via `op:replace` patches through
/// the Document API, so the open buffer never diverges from disk and no
/// `File Cache Conflict` dialog fires. `source` labels the `ops.log` writeback
/// transport lines (`<source>_writeback ... transport=editor_ipc|disk_fallback|blocked`)
/// so each write site is attributable; see [`converge_document_or_disk`] for
/// the shared converge-or-disk wrapper every document-mutating write routes
/// through.
pub fn try_editor_converge(
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    let Some(project_root) = file
        .canonicalize()
        .ok()
        .map(|c| resolve_ipc_project_root_pub(&c))
    else {
        return Ok(false);
    };
    // `#fcc0e`: integrate the converger with the `#ipcdrift` degraded-latch
    // circuit breaker. A session whose socket listener latched degraded
    // (repeated ack timeouts) may skip the socket, but must still prefer the
    // plugin-owned file-IPC queue before refusing the write. The latch self-heals
    // (`#ipc-degrade-self-heal`):
    // `ipc_direct_disk_degraded` re-probes listener liveness and clears the
    // marker the moment the socket recovers.
    cleanup_legacy_ipc_degraded(&project_root);
    if current_content == target {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=already_current",
                file.display()
            ),
        );
        return Ok(true);
    }
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            log_ipc_dewedge_prefer_file_ipc(file, source);
            let canonical = file.canonicalize()?;
            let patch_id = uuid::Uuid::new_v4().to_string();
            let Some(payload) =
                editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
            else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} transport=blocked degraded_cause=no_component_delta action=refuse_external_disk_write",
                        file.display()
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is degraded (cause=no_component_delta)",
                    file.display()
                );
            };
            if try_editor_converge_file_ipc(
                file,
                &project_root,
                &payload,
                &patch_id,
                target,
                source,
                "listener_degraded",
            )? {
                return Ok(true);
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked degraded_cause=listener_degraded action=refuse_external_disk_write",
                    file.display()
                ),
            );
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is degraded",
                file.display()
            );
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[write] WARNING: {source} converge degradation check failed (non-fatal): {e}"
            );
        }
    }
    if !crate::ipc_socket::is_listener_active(&project_root) {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=disk_fallback reason=no_listener",
                file.display()
            ),
        );
        return Ok(false);
    }

    let canonical = file.canonicalize()?;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let Some(payload) =
        editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
    else {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=no_component_delta action=refuse_external_disk_write",
                file.display()
            ),
        );
        anyhow::bail!(
            "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_component_delta)",
            file.display()
        );
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_editor_convergence_attempt file={} patch_id={} patches={} node_patches={} frontmatter={}",
            file.display(),
            patch_id,
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload
                .get("node_patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
        ),
    );

    match crate::ipc_socket::send_message(&project_root, &payload) {
        Ok(Some(_ack)) => {
            let sidecar = poll_ack_content_sidecar(
                &project_root,
                &patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=no_ack_content action=refuse_external_disk_write",
                        file.display(),
                        patch_id
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_ack_content)",
                    file.display()
                );
            };
            if crate::git::normalize_transient_agent_doc_markers(&recovered)
                == crate::git::normalize_transient_agent_doc_markers(target)
            {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                // `#fcc0e`: a confirmed editor convergence proves the socket
                // listener is live; clear any accrued ack-timeout votes (the
                // degraded latch itself only clears on the liveness re-probe).
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                Ok(true)
            } else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=ack_mismatch recovered_len={} target_len={} action=refuse_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is active (reason=ack_mismatch)",
                    file.display()
                );
            }
        }
        Ok(None) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked reason=no_ack action=refuse_external_disk_write",
                    file.display()
                ),
            );
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_ack)",
                file.display()
            );
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked reason=send_failed error={} action=refuse_external_disk_write",
                    file.display(),
                    err
                ),
            );
            // `#fcc0e`: feed the de-wedge circuit breaker — a socket ack timeout
            // here counts toward the latch so a repeatedly-wedged listener trips
            // degraded and subsequent converges skip the doomed socket up front.
            if is_socket_ack_timeout_error(&err) {
                match record_ipc_socket_ack_timeout(&project_root, file, Some(&patch_id), source) {
                    Ok(true) => eprintln!(
                        "[write] IPC listener degraded for {} after repeated {source} ack timeouts",
                        file.display()
                    ),
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[write] WARNING: {source} converge ack-timeout record failed (non-fatal): {e}"
                    ),
                }
            }
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is active (reason=send_failed)",
                file.display()
            );
        }
    }
}

fn try_editor_converge_file_ipc(
    file: &Path,
    project_root: &Path,
    payload: &serde_json::Value,
    patch_id: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked degraded_cause={reason}_no_file_ipc action=refuse_external_disk_write",
                file.display()
            ),
        );
        return Ok(false);
    }
    let patch_file = patches_dir.join(format!("{patch_id}.json"));
    let patch_count = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0)
        + payload
            .get("node_patches")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or(0)
        + usize::from(payload.get("frontmatter").is_some());
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_file_ipc_convergence_attempt file={} patch_id={} degraded_cause={} patches={}",
            file.display(),
            patch_id,
            reason,
            patch_count
        ),
    );
    if write_ipc_and_poll(
        &patch_file,
        payload,
        file,
        patch_count,
        IpcPollOptions::convergence(project_root, Some(target)),
    )? {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} patch_id={} transport=file_ipc degraded_cause={}",
                file.display(),
                patch_id,
                reason
            ),
        );
        return Ok(true);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} patch_id={} transport=blocked degraded_cause={reason}_file_ipc_unproven action=refuse_external_disk_write",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub(crate) fn editor_convergence_payload(
    canonical_file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
    patch_id: &str,
) -> Result<Option<serde_json::Value>> {
    let mut patches = live_prompt_drift_convergence_patches(current_content, target)?;
    let frontmatter = live_prompt_drift_convergence_frontmatter(current_content, target);
    let node_patches = queue_consume_node_patches(current_content, target, source);

    if !node_patches.is_empty() {
        let node_patched_components = node_patches
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        patches.retain(|patch| {
            patch
                .get("component")
                .and_then(|value| value.as_str())
                .is_none_or(|component| !node_patched_components.contains(component))
        });
    }

    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        return Ok(None);
    }

    let normalized_baseline = crate::git::normalize_transient_agent_doc_markers(current_content);
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical_file.to_string_lossy(),
        "patches": patches,
        "node_patches": node_patches,
        "unmatched": "",
        "baseline": current_content,
        "baseline_hash": crate::debounce::content_hash(current_content),
        "baseline_normalized_hash": crate::debounce::content_hash(&normalized_baseline),
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    Ok(Some(payload))
}

fn queue_consume_node_patches(
    current_content: &str,
    target: &str,
    source: &str,
) -> Vec<serde_json::Value> {
    if source != "queue_consume" {
        return Vec::new();
    }
    build_ipc_node_patches_json(Some(current_content), Some(target))
        .into_iter()
        .filter(|patch| patch.get("component").and_then(|value| value.as_str()) == Some("queue"))
        .collect()
}

/// `#fcc0`: the single converge-or-disk gate every document-mutating write site
/// routes through. When a JB editor listener is active it converges `target`
/// through the editor IPC (component `op:replace` — no `File Cache Conflict`
/// dialog); otherwise, it falls back to the guarded disk write only when no
/// listener is running. Active-listener failures fail closed instead of writing
/// behind the editor. `current` is the expected current document content (held
/// under the caller's doc lock) and drives both the editor delta and the
/// idle+current disk guard.
pub fn converge_document_or_disk(
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    atomic_write_if_current_pub(file, target, current, source)
}

/// `#fcc0`: converge-or-**plain**-disk gate for the component-mutating CLI write
/// sites that historically wrote straight to disk with a bare `std::fs::write`
/// (the `agent:pending` / `agent:review` operator ops, `dedupe`, preflight
/// `run_pending_maintenance`, the `agent_doc_pipeline:` frontmatter mirror). When
/// a JB editor listener is active it converges `target` through the editor IPC
/// (component/frontmatter `op:replace` — no `File Cache Conflict` dialog);
/// otherwise it falls back to the SAME plain disk write those sites already did
/// only when no live IDE listener is running, so with no live IDE the behavior
/// is byte-identical to before this gate.
///
/// Unlike [`converge_document_or_disk`], the no-listener disk fallback here is
/// an unguarded `std::fs::write` rather than the visible-buffer-proof guarded
/// write. These sites are CLI document mutations that must not newly fail when
/// no editor is attached. `current` is the expected current on-disk content the
/// editor delta is computed against; `source` labels the `ops.log`
/// `<source>_writeback` line.
pub fn converge_or_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    std::fs::write(file, target)
        .with_context(|| format!("{source}: failed to write {}", file.display()))
}

pub(crate) fn live_prompt_drift_convergence_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    let current_components = component::parse(file_content)
        .with_context(|| "failed to parse current document for editor convergence")?;
    let snapshot_components = component::parse(snapshot)
        .with_context(|| "failed to parse adopted snapshot for editor convergence")?;
    let current_by_name: HashMap<&str, &component::Component> = current_components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect();
    let mut patches = Vec::new();
    for snapshot_component in &snapshot_components {
        let Some(current_component) = current_by_name.get(snapshot_component.name.as_str()) else {
            continue;
        };
        let current_body = current_component.content(file_content);
        let snapshot_body = snapshot_component.content(snapshot);
        if crate::git::normalize_transient_agent_doc_markers(current_body)
            == crate::git::normalize_transient_agent_doc_markers(snapshot_body)
        {
            continue;
        }
        patches.push(serde_json::json!({
            "component": snapshot_component.name,
            "content": snapshot_body,
            "op": "replace",
        }));
    }
    Ok(patches)
}

pub(crate) fn live_prompt_drift_convergence_frontmatter(
    file_content: &str,
    snapshot: &str,
) -> Option<String> {
    let file_frontmatter = raw_frontmatter_yaml(file_content);
    let snapshot_frontmatter = raw_frontmatter_yaml(snapshot)?;
    if file_frontmatter == Some(snapshot_frontmatter) {
        None
    } else {
        Some(snapshot_frontmatter.to_string())
    }
}

pub(crate) fn raw_frontmatter_yaml(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        // Snapshot owns the response the fragmented disk file lost; no disk-only
        // user prompt → safe to auto-recover.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        assert!(
            live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "benign live-prompt-drift wedge should be recoverable"
        );
    }
    #[test]
    fn live_prompt_drift_convergence_patches_builds_replace_patch_for_exchange() {
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();

        let patches = live_prompt_drift_convergence_patches(&fragmented, &snapshot).unwrap();

        assert_eq!(patches.len(), 1, "only exchange should need convergence");
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("### Re: do #fix"),
            "replace payload should carry the recovered response body: {patches:?}"
        );
    }
    #[test]
    fn try_compact_editor_converge_falls_back_to_disk_without_listener() {
        // `#w42v`: with no live JB IPC listener, compact convergence must report
        // disk fallback (Ok(false)) so the caller does the guarded disk write —
        // never silently skip the compaction.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let current = crate::test_support::drift_baseline();
        let compacted = crate::test_support::drift_content_ours();
        std::fs::write(&doc, &current).unwrap();

        let converged = try_editor_converge(&doc, &compacted, &current, "compact").unwrap();
        assert!(
            !converged,
            "without a live JB IPC listener, compact must fall back to the disk write"
        );
    }
    /// Pre-compact document with a multi-item `queue` an operator could be
    /// concurrently editing while compaction archives the exchange tail.
    /// Post-compact document: the `exchange` collapses to a summary marker while
    /// the `queue` is byte-identical to the source (compaction never touches it).
    #[test]
    fn compact_convergence_is_exchange_scoped_preserving_concurrent_queue_edits() {
        // `#jbcompactcrdt`/`#w42v`: compaction only rewrites `exchange`, so the
        // editor-IPC convergence patch must be component-scoped to `exchange` and
        // never carry a `queue` replace. That scoping is exactly what lets an
        // operator concurrently typing queue items survive compaction without a
        // JB `File Cache Conflict` — the editor applies the exchange `op:replace`
        // via the Document API and leaves the live queue buffer untouched.
        let source = crate::test_support::compact_convergence_source();
        let compacted = crate::test_support::compact_convergence_compacted();

        let patches = live_prompt_drift_convergence_patches(&source, &compacted).unwrap();

        assert_eq!(
            patches.len(),
            1,
            "only exchange changed during compaction; queue must not be patched: {patches:?}"
        );
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("*Compacted. Content archived"),
            "the exchange replace must carry the compacted summary body: {patches:?}"
        );
        assert!(
            !patches.iter().any(|patch| patch["component"] == "queue"),
            "a queue replace would clobber the operator's concurrent edits: {patches:?}"
        );
    }
    #[test]
    fn try_compact_editor_converge_converges_via_editor_ipc_with_listener() {
        // `#jbcompactcrdt`/`#w42v`: with a live JB IPC listener, compaction must
        // converge the compacted document through the editor (`transport=editor_ipc`)
        // instead of a direct disk write that diverges from the open buffer and
        // raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::compact_convergence_source();
        let compacted = crate::test_support::compact_convergence_compacted();
        fs::write(&doc, &source).unwrap();

        // The fake editor acks with the compacted content, mirroring a JB plugin
        // that applied the exchange `op:replace` and converged its buffer.
        let _listener = crate::test_support::start_live_prompt_drift_ack_listener(
            dir.path(),
            compacted.clone(),
        );
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &compacted, &source, "compact").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_editor_convergence_attempt"),
            "compact convergence attempt should be observable in ops.log:\n{log}"
        );
        assert!(
            log.contains("compact_writeback") && log.contains("transport=editor_ipc"),
            "successful compaction must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged compaction must not also take the disk fallback:\n{log}"
        );
    }
    /// Pre-consume document with a `go` queue head an operator could be concurrently
    /// editing while the queue is struck.
    /// Post-consume document: only the `queue` head is struck; every other
    /// component is byte-identical (queue consume never touches the exchange).
    #[test]
    fn queue_consume_writeback_converges_via_editor_ipc_with_listener() {
        // `#fcc0`: the queue-consume write must route through the shared
        // converger so an active JB listener converges the struck queue through
        // the editor (`transport=editor_ipc`, `queue_consume`-labelled) instead of
        // a direct disk write that raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener =
            crate::test_support::start_live_prompt_drift_ack_listener(dir.path(), target.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_convergence_attempt"),
            "queue-consume convergence attempt should be source-labelled in ops.log:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback") && log.contains("transport=editor_ipc"),
            "a converged queue consume must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged queue consume must not also take the disk fallback:\n{log}"
        );
    }
    #[test]
    fn queue_consume_editor_convergence_payload_is_node_keyed_and_fenced() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let payload = editor_convergence_payload(
            &doc.canonicalize().unwrap(),
            &target,
            &source,
            "queue_consume",
            "patch-queue-consume",
        )
        .unwrap()
        .expect("queue consume should produce an editor convergence payload");

        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(crate::debounce::content_hash(&source).as_str()),
            "socket convergence payloads must carry the raw generation fence"
        );
        assert_eq!(
            payload["baseline_normalized_hash"].as_str(),
            Some(
                crate::debounce::content_hash(&crate::git::normalize_transient_agent_doc_markers(
                    &source
                ))
                .as_str()
            ),
            "socket convergence payloads must also carry the transient-marker-normalized fence"
        );
        assert!(
            payload["patches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|patch| patch["component"] != "queue"),
            "queue consume must not send a broad legacy queue component replace: {payload:?}"
        );
        let node_patches = payload["node_patches"].as_array().unwrap();
        assert!(
            node_patches
                .iter()
                .any(|patch| { patch["component"] == "queue" && patch["op"] == "strike" }),
            "queue consume must be expressed as an exact node-keyed strike: {payload:?}"
        );
    }
    #[test]
    fn try_editor_converge_treats_active_listener_already_current_as_noop() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &source, &source, "pending_write").unwrap();
        assert!(
            converged,
            "already-current active-listener converge should be a no-op success"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "already-current converge must not mutate the document"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback") && log.contains("transport=already_current"),
            "already-current converge should be observable without disk fallback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback") && !log.contains("transport=blocked"),
            "already-current converge must not fall back or block:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_falls_back_to_guarded_disk_without_listener() {
        // `#fcc0`: with no live JB listener the shared converger must land the
        // target on disk via the guarded write and record the source-labelled
        // disk fallback — never silently skip the write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_document_or_disk(&doc, &target, &source, "queue_consume").unwrap();

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_fallback")
                && log.contains("reason=no_listener"),
            "a no-listener queue consume must record the source-labelled disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_blocks_disk_fallback_with_active_listener_without_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without ack-content should block disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "an unproven editor IPC apply must not be followed by an external disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content"),
            "active listener failure must be logged as a blocked disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_falls_back_to_plain_disk_without_listener() {
        // `#fcc0`: the unguarded converge-or-disk gate (used by the pending/review,
        // dedupe, preflight-maintenance, and pipeline-mirror write sites) must, with
        // no live JB listener, land the target via a plain disk write and record the
        // source-labelled disk fallback — never silently skip the write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_or_disk_write(&doc, &source, &target, "pending_write").unwrap();

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=disk_fallback")
                && log.contains("reason=no_listener"),
            "a no-listener plain converge must record the source-labelled disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_blocks_plain_disk_fallback_with_active_listener_without_ack_content()
    {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let err = converge_or_disk_write(&doc, &source, &target, "pending_write")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without ack-content should block plain disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "plain component maintenance must not write behind a running editor plugin"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content"),
            "active listener failure must be logged as a blocked plain disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        // Snapshot == file: no wedge, nothing to recover, must not fire.
        let snapshot = crate::test_support::drift_content_ours();
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &snapshot),
            "no drift means no auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        // The visible file carries a NEW user prompt the snapshot never saw —
        // adopting content_ours would silently drop it. Fail closed.
        let snapshot = crate::test_support::drift_content_ours();
        let mut fragmented = crate::test_support::drift_baseline();
        fragmented = fragmented.replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "a disk-only user prompt must block auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_queue_item() {
        // A user-added `do [#id]` queue line on disk the snapshot lacks must
        // block auto-recovery (the silent-queue-deletion class).
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(&snapshot, &fragmented),
            "a disk-only queue item must block auto-recovery"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_writes_snapshot_when_blocked_and_safe() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // The drift guard fired this cycle and adopted content_ours.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should return the adopted snapshot content"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the working-tree file should now carry the full response"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("live_prompt_drift_auto_recovered"),
            "auto-recovery must leave an observable ops.log marker:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_prefers_editor_ipc_when_listener_active() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener =
            crate::test_support::start_live_prompt_drift_ack_listener(dir.path(), snapshot.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should accept the editor-applied snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the fake editor listener should converge the working tree through IPC"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_attempt")
                && log.contains("[jbstalecache] editor_convergence_succeeded"),
            "active listener recovery should be observable as editor convergence:\n{log}"
        );
        assert!(
            log.contains("live_prompt_drift_auto_recovered")
                && log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true"),
            "recovery marker should name the editor transport:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "successful editor convergence must not take the stale-cache disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_blocks_disk_fallback_with_active_listener_without_ack_content()
     {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "active listener without ack-content must block binary-owned disk recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "auto-recovery must not write the adopted snapshot behind the editor"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_no_ack_content")
                && log.contains("action=block_external_disk_write"),
            "unproven editor convergence must be logged as a blocked write:\n{log}"
        );
        assert!(
            log.contains("[jbstalecache] auto_recovery_disk_write_blocked")
                && log.contains("reason=editor_ipc_unconfirmed"),
            "auto-recovery must record that it refused the disk fallback:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "active listener recovery must not take or advertise the disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_without_blocked_flag() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // A cycle exists but the drift guard never fired (flag stays false) →
        // not the wedge we own.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "without the drift flag this is not the auto-recovery case"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when recovery does not apply"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_when_dropped_prompts_recorded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // A genuine dropped user prompt was recorded this cycle → session-check
        // owns the fail-closed; auto-recovery must NOT paper over it.
        crate::cycle_state::record_dropped_exchange_prompts(&doc, &["do #dropped".to_string()])
            .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "recorded dropped prompts must block auto-recovery (fail closed)"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when a dropped prompt was recorded"
        );
    }
    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );
        // Consumed (struck) item still present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        // Active item present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        // Genuinely absent → real loss.
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_fires_when_dropped_prompt_is_consumed_in_snapshot() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        // Snapshot consumed the queued `do [#fix]` (struck) and carries the full
        // `### Re:` response; the fragmented disk file also struck it but lost the
        // response body → wedge shape.
        let snapshot =
            crate::test_support::drift_content_ours().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        let fragmented =
            crate::test_support::drift_baseline().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // The drift heuristic recorded the consumed item as a dropped queue prompt.
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#fix]".to_string()]).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_some(),
            "a dropped prompt that survives (struck) in the snapshot must not block recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "auto-recovery must write the adopted snapshot to disk"
        );

        // `#jbstalecache`: the recovery write records the IPC-listener state so the
        // operator can correlate a stale-cache dialog with this disk write. No live
        // listener exists in the test env, so the canonical marker reports
        // `ipc_listener_active=false` and the dedicated stale-cache-risk line stays
        // silent (it only fires when a listener is genuinely active).
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("live_prompt_drift_auto_recovered")
                && ops_log.contains("ipc_listener_active=false"),
            "recovery marker must record the IPC-listener state:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[jbstalecache]"),
            "the stale-cache-risk marker must stay silent without an active listener:\n{ops_log}"
        );
    }
    #[test]
    fn stale_snapshot_reset_drift_blocks_large_snapshot_only_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write");

        let message = result
            .expect_err("stale larger snapshot must fail closed")
            .to_string();
        assert!(
            message.contains("agent-doc reset --from-current"),
            "recovery guidance should name deterministic sidecar reset: {message}"
        );
    }
    #[test]
    fn stale_snapshot_reset_drift_allows_small_size_delta() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = "a".repeat(1000);
        let current = "b".repeat(940);

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "stream write");

        assert!(
            result.is_ok(),
            "minor snapshot/file size drift should not block writes"
        );
    }
}
