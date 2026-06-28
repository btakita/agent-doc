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
) -> Result<bool> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(false);
    };
    if let Ok(Some(cleaned)) =
        crate::template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(false);
    }
    let Some((snapshot_len, current_len)) = stale_snapshot_reset_drift(snapshot_doc, current_doc)
    else {
        return Ok(false);
    };
    if let Some(reason) = classify_stale_snapshot_visible_rebase(file, snapshot_doc, current_doc) {
        crate::snapshot::save(file, current_doc)?;
        let crdt = crate::crdt::CrdtDoc::from_text(current_doc).encode_state();
        crate::snapshot::save_document_crdt(file, &crdt, current_doc)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "stale_snapshot_visible_rebased file={} phase={} reason={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                reason,
                snapshot_len,
                current_len
            ),
        );
        return Ok(true);
    }

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

fn classify_stale_snapshot_visible_rebase(
    file: &Path,
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<&'static str> {
    // `#provauth3`: the turn scope is the per-turn operator-edit provenance record,
    // but it is ABSENT after a `/clear` (fresh session resume). Do not hard-require
    // it — a binary-authored compaction is a known-origin reduction whose authority
    // does not depend on a live turn scope. Non-exchange component drift still needs
    // the scope to be classified as turn-independent, so that path fails closed
    // below when the scope is missing.
    let scope = crate::turn_scope_store::load(file);
    // Known binary-origin signal: the binary recorded that it compacted this
    // document's exchange within the recent window. That makes a snapshot→visible
    // exchange shrink authoritative binary state, not a "suspicious manual cleanup"
    // — the central #provauth3 replacement of a content guess with a recorded
    // origin fact. (After a `/clear` the on-disk marker survives, so a resumed
    // session can still recognize its own prior compaction.)
    let recent_binary_compaction =
        crate::session_accretion::recent_exchange_compaction_timestamp(file)
            .ok()
            .flatten()
            .is_some();
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        return None;
    }

    let (snapshot_frontmatter, snapshot_body) = crate::frontmatter::parse(snapshot_doc).ok()?;
    let (current_frontmatter, current_body) = crate::frontmatter::parse(current_doc).ok()?;
    if !frontmatter_agent_only_equivalent(&snapshot_frontmatter, &current_frontmatter) {
        return None;
    }

    let snap_components = crate::component::parse(snapshot_body).ok()?;
    let current_components = crate::component::parse(current_body).ok()?;
    if snap_components.is_empty() || snap_components.len() != current_components.len() {
        return None;
    }

    let mut saw_exchange_trim = false;
    let mut saw_independent_component = false;
    for (snap_comp, current_comp) in snap_components.iter().zip(current_components.iter()) {
        if snap_comp.name != current_comp.name {
            return None;
        }
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != current_comp.patch_mode()
        {
            return None;
        }

        let snap_content =
            crate::git::normalize_component_content_for_absorb(snap_comp.content(snapshot_body));
        let current_content =
            crate::git::normalize_component_content_for_absorb(current_comp.content(current_body));
        if snap_content == current_content {
            continue;
        }

        if snap_comp.name == "exchange" {
            if exchange_change_is_safe_historical_reduction(
                snap_comp.content(snapshot_body),
                current_comp.content(current_body),
            ) {
                saw_exchange_trim = true;
                continue;
            }
            return None;
        }

        // A non-exchange component changed: this requires the turn scope to prove
        // the change is independent of the current turn. Without a scope we cannot
        // make that judgment, so fail closed (unchanged pre-#provauth3 behavior —
        // the old `?` on the scope load returned None before reaching this point).
        match scope.as_ref() {
            Some(scope)
                if component_change_is_turn_independent(
                    snapshot_body,
                    current_body,
                    &snap_comp.name,
                    scope,
                ) =>
            {
                saw_independent_component = true;
                continue;
            }
            _ => return None,
        }
    }

    match (saw_exchange_trim, saw_independent_component) {
        (true, true) => Some("historical_exchange_trim_unrelated_drift"),
        // Exchange-only safe reduction. Allow the rebase with a live turn scope
        // (in-session historical trim, the pre-#provauth3 path) OR a recorded
        // binary-origin compaction (post-`/clear` resume). Without either
        // provenance signal, fail closed so a genuine manual cleanup still trips
        // the guard and the operator is told to `reset --from-current`.
        (true, false) => {
            if scope.is_some() || recent_binary_compaction {
                Some("historical_exchange_trim")
            } else {
                None
            }
        }
        (false, true) => Some("unrelated_component_drift"),
        (false, false) => None,
    }
}

fn active_capture_response_removed(file: &Path, snapshot_doc: &str, current_doc: &str) -> bool {
    let Ok(Some(state)) = crate::cycle_state::load(file) else {
        return false;
    };
    if !state.is_open() {
        return false;
    }
    let Ok(Some(capture)) = crate::capture::load_active(file) else {
        return false;
    };
    !capture.response_body.trim().is_empty()
        && crate::repair::response_already_applied(snapshot_doc, &capture.response_body)
        && !crate::repair::response_already_applied(current_doc, &capture.response_body)
}

fn frontmatter_agent_only_equivalent(
    snapshot: &crate::frontmatter::Frontmatter,
    current: &crate::frontmatter::Frontmatter,
) -> bool {
    normalized_frontmatter_without_agent(snapshot)
        .zip(normalized_frontmatter_without_agent(current))
        .is_some_and(|(snapshot, current)| snapshot == current)
}

fn normalized_frontmatter_without_agent(
    frontmatter: &crate::frontmatter::Frontmatter,
) -> Option<serde_yaml::Value> {
    let mut value = serde_yaml::to_value(frontmatter).ok()?;
    if let serde_yaml::Value::Mapping(map) = &mut value {
        map.remove(serde_yaml::Value::String("agent".to_string()));
    }
    Some(value)
}

fn component_change_is_turn_independent(
    snap_body: &str,
    current_body: &str,
    component_name: &str,
    scope: &agent_doc_core::turn_scope::TurnScope,
) -> bool {
    use agent_doc_core::op_log::OpActor;
    use agent_doc_core::turn_scope::{Address, classify_op};

    let events: Vec<_> = agent_doc_markdown_ast::events::diff_node_events(snap_body, current_body)
        .into_iter()
        .filter(|event| event.component == component_name)
        .collect();
    if events.is_empty() {
        return false;
    }

    events.iter().all(|event| {
        let address = Address::from_component_node_key(&event.component, &event.node_key);
        let node_index = event.after_index.or(event.before_index);
        !classify_op(
            OpActor::User,
            event.kind.as_str(),
            &address,
            node_index,
            scope,
        )
        .affects_turn()
    })
}

fn exchange_change_is_complete_response_block_trim(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let blocks = exchange_response_block_ranges(snapshot);
    if blocks.is_empty() {
        return false;
    }

    let mut snapshot_pos = 0usize;
    let mut current_pos = 0usize;
    let mut removed = 0usize;
    for block in blocks {
        let prefix = &snapshot[snapshot_pos..block.start];
        if !current[current_pos..].starts_with(prefix) {
            return false;
        }
        current_pos += prefix.len();

        let block_text = &snapshot[block.clone()];
        if current[current_pos..].starts_with(block_text) {
            current_pos += block_text.len();
        } else {
            removed += 1;
        }
        snapshot_pos = block.end;
    }

    removed > 0 && current[current_pos..] == snapshot[snapshot_pos..]
}

fn exchange_change_is_safe_historical_reduction(snapshot: &str, current: &str) -> bool {
    exchange_change_is_complete_response_block_trim(snapshot, current)
        || exchange_change_is_compact_summary_replacement(snapshot, current)
}

fn exchange_change_is_compact_summary_replacement(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let current_trimmed = current.trim_start();
    if !current_trimmed.starts_with("### Session Summary") {
        return false;
    }
    if !current.contains("*Compacted. Content archived to `")
        && !current.contains("Compacted content:")
    {
        return false;
    }

    let snapshot_headings = exchange_response_heading_lines(snapshot);
    if snapshot_headings.is_empty() {
        return false;
    }
    let current_headings = exchange_response_heading_lines(current);
    if current_headings
        .iter()
        .any(|heading| !snapshot_headings.contains(heading))
    {
        return false;
    }

    current_headings.len() < snapshot_headings.len()
}

fn exchange_response_heading_lines(exchange: &str) -> Vec<String> {
    exchange
        .lines()
        .filter(|line| is_exchange_response_heading(line))
        .map(|line| line.trim().to_string())
        .collect()
}

fn exchange_response_block_ranges(exchange: &str) -> Vec<std::ops::Range<usize>> {
    #[derive(Clone, Copy)]
    struct Line<'a> {
        start: usize,
        end: usize,
        text: &'a str,
    }

    let mut lines = Vec::new();
    let mut offset = 0usize;
    for line in exchange.split_inclusive('\n') {
        let end = offset + line.len();
        lines.push(Line {
            start: offset,
            end,
            text: line,
        });
        offset = end;
    }
    if offset < exchange.len() {
        lines.push(Line {
            start: offset,
            end: exchange.len(),
            text: &exchange[offset..],
        });
    }

    let heading_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_exchange_response_heading(line.text).then_some(idx))
        .collect();
    let mut ranges = Vec::new();
    for (pos, &heading_idx) in heading_indices.iter().enumerate() {
        let mut end_idx = heading_indices.get(pos + 1).copied().unwrap_or(lines.len());
        for (idx, line) in lines.iter().enumerate().take(end_idx).skip(heading_idx + 1) {
            if is_exchange_boundary(line.text) {
                end_idx = idx;
                break;
            }
        }
        ranges.push(lines[heading_idx].start..lines[end_idx - 1].end);
    }
    ranges
}

fn is_exchange_response_heading(line: &str) -> bool {
    line.trim_start().starts_with("### Re:")
}

fn is_exchange_boundary(line: &str) -> bool {
    line.trim_start().starts_with("<!-- agent:boundary:")
}

/// `#exch-intermix`: realtime resolver for the `live_prompt_drift_after_preflight`
/// closeout wedge. After the IPC drift guard carries the agent response in the
/// snapshot candidate, the visible document may still be missing that response
/// while carrying newer operator-visible edits. Recovery must rebase only the
/// missing response block onto the current document; it must not adopt the
/// snapshot as a whole-document authority.
///
/// This returns true only when the current realtime document can preserve the
/// operator-visible state and accept the missing agent response as a delta. It
/// never authorizes wholesale snapshot adoption: queue/backlog/frontmatter and
/// other disjoint operator edits stay as they are in `file_content`, while only
/// the newest missing `### Re:` block from `snapshot` may be appended to
/// `agent:exchange`. Prompt-target edits inside the visible file still fail
/// closed because the resolver cannot prove where the response should land
/// relative to a newly typed prompt.
pub fn live_prompt_drift_auto_recovery_safe(snapshot: &str, file_content: &str) -> bool {
    live_prompt_drift_recovery_target(snapshot, file_content).is_some()
}

fn live_prompt_drift_recovery_target(snapshot: &str, file_content: &str) -> Option<String> {
    // A newly typed prompt inside `agent:exchange` makes response placement
    // ambiguous. Queue/backlog prompt text is disjoint operator state and is
    // preserved by the merged target below.
    if exchange_has_disk_only_prompt_target(snapshot, file_content) {
        return None;
    }

    let response_block = latest_missing_snapshot_response_block(snapshot, file_content)?;
    let components = component::parse(file_content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == AGENT_RESPONSE_COMPONENT)?;
    let mut exchange_body = exchange.content(file_content).to_string();
    push_materialization_segment(&mut exchange_body, &response_block);
    let recovered = exchange.replace_content(file_content, &exchange_body);
    (normalize_visible_recovery_compare(&recovered)
        != normalize_visible_recovery_compare(file_content))
    .then_some(recovered)
}

fn exchange_has_disk_only_prompt_target(snapshot: &str, file_content: &str) -> bool {
    let (Ok(snapshot_components), Ok(file_components)) =
        (component::parse(snapshot), component::parse(file_content))
    else {
        return true;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return true;
    };
    let snapshot_counts = exchange_prompt_target_counts(snapshot_exchange.content(snapshot));
    let mut seen: HashMap<String, usize> = HashMap::new();
    for prompt in exchange_prompt_target_lines(file_exchange.content(file_content)) {
        let count = seen.entry(prompt.clone()).or_insert(0);
        *count += 1;
        if *count > snapshot_counts.get(&prompt).copied().unwrap_or(0) {
            return true;
        }
    }
    false
}

fn exchange_prompt_target_counts(exchange_body: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for prompt in exchange_prompt_target_lines(exchange_body) {
        *counts.entry(prompt).or_insert(0) += 1;
    }
    counts
}

fn exchange_prompt_target_lines(exchange_body: &str) -> Vec<String> {
    exchange_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('❯') || crate::diff::text_line_looks_like_prompt_target(trimmed)
            {
                Some(
                    trimmed
                        .strip_prefix('❯')
                        .unwrap_or(trimmed)
                        .trim()
                        .to_string(),
                )
            } else {
                None
            }
        })
        .collect()
}

fn latest_missing_snapshot_response_block(snapshot: &str, file_content: &str) -> Option<String> {
    let (Ok(snapshot_components), Ok(file_components)) =
        (component::parse(snapshot), component::parse(file_content))
    else {
        return None;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return None;
    };
    let snapshot_body = snapshot_exchange.content(snapshot);
    let file_body = file_exchange.content(file_content);
    let file_norm = normalize_visible_recovery_compare(file_body);
    for range in exchange_response_block_ranges(snapshot_body)
        .into_iter()
        .rev()
    {
        let block = &snapshot_body[range];
        let block_norm = normalize_visible_recovery_compare(block);
        let block_trimmed = block_norm.trim();
        if block_trimmed.is_empty() {
            continue;
        }
        if !file_norm.contains(block_trimmed) {
            return Some(block.to_string());
        }
    }
    None
}

fn normalize_visible_recovery_compare(content: &str) -> String {
    crate::git::normalize_transient_agent_doc_markers(&strip_boundary_for_dedup(content))
}

/// `#exch-intermix-falsedrop`: true when a recorded dropped prompt is still
/// present in the response candidate — as an active line, a
/// struck/consumed queue item (`~~…~~`), or echoed in a `### Re:` heading — so
/// response recovery loses nothing. The drift-time dropped-prompt record
/// compares the divergent IPC candidate against `content_ours` and therefore
/// false-positives on prompts that `content_ours` consumed or preserved; this
/// containment check reconciles those against the response candidate text.
/// Returns false only when the prompt text genuinely does not appear in the
/// candidate (real user-content loss -> fail closed). Strike markers are stripped from both sides
/// so a consumed item still matches its recorded prompt text.
pub(crate) fn snapshot_contains_dropped_prompt(snapshot: &str, prompt: &str) -> bool {
    let stripped = prompt.replace("~~", "");
    let needle = stripped.trim();
    if needle.is_empty() {
        return true;
    }
    snapshot.replace("~~", "").contains(needle)
}

/// `#exch-intermix`: auto-recover the `live_prompt_drift_after_preflight`
/// closeout wedge by rebasing the missing agent response onto the realtime
/// document. Returns the recovered file content on success (the caller must
/// refresh its `file_content` and snapshot), or `None` when no recovery applies —
/// leaving the existing fail-closed guard to handle it.
///
/// Because this is automatic data mutation it is intentionally narrow and fails
/// closed on any doubt:
/// - the cycle must carry the `ipc_snapshot_adoption_blocked` flag (the drift
///   guard ran and preserved the response candidate for recovery),
/// - any recorded dropped prompt must still be present in the response candidate,
/// - realtime resolution must produce a response-only merge target.
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
    // response candidate. A queue item consumed (struck) this cycle, or a user
    // prompt `content_ours` preserved, is recorded as "dropped" by the
    // drift-time candidate-vs-`content_ours` heuristic yet still survives in the
    // candidate. Only bail when a dropped prompt is missing from that candidate;
    // the realtime merge target below remains authoritative for current
    // operator-visible content.
    let dropped_missing_from_snapshot = cycle
        .dropped_exchange_prompts
        .iter()
        .chain(cycle.dropped_queue_prompts.iter())
        .any(|prompt| !snapshot_contains_dropped_prompt(snapshot, prompt));
    if dropped_missing_from_snapshot {
        return Ok(None);
    }
    let Some(recovery_target) = live_prompt_drift_recovery_target(snapshot, file_content) else {
        return Ok(None);
    };

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
        match try_editor_converge_live_prompt_drift(
            file,
            project_root,
            &recovery_target,
            file_content,
        ) {
            Ok(Some(recovered)) => {
                log_live_prompt_drift_auto_recovered(
                    file,
                    &recovery_target,
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
                    recovery_target.len()
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
                "[jbstalecache] auto_recovery_disk_write_blocked file={} target_len={} reason=editor_ipc_unconfirmed",
                file.display(),
                recovery_target.len()
            ),
        );
        return Ok(None);
    }

    atomic_write(file, &recovery_target).with_context(|| {
        format!(
            "live_prompt_drift auto-recover write for {}",
            file.display()
        )
    })?;
    crate::snapshot::save(file, &recovery_target)?;
    let crdt_doc = crate::crdt::CrdtDoc::from_text(&recovery_target);
    crate::snapshot::save_document_crdt(file, &crdt_doc.encode_state(), &recovery_target)?;
    log_live_prompt_drift_auto_recovered(
        file,
        &recovery_target,
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
        "[commit] auto-recovered live_prompt_drift wedge for {} — merged the missing response into the realtime document ({} bytes) so operator-visible edits stay authoritative",
        file.display(),
        recovery_target.len()
    );
    Ok(Some(recovery_target))
}

pub(crate) fn log_live_prompt_drift_auto_recovered(
    file: &Path,
    target: &str,
    file_content: &str,
    ipc_listener_active: bool,
    transport: &str,
) {
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_drift_auto_recovered file={} target_len={} file_len={} target_hash={} ipc_listener_active={} transport={}",
            file.display(),
            target.len(),
            file_content.len(),
            crate::ops_log::content_hash(target),
            ipc_listener_active,
            transport
        ),
    );
}

/// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`) — pure classifier for the
/// typed `write_wedged` fact the route-owned supervisor uses as a recycle trigger.
///
/// The editor-IPC write path is "wedged" when a *nominally-active* JB listener has
/// refused a bounded number of consecutive writes (`send_failed`/`no_ack`/
/// `retry_without_disk_write` ack timeouts) without ever proving delivery — exactly
/// the `consecutive_timeouts` the `#fcc0e` de-wedge circuit breaker latches
/// `degraded` on. A failure against a listener that is NOT nominally active is a
/// fail-closed missing-listener condition, not a wedged active listener, so it
/// never trips this classifier. Pure so the derivation is unit-testable without
/// a live socket.
pub fn write_wedged_from_ipc_failures(
    consecutive_failures: u64,
    listener_nominally_active: bool,
    threshold: u64,
) -> bool {
    listener_nominally_active && consecutive_failures >= threshold
}

/// `#supselfheal` Phase 2 — read the persisted editor-IPC wedge fact for `file` so
/// the route-owned supervisor idle watch can feed `write_wedged` into
/// `supervisor_recycle_action`. Returns `true` once the de-wedge circuit breaker
/// has latched `degraded` for the current session (the converge closeout path's
/// repeated refusals against a nominally-active listener). This is the wedge → owner
/// "request a recycle" channel: the converge process persists the latch, the
/// supervisor reads it here and combines it with its own staleness probe. The
/// converge side self-heals the marker the moment the socket recovers
/// (`ipc_direct_disk_degraded` → `listener_ack_recovered`), so a read of the raw
/// latch is intentional — the supervisor must not run its own socket probe. Best
/// effort: a missing/unreadable marker is "not wedged".
pub(crate) fn editor_ipc_write_wedged(project_root: &Path, file: &Path) -> bool {
    ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// `#supselfheal` Phase 2 — log that a wedged editor-IPC write is now requesting a
/// supervisor recycle through the policy owner, instead of the converge path
/// silently looping refusals. Emitted once when the de-wedge latch first trips so
/// the wedge → recycle escalation is attributable in `ops.log`.
pub(crate) fn log_write_wedge_requests_supervisor_recycle(file: &Path, source: &str) {
    crate::ops_log::log_op(
        file,
        &format!(
            "write_wedged_supervisor_recycle_requested file={} source={} action=request_recycle_through_owner reason=repeated_ack_timeout_active_listener",
            file.display(),
            source
        ),
    );
}

/// The agent's response component in template mode: the single AST node the agent
/// authors during a response cycle. Every OTHER component — the managed
/// queue/backlog/review/status AND any component a plugin defines — is owned by
/// the live editor buffer during a `live_prompt_drift` recovery. Keying the
/// reconciliation off this one agent-authored node (instead of enumerating
/// editor-owned component names) keeps it open to arbitrary / plugin-defined
/// components with no hardcoded allowlist.
const AGENT_RESPONSE_COMPONENT: &str = "exchange";

/// Blank the *content* (not the markers) of every component whose name is NOT in
/// `keep`, so two documents that differ only inside those components compare
/// equal. The kept components' content, the non-component regions (preamble,
/// frontmatter, interstitial text), and all component markers are preserved for
/// comparison — only the unkept components' bodies are cleared. Returns `None` if
/// the document does not parse. Spans are cleared from the end backwards so
/// earlier byte offsets stay valid. Component-name-agnostic by construction: it
/// keys off the AST `component::parse` structure, so arbitrary / plugin-defined
/// components are handled without enumeration.
fn blank_components_except(doc: &str, keep: &[&str]) -> Option<String> {
    let comps = crate::component::parse(doc).ok()?;
    let mut spans: Vec<(usize, usize)> = comps
        .iter()
        .filter(|c| !keep.contains(&c.name.as_str()))
        .map(|c| (c.open_end, c.close_start))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, "");
        }
    }
    Some(out)
}

/// `#qpcwcmerge` — true when the editor-converged `recovered` buffer is safe to
/// commit even though it is not byte-identical to `snapshot` (`content_ours`),
/// because every divergence lives INSIDE a component other than the agent's
/// response component ([`AGENT_RESPONSE_COMPONENT`]). Those other components — the
/// managed queue/backlog/review/status AND any plugin-defined component — are
/// owned by the live editor buffer (editor-wins, `#queue-user-edit-overwrite`);
/// the agent only authored the response. The response component, the document's
/// non-component regions (preamble/frontmatter/interstitial), and the component
/// STRUCTURE (a component added or removed shifts the blanked markers and so fails
/// closed) must all match (normalized). That proves the response landed and no
/// churn leaked outside the editor-owned components, so committing `recovered`
/// makes HEAD equal the editor buffer and eliminates the recurring `#pcwc`
/// post-commit worktree drift — instead of falling back to the `content_ours` disk
/// write that drops the editor's components. Conservative: any out-of-response
/// divergence, a structural change, or a parse failure returns false (block).
///
/// This is AST-structure-driven and component-name-agnostic: it never enumerates
/// the editor-owned component names, so a plugin that defines a new component is
/// reconciled the same way the built-in queue is.
fn convergence_recovered_editor_wins_outside_response(recovered: &str, snapshot: &str) -> bool {
    let (Some(rec_blanked), Some(snap_blanked)) = (
        blank_components_except(recovered, &[AGENT_RESPONSE_COMPONENT]),
        blank_components_except(snapshot, &[AGENT_RESPONSE_COMPONENT]),
    ) else {
        return false;
    };
    let norm = |text: &str| crate::git::normalize_transient_agent_doc_markers(text);
    // Require an actual out-of-response divergence (otherwise the strict
    // whole-document equality check already accepted it; this branch only handles
    // the editor-owned-component mismatch case).
    norm(&rec_blanked) == norm(&snap_blanked) && norm(recovered) != norm(snapshot)
}

/// `#pcwcwarn` — reconcile the agent-owned `exchange` component of a carry-forward
/// superset working tree back to HEAD when the working tree's `exchange` is HEAD's
/// PLUS only stale leftover blockquote lines. Returns the reconciled document, or
/// `None` to leave the working tree alone.
///
/// The post-commit working tree reaches this path only when it lost no committed
/// content (it is a superset of HEAD — the legitimate carry-forward invariant). The
/// `#pcwcwarn` failure mode is a stale live editor buffer that retains a prior
/// cycle's `> **Queue prompt:**`-style blockquote INSIDE the `exchange` response
/// component: `flush_editor_buffer_to_clear_drift` persists the stale buffer, so
/// the worktree re-drifts every cycle and needs a manual `git checkout HEAD`. The
/// agent owns `exchange`, so HEAD is authoritative there; this splices HEAD's
/// `exchange` body into the working tree's `exchange` span, dropping the stale
/// blockquote while preserving every editor-owned component (queue/backlog/… and
/// any plugin-defined component) exactly as the working tree has it. It is the
/// per-component INVERSE of `#qpcwcmerge`
/// (`convergence_recovered_editor_wins_outside_response`): there the editor wins
/// OUTSIDE the response; here HEAD wins INSIDE the response.
///
/// Deliberately NARROW so it never reconciles legitimate exchange carry-forward
/// (a compacted `### Session Summary`, an in-progress scratch comment, a typed
/// follow-up). Fails closed (`None`) unless ALL hold:
/// - both documents parse and have an `exchange` component,
/// - the `exchange` bodies differ (normalized) — otherwise the flush path owns it,
/// - every non-blank HEAD `exchange` line is present in the working `exchange`
///   (HEAD fully contained — the response lost nothing),
/// - every working-only `exchange` line (absent from HEAD) is a blockquote (`>`)
///   line — the stale-quote class; any other novel content (summary heading,
///   archive bullet, scratch comment, prose) means real carry-forward, so preserve,
/// - no working-only line is a user `PromptTarget` (defence in depth).
pub fn reconcile_postcommit_exchange_to_head(working: &str, head: &str) -> Option<String> {
    let working_comps = crate::component::parse(working).ok()?;
    let head_comps = crate::component::parse(head).ok()?;
    let head_exchange = head_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT)?;
    let working_exchange = working_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT)?;
    let head_body = head_exchange.content(head);
    let working_body = working_exchange.content(working);
    let norm = |t: &str| crate::git::normalize_transient_agent_doc_markers(t);
    let head_norm = norm(head_body);
    let working_norm = norm(working_body);
    if head_norm == working_norm {
        return None;
    }
    // HEAD must be fully contained in the working exchange (no committed response
    // content dropped from the tree) — line-set containment over the normalized
    // bodies so `(HEAD)`/boundary markers do not perturb the comparison.
    let head_lines: HashSet<&str> = head_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let working_lines: HashSet<&str> = working_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !head_lines.iter().all(|l| working_lines.contains(l)) {
        return None;
    }
    // Every working-only line must be a stale blockquote — the narrow class this
    // repair targets. Any other novel line is legitimate exchange carry-forward.
    let working_only: Vec<&str> = working_lines.difference(&head_lines).copied().collect();
    if working_only.is_empty() || !working_only.iter().all(|l| l.starts_with('>')) {
        return None;
    }
    // Defence in depth: never drop a genuine new user prompt typed into the tail.
    let exchange_changes = prompt_bearing_user_changes_between(head_body, working_body);
    if exchange_changes
        .iter()
        .any(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
    {
        return None;
    }
    let start = working_exchange.open_end;
    let end = working_exchange.close_start;
    if !(start <= end
        && end <= working.len()
        && working.is_char_boundary(start)
        && working.is_char_boundary(end))
    {
        return None;
    }
    let mut out = working.to_string();
    out.replace_range(start..end, head_body);
    Some(out)
}

/// `#ipctruncrecover` — true when a flushed editor buffer (`flushed`) preserved every
/// committed `exchange` (response) line from `head`. The agent owns `exchange`, so a
/// trustworthy editor buffer may add editor-owned content (queue/backlog edits) and may
/// drop nothing HEAD committed in the response component. Used by the preflight
/// editor-buffer-as-truth recovery to refuse trusting an editor buffer that *itself* lost
/// the committed response (e.g. a doubly-truncated buffer) — that case falls through to the
/// safe bail instead of auto-reconciling to a response-less document. Line-set containment
/// over normalized bodies so `(HEAD)` / boundary markers do not perturb the comparison.
/// Conservatively returns `false` when either side fails to parse or lacks an `exchange`
/// component, so an unparseable flush is never silently trusted.
pub fn editor_buffer_preserved_head_exchange(flushed: &str, head: &str) -> bool {
    let (Ok(flushed_comps), Ok(head_comps)) = (
        crate::component::parse(flushed),
        crate::component::parse(head),
    ) else {
        return false;
    };
    let (Some(head_exchange), Some(flushed_exchange)) = (
        head_comps
            .iter()
            .find(|c| c.name == AGENT_RESPONSE_COMPONENT),
        flushed_comps
            .iter()
            .find(|c| c.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return false;
    };
    let norm = |t: &str| crate::git::normalize_transient_agent_doc_markers(t);
    let head_norm = norm(head_exchange.content(head));
    let flushed_norm = norm(flushed_exchange.content(flushed));
    let flushed_lines: HashSet<&str> = flushed_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    head_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .all(|l| flushed_lines.contains(l))
}

/// `#pzjy` — repair a stale live editor buffer that resurrected queue prompts
/// HEAD already committed as completed. This is deliberately directional:
/// HEAD-completed + working-active is repaired, while HEAD-active +
/// working-completed remains editor-owned (`#qpcwcmerge`).
pub fn reconcile_postcommit_queue_strikes_to_head(working: &str, head: &str) -> Option<String> {
    let working_comps = crate::component::parse(working).ok()?;
    let head_comps = crate::component::parse(head).ok()?;
    let working_queue = working_comps.iter().find(|c| c.name == "queue")?;
    let head_queue = head_comps.iter().find(|c| c.name == "queue")?;
    let working_body = working_queue.content(working);
    let head_body = head_queue.content(head);
    let working_entries = crate::queue::parse(working_body).ok()?;
    let head_entries = crate::queue::parse(head_body).ok()?;

    let prompt_key = |text: &str| text.trim().to_string();
    let mut head_active_counts: HashMap<String, usize> = HashMap::new();
    let mut head_completed_counts: HashMap<String, usize> = HashMap::new();
    for entry in &head_entries {
        match entry {
            crate::queue::QueueEntry::Prompt(prompt) => {
                *head_active_counts
                    .entry(prompt_key(&prompt.text))
                    .or_insert(0) += 1;
            }
            crate::queue::QueueEntry::Completed(prompt) => {
                *head_completed_counts
                    .entry(prompt_key(&prompt.text))
                    .or_insert(0) += 1;
            }
            _ => {}
        }
    }
    if head_completed_counts.is_empty() {
        return None;
    }

    let mut seen_working_active: HashMap<String, usize> = HashMap::new();
    let mut restored = false;
    let reconciled_entries: Vec<crate::queue::QueueEntry> = working_entries
        .into_iter()
        .map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => {
                let key = prompt_key(&prompt.text);
                let seen = seen_working_active.entry(key.clone()).or_insert(0);
                *seen += 1;
                let allowed_active = head_active_counts.get(&key).copied().unwrap_or(0);
                let head_completed = head_completed_counts.get(&key).copied().unwrap_or(0);
                if *seen > allowed_active && head_completed > 0 {
                    restored = true;
                    crate::queue::QueueEntry::Completed(prompt)
                } else {
                    crate::queue::QueueEntry::Prompt(prompt)
                }
            }
            other => other,
        })
        .collect();
    if !restored {
        return None;
    }

    let new_body = crate::queue::render(&reconciled_entries);
    if new_body == working_body {
        return None;
    }
    Some(working_queue.replace_content(working, &new_body))
}

pub(crate) fn try_editor_converge_live_prompt_drift(
    file: &Path,
    project_root: &Path,
    target: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let patches = live_prompt_drift_response_patches(file_content, target)?;
    let frontmatter = None;
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
            "[jbstalecache] editor_convergence_attempt file={} patch_id={} patches={} frontmatter={} target_hash={}",
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
            crate::ops_log::content_hash(target)
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
                == crate::git::normalize_transient_agent_doc_markers(target)
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
            } else if convergence_recovered_editor_wins_outside_response(&recovered, target) {
                // `#qpcwcmerge`: the editor buffer diverges from `content_ours` only
                // INSIDE components other than the agent's response component — its
                // live queue + same-cycle auto-strikes, or any plugin-defined
                // component — while the response and everything else match (the
                // response landed). Commit the editor buffer (editor-wins outside the
                // response) so HEAD equals the editor and the recurring post-commit
                // worktree drift (`#pcwc`) is eliminated, rather than blocking and
                // falling back to the `content_ours` disk write that drops the
                // editor's components.
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_response #qpcwcmerge",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(Some(recovered))
            } else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_ack_mismatch file={} patch_id={} recovered_len={} target_len={} action=block_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
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

fn live_prompt_drift_response_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut patches = live_prompt_drift_convergence_patches(file_content, snapshot)?;
    // `live_prompt_drift` recovery is only authorized to materialize the agent's
    // response node. Non-response components and frontmatter belong to the live
    // editor/operator in this recovery path; if they differ, the containment gate
    // above has already failed closed instead of sending a patch that could reset
    // operator text.
    patches.retain(|patch| {
        patch.get("component").and_then(|value| value.as_str()) == Some(AGENT_RESPONSE_COMPONENT)
    });
    Ok(patches)
}

/// `#w42v`: converge a compacted document through the editor instead of a direct
/// disk write that diverges from the open JB buffer (`File Cache Conflict`).
///
/// Mirrors the `#q7jm` live_prompt_drift convergence: when a JB IPC listener is
/// active, send component `op:replace` patches for the changed components
/// (`exchange`, etc.) and verify the editor's ack content matches the compacted
/// target. Returns `Ok(true)` when converged via editor IPC (the caller skips
/// the disk write) and `Err` when editor convergence is unavailable or unproven.
/// The error is intentional: direct disk writes behind or around editor
/// convergence are the File Cache Conflict source this guard prevents.
/// `#fcc0`/`#w42v`: converge a full-document write through the editor IPC when a
/// JB listener is active, returning `true` when the editor buffer has been
/// converged to `target` (no disk write needed).
///
/// When a listener is active this computes the component-scoped delta between
/// `current_content` and `target` and applies it via `op:replace` patches through
/// the Document API, so the open buffer never diverges from disk and no
/// `File Cache Conflict` dialog fires. `source` labels the `ops.log` writeback
/// transport lines (`<source>_writeback ... transport=editor_ipc|blocked`)
/// so each write site is attributable; see [`converge_document_or_disk`] for
/// the shared converge-or-disk wrapper every document-mutating write routes
/// through.
/// `#6b5h`: at a proven-no-delivery editor-converge refusal point, fail closed.
///
/// The realtime cutover removes the old synchronous "send patch, wait, then
/// disk-fallback" branch: once a live editor owner or sidecar is observed,
/// missing or untrusted ACK proof marks editor convergence required. A direct
/// disk write is allowed only in detached realtime, after the current visible
/// file is rechecked as the merge input.
fn refuse_unproven_editor_delivery(
    file: &Path,
    source: &str,
    reason: &str,
    patch_id: Option<&str>,
) -> Result<bool> {
    let editor_endpoint = if crate::merge_control_state_machine::disk_write_permitted_for_file(
        &file.to_string_lossy(),
    ) && !live_editor_sidecar_present(file)
    {
        "absent"
    } else {
        "live"
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=blocked reason={reason} editor_endpoint={} action=editor_convergence_required",
            file.display(),
            editor_endpoint
        ),
    );
    let detail = format!("editor_endpoint={editor_endpoint}");
    if let Err(err) = crate::cycle_state::record_editor_convergence_required(
        file,
        source,
        reason,
        patch_id,
        Some(&detail),
    ) {
        eprintln!(
            "[write] WARNING: failed to record editor-convergence blocked closeout for {}: {err}",
            file.display()
        );
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} while editor convergence is unproven (reason={reason}, editor_endpoint={editor_endpoint})",
        file.display()
    );
}

fn live_editor_sidecar_present(file: &Path) -> bool {
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    crate::debounce::live_buffer_snapshots(&indicator_path)
        .iter()
        .any(crate::debounce::live_buffer_snapshot_editor_is_live)
}

fn try_detached_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    if !crate::merge_control_state_machine::disk_write_permitted_for_file(&file.to_string_lossy())
        || live_editor_sidecar_present(file)
    {
        return Ok(false);
    }

    guard_visible_write_idle_and_current(file, source, current)?;
    atomic_write(file, target).with_context(|| {
        format!(
            "{source}: failed detached disk write for {}",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=disk_detached reason={} len={} hash={}",
            file.display(),
            reason,
            target.len(),
            crate::ops_log::content_hash(target)
        ),
    );
    Ok(true)
}

fn blank_components_named(doc: &str, names: &[&str]) -> Option<String> {
    let comps = crate::component::parse(doc).ok()?;
    let mut spans: Vec<(usize, usize)> = comps
        .iter()
        .filter(|c| names.contains(&c.name.as_str()))
        .map(|c| (c.open_end, c.close_start))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, "");
        }
    }
    Some(out)
}

fn stale_queue_prompt_exchange_artifact(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('>') || trimmed == "❯ >"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckMismatchRecovery {
    RevertUntrustedAckToCurrent,
    ReplayMissingAgentResponseToTarget,
}

fn missing_agent_response_block<'a>(target_body: &'a str, recovered_body: &str) -> Option<&'a str> {
    if target_body.len() <= recovered_body.len() {
        return None;
    }
    let missing = if let Some(missing) = target_body.strip_prefix(recovered_body) {
        missing
    } else if let Some(missing) = target_body.strip_suffix(recovered_body) {
        missing
    } else {
        let start = target_body.find(recovered_body)?;
        let end = start + recovered_body.len();
        let before = &target_body[..start];
        let after = &target_body[end..];
        if before.trim().is_empty() {
            after
        } else if after.trim().is_empty() {
            before
        } else {
            return None;
        }
    };
    let trimmed = missing.trim_start();
    if trimmed.starts_with("### Re:") || trimmed.contains("\n### Re:") {
        Some(missing)
    } else {
        None
    }
}

fn classify_ack_mismatch_recovery(target: &str, recovered: &str) -> Option<AckMismatchRecovery> {
    let (Some(target_without_exchange), Some(recovered_without_exchange)) = (
        blank_components_named(target, &[AGENT_RESPONSE_COMPONENT]),
        blank_components_named(recovered, &[AGENT_RESPONSE_COMPONENT]),
    ) else {
        return None;
    };
    let norm = |text: &str| crate::git::normalize_transient_agent_doc_markers(text);
    if norm(&target_without_exchange) != norm(&recovered_without_exchange) {
        return None;
    }

    let (Ok(target_comps), Ok(recovered_comps)) = (
        crate::component::parse(target),
        crate::component::parse(recovered),
    ) else {
        return None;
    };
    let target_exchange = target_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let recovered_exchange = recovered_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let (Some(target_exchange), Some(recovered_exchange)) = (target_exchange, recovered_exchange)
    else {
        return None;
    };
    let target_body = norm(target_exchange.content(target));
    let recovered_body = norm(recovered_exchange.content(recovered));
    if target_body == recovered_body {
        return None;
    }
    if recovered_body.len() < target_body.len()
        && missing_agent_response_block(&target_body, &recovered_body).is_some()
    {
        return Some(AckMismatchRecovery::ReplayMissingAgentResponseToTarget);
    }
    let target_lines: HashSet<&str> = target_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let recovered_lines: HashSet<&str> = recovered_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !target_lines
        .iter()
        .all(|line| recovered_lines.contains(line))
    {
        return None;
    }
    let recovered_only: Vec<&str> = recovered_lines.difference(&target_lines).copied().collect();
    if !recovered_only.is_empty()
        && recovered_only
            .iter()
            .all(|line| stale_queue_prompt_exchange_artifact(line))
        && recovered_only
            .iter()
            .any(|line| line.trim().starts_with("> **Queue prompt:**"))
    {
        return Some(AckMismatchRecovery::RevertUntrustedAckToCurrent);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckMismatchRefreshOutcome {
    NoRecovery,
    RevertedToCurrent,
    ReplayedTarget,
}

fn refresh_editor_after_ack_mismatch(
    file: &Path,
    project_root: &Path,
    canonical: &Path,
    target: &str,
    recovered: &str,
    current_content: &str,
    source: &str,
) -> AckMismatchRefreshOutcome {
    let stale_hash = crate::ops_log::content_hash(recovered);
    let Some(recovery) = classify_ack_mismatch_recovery(target, recovered) else {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason=untrusted_ack_content_contains_user_drift action=leave_editor_owned_ack_content stale_len={} stale_hash={}",
                file.display(),
                recovered.len(),
                &stale_hash[..stale_hash.len().min(12)]
            ),
        );
        return AckMismatchRefreshOutcome::NoRecovery;
    };
    let (refresh_content, action, success_outcome) = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => (
            current_content,
            "revert_untrusted_ack_content",
            AckMismatchRefreshOutcome::RevertedToCurrent,
        ),
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => (
            target,
            "replay_missing_agent_response",
            AckMismatchRefreshOutcome::ReplayedTarget,
        ),
    };
    let target_hash = crate::ops_log::content_hash(refresh_content);
    let failure_action = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => {
            "left_untrusted_ack_content_editor_owned"
        }
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "left_missing_agent_response_editor_owned"
        }
    };
    let failure_reason = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => "safe_stale_prompt_refresh_failed",
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "safe_missing_agent_response_refresh_failed"
        }
    };
    match crate::ipc_socket::send_refresh_content(
        project_root,
        &canonical.to_string_lossy(),
        refresh_content,
        &stale_hash,
        recovered.len(),
    ) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=editor_ipc action={} stale_len={} stale_hash={} target_len={} target_hash={}",
                    file.display(),
                    action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)],
                    refresh_content.len(),
                    &target_hash[..target_hash.len().min(12)]
                ),
            );
            success_outcome
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} no_ack=true action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} send_failed=true error={} action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    err,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
    }
}

pub(crate) fn live_buffer_delivery_missing_operator_text_authority_after_refresh(
    file: &Path,
    content: &str,
    source: &str,
) -> Option<crate::debounce::LiveBufferSnapshot> {
    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let indicator_path = canonical_file.to_string_lossy().to_string();
    let missing = crate::debounce::live_buffer_delivery_missing_operator_text_authority(
        &indicator_path,
        content,
    )?;
    let project_root = resolve_ipc_project_root_pub(&canonical_file);
    if !crate::ipc_socket::is_listener_active(&project_root) {
        return match crate::ipc_socket::send_publish_live_buffer_file_signal(
            &project_root,
            &indicator_path,
        ) {
            Ok(true) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=file_signal action=publish_live_buffer",
                        file.display()
                    ),
                );
                wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
            }
            Ok(false) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_unavailable action=editor_reload_required",
                        file.display()
                    ),
                );
                Some(missing)
            }
            Err(err) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_failed error={} action=editor_reload_required",
                        file.display(),
                        err
                    ),
                );
                Some(missing)
            }
        };
    }

    match crate::ipc_socket::send_publish_live_buffer(&project_root, &indicator_path) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=editor_ipc action=publish_live_buffer",
                    file.display()
                ),
            );
            wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed action=editor_reload_required",
                    file.display()
                ),
            );
            Some(missing)
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed error={} action=editor_reload_required",
                    file.display(),
                    err
                ),
            );
            Some(missing)
        }
    }
}

fn wait_for_operator_text_authority_refresh(
    indicator_path: &str,
    content: &str,
    mut latest_missing: crate::debounce::LiveBufferSnapshot,
) -> Option<crate::debounce::LiveBufferSnapshot> {
    for _ in 0..20 {
        match crate::debounce::live_buffer_delivery_missing_operator_text_authority(
            indicator_path,
            content,
        ) {
            Some(still_missing) => {
                latest_missing = still_missing;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            None => return None,
        }
    }
    Some(latest_missing)
}

pub fn try_editor_converge(
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    let canonical_file = file
        .canonicalize()
        .with_context(|| format!("{source}: failed to resolve {}", file.display()))?;
    let project_root = resolve_ipc_project_root_pub(&canonical_file);
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
    if let Some(snapshot) = live_buffer_delivery_missing_operator_text_authority_after_refresh(
        &canonical_file,
        current_content,
        source,
    ) {
        let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=editor_capability_missing capability={} editor_id={} live_len={} live_hash={} action=editor_reload_required",
                file.display(),
                crate::debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                editor_id,
                snapshot.len,
                snapshot.hash
            ),
        );
        anyhow::bail!(
            "{source}: refused editor convergence for {} because live editor buffer {} lacks required capability {}",
            file.display(),
            editor_id,
            crate::debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
        );
    }
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            log_ipc_dewedge_prefer_file_ipc(file, source);
            let canonical = file.canonicalize()?;
            let patch_id = uuid::Uuid::new_v4().to_string();
            let Some(payload) =
                editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
            else {
                if try_detached_disk_write(
                    file,
                    current_content,
                    target,
                    source,
                    "listener_degraded_no_component_delta",
                )? {
                    return Ok(true);
                }
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
            if try_detached_disk_write(
                file,
                current_content,
                target,
                source,
                "listener_degraded_editor_detached",
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
        if try_detached_disk_write(file, current_content, target, source, "no_listener")? {
            return Ok(true);
        }
        return refuse_unproven_editor_delivery(file, source, "no_listener", None);
    }

    let canonical = canonical_file;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let Some(payload) =
        editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
    else {
        if try_detached_disk_write(file, current_content, target, source, "no_component_delta")? {
            return Ok(true);
        }
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
                // `#6b5h`: ack received but no content sidecar proves application —
                // fail closed instead of routing a sync wait failure to disk.
                return refuse_unproven_editor_delivery(
                    file,
                    source,
                    "no_ack_content",
                    Some(&patch_id),
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
                let recovery = refresh_editor_after_ack_mismatch(
                    file,
                    &project_root,
                    &canonical,
                    target,
                    &recovered,
                    current_content,
                    source,
                );
                if recovery == AckMismatchRefreshOutcome::ReplayedTarget {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc recovery=ack_mismatch_replayed_target",
                            file.display(),
                            patch_id,
                            recovered.len(),
                            target.len()
                        ),
                    );
                    if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                        eprintln!(
                            "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                        );
                    }
                    return Ok(true);
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=ack_mismatch recovered_len={} target_len={} action=editor_convergence_required",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                // The ACK came back but content drifted. This is unproven editor
                // convergence, not authorization to write through disk.
                refuse_unproven_editor_delivery(file, source, "ack_mismatch", Some(&patch_id))
            }
        }
        Ok(None) => {
            if try_detached_disk_write(file, current_content, target, source, "no_ack")? {
                return Ok(true);
            }
            // Missing ACK against a live editor marks the editor path stale; it
            // must not trigger a direct disk fallback.
            refuse_unproven_editor_delivery(file, source, "no_ack", Some(&patch_id))
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} reason=send_failed error={} note=converge_send_error",
                    file.display(),
                    err
                ),
            );
            // A terminal `status:error` means the socket listener received the
            // patch but its socket-side apply path rejected it. That is still not
            // permission to raw-write the file, but the plugin-owned file-IPC
            // watcher may be able to apply the exact same patch and prove the
            // resulting buffer through ack-content in this same cycle.
            if is_socket_status_error(&err)
                && try_editor_converge_file_ipc(
                    file,
                    &project_root,
                    &payload,
                    &patch_id,
                    target,
                    source,
                    "socket_status_error",
                )?
            {
                return Ok(true);
            }
            // `#fcc0e`: feed the de-wedge circuit breaker — a socket ack timeout
            // here counts toward the latch so a repeatedly-wedged listener trips
            // degraded and subsequent converges skip the doomed socket up front.
            // (Recovery targets a live editor; an editor-less session disk-falls
            // back below, but recording the socket failure is still harmless.)
            if is_socket_ack_timeout_error(&err) {
                match record_ipc_socket_ack_timeout(&project_root, file, Some(&patch_id), source) {
                    Ok(true) => {
                        eprintln!(
                            "[write] IPC listener degraded for {} after repeated {source} ack timeouts",
                            file.display()
                        );
                        // `#supselfheal` Phase 2: the latch just tripped — the editor
                        // write is wedged against a nominally-active listener. Record
                        // that the wedge is now a supervisor-recycle request (the
                        // route-owned supervisor reads the latched marker via
                        // `editor_ipc_write_wedged` and recycles a stale binary)
                        // instead of looping silent refusals.
                        log_write_wedge_requests_supervisor_recycle(file, source);
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[write] WARNING: {source} converge ack-timeout record failed (non-fatal): {e}"
                    ),
                }
            }
            if try_detached_disk_write(file, current_content, target, source, "send_failed")? {
                return Ok(true);
            }
            // Send failure against a live editor marks the editor path stale; it
            // must not trigger a direct disk fallback.
            refuse_unproven_editor_delivery(file, source, "send_failed", Some(&patch_id))
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
/// dialog). If editor convergence is unavailable or unproven, the write fails
/// closed instead of falling back to disk. `current` is the expected current
/// document content (held under the caller's doc lock) and drives the editor
/// delta.
pub fn converge_document_or_disk(
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

/// `#fcc0`: converge-only gate for the component-mutating CLI write
/// sites that historically wrote straight to disk with a bare `std::fs::write`
/// (the `agent:pending` / `agent:review` operator ops, `dedupe`, preflight
/// `run_pending_maintenance`, the `agent_doc_pipeline:` frontmatter mirror). When
/// a JB editor listener is active it converges `target` through the editor IPC
/// (component/frontmatter `op:replace` — no `File Cache Conflict` dialog). If
/// editor convergence is unavailable or unproven, the write fails closed instead
/// of falling back to the historical plain disk write. `current` is the expected
/// current on-disk content the editor delta is computed against; `source` labels
/// the `ops.log` `<source>_writeback` line.
pub fn converge_or_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub(crate) fn live_prompt_drift_convergence_patches(
    file_content: &str,
    target: &str,
) -> Result<Vec<serde_json::Value>> {
    let current_components = component::parse(file_content)
        .with_context(|| "failed to parse current document for editor convergence")?;
    let target_components = component::parse(target)
        .with_context(|| "failed to parse target document for editor convergence")?;
    let current_by_name: HashMap<&str, &component::Component> = current_components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect();
    let mut patches = Vec::new();
    for target_component in &target_components {
        let Some(current_component) = current_by_name.get(target_component.name.as_str()) else {
            continue;
        };
        let current_body = current_component.content(file_content);
        let target_body = target_component.content(target);
        if crate::git::normalize_transient_agent_doc_markers(current_body)
            == crate::git::normalize_transient_agent_doc_markers(target_body)
        {
            continue;
        }
        patches.push(serde_json::json!({
            "component": target_component.name,
            "content": target_body,
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

    fn doc_with_queue_and_exchange(queue_body: &str, response: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    fn queue_node_key_for_id(doc: &str, id: &str) -> String {
        agent_doc_markdown_ast::mutations::all_item_nodes(doc)
            .into_iter()
            .find(|node| node.component == "queue" && node.item.id == id)
            .map(|node| node.node_key)
            .unwrap_or_else(|| panic!("missing queue node id {id}"))
    }

    fn start_ack_mismatch_then_refresh_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
        let listener_root = project_root.to_path_buf();
        std::thread::spawn(move || {
            let root_clone = listener_root.clone();
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let msg_type = v.get("type").and_then(|value| value.as_str()).unwrap_or("");
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let content = if msg_type == "refresh_content" {
                    v.get("content")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    ack_content.clone()
                };
                if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(file_path, &content);
                }
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    #[test]
    fn qpcwcmerge_accepts_editor_buffer_when_only_queue_differs() {
        // #qpcwcmerge: the editor buffer (recovered) has a struck queue head the
        // operator's live state owns; content_ours (snapshot) still has it active.
        // The exchange (response) is identical. The editor buffer must be accepted
        // (editor-wins outside the response) so HEAD == editor and no post-commit drift.
        let snapshot =
            doc_with_queue_and_exchange("- a free-text head\n", "### Re: topic\n\nAnswered.");
        let recovered =
            doc_with_queue_and_exchange("- ~~a free-text head~~\n", "### Re: topic\n\nAnswered.");
        assert!(
            convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "queue-only divergence with matching response must be accepted"
        );
    }

    #[test]
    fn postcommit_queue_reconcile_restores_answered_pinned_and_free_text_heads() {
        let head = doc_with_queue_and_exchange(
            "- ~~:pushpin: do [#pzjy]~~\n- ~~plain queued report~~\n",
            "### Re: topic\n\nAnswered.",
        );
        let working = doc_with_queue_and_exchange(
            "- :pushpin: do [#pzjy]\n- plain queued report\n- do [#new]\n",
            "### Re: topic\n\nAnswered.",
        );

        let reconciled =
            reconcile_postcommit_queue_strikes_to_head(&working, &head).expect("queue repair");
        assert!(
            reconciled.contains("- ~~:pushpin: do [#pzjy]~~\n"),
            "pinned completed prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- ~~plain queued report~~\n"),
            "answered free-text prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- do [#new]\n"),
            "unrelated queue additions must remain live:\n{reconciled}"
        );
    }

    #[test]
    fn postcommit_queue_reconcile_does_not_unstrike_editor_completed_head() {
        let head =
            doc_with_queue_and_exchange("- a free-text head\n", "### Re: topic\n\nAnswered.");
        let working =
            doc_with_queue_and_exchange("- ~~a free-text head~~\n", "### Re: topic\n\nAnswered.");

        assert!(
            reconcile_postcommit_queue_strikes_to_head(&working, &head).is_none(),
            "editor-owned queue strike must remain editor-wins"
        );
    }

    #[test]
    fn qpcwcmerge_accepts_editor_buffer_for_arbitrary_plugin_component() {
        // AST-driven generality (operator directive): a component a PLUGIN defines —
        // not in any hardcoded allowlist — must be editor-authoritative exactly like
        // the built-in queue. Here a `agent:pluginpanel` component diverges while the
        // response matches → accept the editor buffer.
        let doc = |panel: &str| {
            format!(
                "---\nq: 1\n---\n\n<!-- agent:exchange -->\n### Re: x\n\nbody\n<!-- /agent:exchange -->\n\n<!-- agent:pluginpanel -->\n{panel}\n<!-- /agent:pluginpanel -->\n"
            )
        };
        let snapshot = doc("plugin state v1");
        let recovered = doc("plugin state v2 (editor-updated)");
        assert!(
            convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a plugin-defined component must be editor-authoritative without an allowlist"
        );
    }

    #[test]
    fn qpcwcmerge_rejects_when_response_differs() {
        // The response itself diverges — NOT safe to accept the editor buffer; the
        // strict content_ours path must own the response component. Fail closed.
        let snapshot =
            doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered correctly.");
        let recovered =
            doc_with_queue_and_exchange("- ~~head~~\n", "### Re: topic\n\nAnswered DIFFERENTLY.");
        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a response (exchange) divergence must fail closed"
        );
    }

    #[test]
    fn qpcwcmerge_rejects_when_identical() {
        // No out-of-response divergence at all → the strict equality check already
        // accepted it; this branch must not fire (requires a real mismatch).
        let doc = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");
        assert!(
            !convergence_recovered_editor_wins_outside_response(&doc, &doc),
            "identical docs are handled by the strict path, not this branch"
        );
    }

    #[test]
    fn qpcwcmerge_rejects_when_non_component_region_differs() {
        // A divergence OUTSIDE any component (here: an interstitial heading) must
        // fail closed even if the queue also differs — injected churn outside the
        // editor-owned components is never silently accepted.
        let snapshot = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");
        let mut recovered =
            doc_with_queue_and_exchange("- ~~head~~\n", "### Re: topic\n\nAnswered.");
        recovered = recovered.replace("## Queue", "## Queue (tampered interstitial)");
        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a non-component-region divergence must fail closed"
        );
    }

    #[test]
    fn qpcwcmerge_rejects_structural_component_add() {
        // The editor added a whole component content_ours lacks → structural change.
        // Fail closed (conservative): the strict path owns structural divergence.
        let snapshot = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");
        let recovered = format!(
            "{}\n<!-- agent:extra -->\nnew\n<!-- /agent:extra -->\n",
            doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.").trim_end()
        );
        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a structural component add must fail closed"
        );
    }

    #[test]
    fn pcwcwarn_reconciles_stale_exchange_blockquote_preserving_queue() {
        // #pcwcwarn: the working tree (stale editor buffer) carries a prior cycle's
        // `> **Queue prompt:**` blockquote INSIDE the exchange that HEAD dropped,
        // AND the operator carried a struck queue head forward. HEAD's exchange is
        // authoritative; the queue is editor-owned. Reconcile must adopt HEAD's
        // exchange while preserving the working tree's queue.
        let head =
            doc_with_queue_and_exchange("- a live head\n", "### Re: topic\n\nAnswered cleanly.");
        let working = doc_with_queue_and_exchange(
            "- ~~a live head~~\n",
            "### Re: topic\n\nAnswered cleanly.\n\n> **Queue prompt:** stale leftover from a prior cycle",
        );
        let reconciled = reconcile_postcommit_exchange_to_head(&working, &head)
            .expect("stale exchange blockquote must reconcile to HEAD");
        assert!(
            !reconciled.contains("stale leftover from a prior cycle"),
            "the stale exchange blockquote must be dropped"
        );
        assert!(
            reconciled.contains("- ~~a live head~~"),
            "the editor-owned queue (struck head) must be preserved"
        );
        // The exchange now matches HEAD; a second pass is a no-op.
        assert!(
            reconcile_postcommit_exchange_to_head(&reconciled, &head).is_none()
                || reconcile_postcommit_exchange_to_head(&reconciled, &head)
                    .as_deref()
                    .map(|d| d == reconciled)
                    .unwrap_or(false),
            "reconcile must converge"
        );
    }

    #[test]
    fn pcwcwarn_returns_none_when_only_queue_differs() {
        // The exchange already matches HEAD; the divergence is purely editor-owned
        // queue carry-forward. Reconcile must NOT fire — the flush path handles it.
        let head = doc_with_queue_and_exchange("- a head\n", "### Re: x\n\nbody");
        let working = doc_with_queue_and_exchange("- ~~a head~~\n", "### Re: x\n\nbody");
        assert!(
            reconcile_postcommit_exchange_to_head(&working, &head).is_none(),
            "queue-only divergence with a matching exchange must not reconcile"
        );
    }

    #[test]
    fn pcwcwarn_fails_closed_on_new_user_prompt_in_exchange() {
        // A genuine post-commit user follow-up typed into the exchange tail must NOT
        // be dropped by adopting HEAD — fail closed and leave it for the next cycle.
        let head = doc_with_queue_and_exchange("- a head\n", "### Re: x\n\nbody");
        let working = doc_with_queue_and_exchange(
            "- a head\n",
            "### Re: x\n\nbody\n\n❯ do [#followup] a new directive",
        );
        assert!(
            reconcile_postcommit_exchange_to_head(&working, &head).is_none(),
            "a new user PromptTarget in the working exchange must fail closed"
        );
    }

    #[test]
    fn blank_components_except_clears_others_keeps_exchange() {
        let doc = doc_with_queue_and_exchange("- some head\n", "### Re: x\n\nbody");
        let blanked = blank_components_except(&doc, &[AGENT_RESPONSE_COMPONENT]).unwrap();
        assert!(
            !blanked.contains("some head"),
            "queue content must be blanked"
        );
        assert!(
            blanked.contains("### Re: x"),
            "response content must be preserved"
        );
        assert!(
            blanked.contains("<!-- agent:queue -->"),
            "queue markers stay"
        );
    }

    #[test]
    fn write_wedged_classifier_trips_only_against_active_listener_at_threshold() {
        // `#supselfheal` Phase 2: the wedge fact trips only when a *nominally
        // active* listener has refused >= threshold consecutive writes. A failure
        // against an inactive listener is a missing-listener block, not a wedge.
        let threshold = IPC_DEWEDGE_TIMEOUT_THRESHOLD;
        // Active listener at/over threshold → wedged.
        assert!(write_wedged_from_ipc_failures(threshold, true, threshold));
        assert!(write_wedged_from_ipc_failures(
            threshold + 1,
            true,
            threshold
        ));
        // Active listener under threshold → not yet wedged (transient lull).
        assert!(!write_wedged_from_ipc_failures(
            threshold - 1,
            true,
            threshold
        ));
        // No listener nominally active → never an active-listener wedge.
        assert!(!write_wedged_from_ipc_failures(
            threshold + 5,
            false,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(0, true, threshold));
    }

    #[test]
    fn editor_ipc_write_wedged_reads_latched_degraded_marker() {
        // `#supselfheal` Phase 2: the supervisor-facing reader returns true once the
        // de-wedge latch has persisted `degraded` for the current session, and false
        // when there is no marker. Drive it through the real persistence path.
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();
        // No marker yet → not wedged.
        assert!(!editor_ipc_write_wedged(project_root, &file));
        // Record ack timeouts up to the latch threshold → degraded persisted.
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "a latched degraded marker should read as a write wedge"
        );
    }

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
    fn live_prompt_drift_response_patches_ignore_operator_owned_components() {
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let generic = live_prompt_drift_convergence_patches(&fragmented, &snapshot).unwrap();
        let generic_components: Vec<&str> = generic
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .collect();
        assert!(
            generic_components.contains(&"exchange") && generic_components.contains(&"backlog"),
            "generic convergence should notice both component deltas: {generic:?}"
        );

        let response_only = live_prompt_drift_response_patches(&fragmented, &snapshot).unwrap();
        assert_eq!(
            response_only.len(),
            1,
            "live drift recovery only owns exchange"
        );
        assert_eq!(response_only[0]["component"], "exchange");
    }

    #[test]
    fn try_compact_editor_converge_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the converger may use a
        // guarded direct disk write. This is not a snapshot fallback.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let current = crate::test_support::drift_baseline();
        let compacted = crate::test_support::drift_content_ours();
        std::fs::write(&doc, &current).unwrap();

        let converged = try_editor_converge(&doc, &compacted, &current, "compact").unwrap();
        assert!(converged, "detached compact should write the target");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            compacted,
            "no-listener compact convergence should write the compacted target"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "no-listener compact must record a detached disk writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener compact must not advertise disk fallback:\n{log}"
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
    fn queue_consume_socket_status_error_falls_back_to_proven_file_ipc() {
        // A live editor socket can accept a patch, emit the early pending ack,
        // then reject the terminal apply (`status:error`) because the editor is
        // busy or the socket-side apply path lost its generation race. That must
        // not authorize a raw disk write, but it should try the plugin-owned
        // file-IPC queue in the same cycle and accept it only with ack-content.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let listener_root = dir.path().to_path_buf();
        let _listener = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                Some(
                    serde_json::json!({
                        "type": "ack",
                        "id": patch_id,
                        "status": "error",
                        "reason": "socket_apply_failed"
                    })
                    .to_string(),
                )
            });
        });
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let watcher_dir = agent_doc_dir.join("patches");
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_doc = doc.clone();
        let watcher_target = target.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| e == "json") {
                        continue;
                    }
                    let payload_text = fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&payload_text).unwrap();
                    let patch_id = payload
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap()
                        .to_string();
                    fs::write(&watcher_doc, &watcher_target).unwrap();
                    fs::write(
                        watcher_ack_dir.join(format!("{patch_id}.md")),
                        &watcher_target,
                    )
                    .unwrap();
                    fs::remove_file(path).unwrap();
                    return true;
                }
            }
            false
        });

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "socket status:error should retry through proven file IPC before failing closed"
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("send_failed")
                && log.contains("IPC ack status error"),
            "socket status error should remain auditable:\n{log}"
        );
        assert!(
            log.contains("queue_consume_file_ipc_convergence_attempt")
                && log.contains("degraded_cause=socket_status_error")
                && log.contains("transport=file_ipc"),
            "socket status error should fall back to proven file IPC:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "socket status-error fallback must not raw-write behind the plugin:\n{log}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
    }

    #[test]
    fn queue_consume_ack_mismatch_refreshes_editor_back_to_preconsume() {
        // `#fcc0-ack-mismatch`: when the editor acks with content that does not
        // match the target, the disk write must still fail closed. The previous
        // behavior left that untrusted ACK content in the live editor buffer, so a
        // later flush could persist a stale queue strike. Refresh it back to the
        // pre-consume document using the ACK content as the stale hash guard.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        let stale_ack = target.replace(
            "<!-- /agent:exchange -->",
            "> **Queue prompt:** stale leftover from failed queue consume\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, stale_ack);
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::plugin_owner::write_plugin_owner_lease_for_test(
            doc.to_str().unwrap(),
            std::process::id(),
        );

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "untrusted ACK content should be refreshed back to the pre-consume editor buffer"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("ack_mismatch"),
            "ACK mismatch must remain a blocked writeback:\n{log}"
        );
        assert!(
            log.contains("queue_consume_ack_mismatch_editor_refresh")
                && log.contains("action=revert_untrusted_ack_content"),
            "ACK mismatch should refresh the editor back to the pre-consume buffer:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "ACK mismatch must not take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn pending_write_shorter_ack_replays_missing_agent_response() {
        // `#ack-shorter-replay`: a plugin ACK that proves every non-exchange
        // component but is missing the newly materialized `### Re:` block is not
        // user drift. Refresh the editor to the target response and treat the
        // write as converged instead of leaving the cycle interrupted.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = doc_with_queue_and_exchange("- do [#head]\n", "");
        let target = doc_with_queue_and_exchange(
            "- do [#head]\n",
            "### Re: do [#head]\n\nAnswered from the agent.\n",
        );
        let shorter_ack = source.clone();
        assert!(
            shorter_ack.len() < target.len(),
            "test setup should model the shorter recovered ack"
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, shorter_ack);
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::plugin_owner::write_plugin_owner_lease_for_test(
            doc.to_str().unwrap(),
            std::process::id(),
        );

        converge_document_or_disk(&doc, &target, &source, "pending_write")
            .expect("safe shorter ack should replay the target response through the editor");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "safe shorter ack should leave the editor/disk at the target response"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_ack_mismatch_editor_refresh")
                && log.contains("action=replay_missing_agent_response"),
            "shorter ack should refresh the editor to the target response:\n{log}"
        );
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=editor_ipc")
                && log.contains("recovery=ack_mismatch_replayed_target"),
            "shorter ack recovery should be recorded as successful editor convergence:\n{log}"
        );
        assert!(
            !log.contains("action=refuse_external_disk_write"),
            "safe shorter ack must not be recorded as a refused external disk write:\n{log}"
        );
    }

    #[test]
    fn queue_consume_ack_mismatch_does_not_refresh_user_prompt_drift() {
        // If the ACK content carries a genuine concurrent editor prompt, the
        // binary must still refuse the disk write but must not refresh the editor
        // back to the pre-consume document, because that would drop user work.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        let user_ack = target.replace(
            "<!-- /agent:exchange -->",
            "❯ do [#followup] preserve this concurrent prompt\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, user_ack.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::plugin_owner::write_plugin_owner_lease_for_test(
            doc.to_str().unwrap(),
            std::process::id(),
        );

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            user_ack,
            "user prompt drift must remain editor-owned instead of being refreshed away"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_ack_mismatch_editor_refresh")
                && log.contains("untrusted_ack_content_contains_user_drift")
                && log.contains("action=leave_editor_owned_ack_content"),
            "user drift should block the refresh path:\n{log}"
        );
        assert!(
            !log.contains("action=revert_untrusted_ack_content"),
            "user drift must not be reverted:\n{log}"
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
    fn converge_document_or_disk_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the shared converger
        // may use a guarded direct disk write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .expect("detached queue consume should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger should write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener queue consume must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener queue consume must not record disk fallback:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_diverged_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "under-capable editor sidecar must block with the missing capability: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_matching_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "matching under-capable editor sidecar must block delivery too: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "matching under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "delivery capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_editor_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let listener_root = dir.path().to_path_buf();
        let doc_for_listener = doc_str.clone();
        let source_for_listener = source.clone();
        let server = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v.clone());
                if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer") {
                    let published =
                        crate::debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_listener,
                            &source_for_listener,
                            "jetbrains-old",
                            "jetbrains",
                            "test",
                            &[crate::debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        );
                    published.ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            });
        });
        std::thread::sleep(Duration::from_millis(120));

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        assert!(
            missing.is_none(),
            "a capable editor refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw publish_live_buffer");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=editor_ipc")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only editor IPC:\n{log}"
        );

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_file_signal() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("vscode-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let signal_root = dir.path().to_path_buf();
        let doc_for_signal = doc_str.clone();
        let source_for_signal = source.clone();
        let signal_thread = std::thread::spawn(move || {
            let signal = signal_root
                .join(".agent-doc")
                .join("patches")
                .join("publish-live-buffer.signal");
            for _ in 0..100 {
                if let Ok(raw) = fs::read_to_string(&signal) {
                    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    *captured_clone.lock().unwrap() = Some(v.clone());
                    if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer")
                    {
                        crate::debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_signal,
                            &source_for_signal,
                            "vscode-old",
                            "vscode",
                            "test",
                            &[crate::debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        )
                        .unwrap();
                    }
                    let _ = fs::remove_file(&signal);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("publish-live-buffer file signal was not written");
        });

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        signal_thread.join().unwrap();
        assert!(
            missing.is_none(),
            "a capable file-signal refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("file signal was captured");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish-live-buffer signal must be read-only: {msg}"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=file_signal")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only file signal IPC:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_detached_disk_with_capable_live_buffer() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            "jetbrains-new",
            "jetbrains",
            "0.2.197",
            &[crate::debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no_listener"),
            "capable sidecar without listener should fail closed instead of detached disk: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "live editor sidecar must leave the on-disk document unchanged"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("reason=editor_capability_missing"),
            "capable sidecar must not trip the capability guard:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_detached"),
            "live editor sidecar must block detached disk write:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_route_source_writes_detached_disk_without_listener() {
        // `#fccroute`: the three route/dispatch session-document write sites
        // (`route_session_id`, `route_dedup_scrub`, `route_queue_activation`) now
        // route their disk writes through `converge_document_or_disk` so a live JB
        // editor converges them instead of hitting the File Cache Conflict dialog.
        // With no listener or live editor sidecar, detached realtime writes the
        // current file through the guarded disk path. Cover each route label so a
        // future regression on any one of them is caught.
        for source_label in [
            "route_session_id",
            "route_dedup_scrub",
            "route_queue_activation",
        ] {
            let dir = TempDir::new().unwrap();
            let agent_doc_dir = dir.path().join(".agent-doc");
            fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
            let doc = dir.path().join("plan.md");

            let source = crate::test_support::queue_consume_convergence_source();
            let target = crate::test_support::queue_consume_convergence_target();
            fs::write(&doc, &source).unwrap();

            converge_document_or_disk(&doc, &target, &source, source_label)
                .unwrap_or_else(|err| panic!("{source_label}: detached write failed: {err}"));

            assert_eq!(
                fs::read_to_string(&doc).unwrap(),
                target,
                "{source_label}: with no listener the converger must write the target"
            );
            let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
            assert!(
                log.contains(&format!("{source_label}_writeback"))
                    && log.contains("transport=disk_detached")
                    && log.contains("reason=no_listener"),
                "{source_label}: no-listener route write must record a source-labelled detached writeback:\n{log}"
            );
            assert!(
                !log.contains("transport=disk_fallback"),
                "{source_label}: no-listener route write must not record disk fallback:\n{log}"
            );
        }
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
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed (protects the buffer) rather than treating the
        // ack-without-content listener as the editor-less CLI-only case.
        crate::plugin_owner::write_plugin_owner_lease_for_test(
            doc.to_str().unwrap(),
            std::process::id(),
        );

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
    fn converge_or_disk_write_writes_detached_disk_without_listener() {
        // Detached realtime: the converge-or-disk gate used by pending/review,
        // dedupe, preflight-maintenance, and pipeline-mirror write sites may
        // write disk directly only when no editor endpoint or live sidecar owns
        // the document.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_or_disk_write(&doc, &source, &target, "pending_write")
            .expect("detached pending write should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener plain converge must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a no-listener plain converge must not record disk fallback:\n{log}"
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
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed on unproven delivery.
        crate::plugin_owner::write_plugin_owner_lease_for_test(
            doc.to_str().unwrap(),
            std::process::id(),
        );

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
    fn converge_document_or_disk_editorless_socket_blocks_without_ack_proof() {
        // `#6b5h` cutover: a pure-CLI session may see a connectable
        // controller-hosted socket with NO plugin editor behind it. An
        // ack-without-content listener still does not prove editor convergence, so
        // the realtime path fails closed instead of routing the write to disk.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        // No plugin-owner lease seeded → no live editor endpoint, but the
        // connectable socket still requires convergence proof.

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("editor convergence is unproven"),
            "editorless socket without ack proof should fail closed: {err}"
        );

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "unproven editor convergence must not be followed by a disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content")
                && log.contains("editor_endpoint=absent")
                && log.contains("action=editor_convergence_required"),
            "editorless socket must record a fail-closed convergence requirement:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "editorless socket must not route missing ACK proof to disk fallback:\n{log}"
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
    fn live_prompt_drift_auto_recovery_preserves_disk_only_queue_item() {
        // A user-added `do [#id]` queue line is disjoint realtime state: the
        // response can land while the queue edit remains in the merged target.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );
        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("queue edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- do [#user-added-queue-item]"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_partial_exchange_word() {
        // A raw word typed into the exchange after preflight is operator-visible
        // document text even when it is not yet a complete prompt. Recovery may
        // append the missing agent response, but it must not reset the exchange
        // back to the pre-typing snapshot.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("partial exchange text should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(
            target.contains("operator-partial-wo"),
            "operator-typed partial word must survive recovery:\n{target}"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_disk_only_backlog_text() {
        // Ordinary operator text is just as authoritative as prompt-shaped text:
        // realtime recovery keeps it and adds only the missing response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- existing backlog text with operator word"));
        assert!(!target.contains("- existing backlog text\n<!-- /agent:backlog -->"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_deleted_backlog_text() {
        // Operator deletions are also authoritative. Recovery must not resurrect
        // a deleted backlog line while restoring the agent response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n- operator deleted this\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("backlog deletions should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- keep this"));
        assert!(!target.contains("operator deleted this"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_edited_backlog_text() {
        // Same for edits/replacements: the file line is not a prompt, but the
        // operator-visible value must win over the older snapshot value.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- edited backlog wording"));
        assert!(!target.contains("- original backlog wording"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_rebases_onto_post_preflight_response_block_deletion() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let historical =
            "### Re: do #old — gpt-5\n\nHistorical answer the operator deleted after preflight.\n";
        let preflight = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let snapshot = crate::test_support::drift_content_ours().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let current = preflight.replace(historical, "");
        fs::write(&doc, &current).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // Preflight observed the historical response. The operator deleted it
        // before auto-recovery ran, so recovery must not resurrect it while
        // trying to restore the new response.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&preflight)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &current).unwrap();
        assert!(
            recovered.as_deref().is_some_and(|content| {
                content.contains("### Re: do #fix")
                    && !content.contains("Historical answer the operator deleted")
            }),
            "post-preflight response-block deletion should be preserved while the new response lands"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert!(disk.contains("### Re: do #fix"));
        assert!(!disk.contains("Historical answer the operator deleted"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_advances_snapshot_to_operator_preserving_merge() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented)
            .unwrap()
            .expect("response should merge onto edited backlog");

        assert!(recovered.contains("### Re: do #fix"));
        assert!(recovered.contains("- edited backlog wording"));
        assert!(!recovered.contains("- original backlog wording"));
        assert_eq!(fs::read_to_string(&doc).unwrap(), recovered);
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(recovered.as_str()),
            "snapshot must advance to the operator-preserving merged document"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_writes_realtime_merge_when_blocked_and_safe() {
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
            "the no-operator-drift merge equals the candidate response snapshot"
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
    fn try_auto_recover_live_prompt_drift_editor_ipc_preserves_partial_exchange_word() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );
        let recovery_target = live_prompt_drift_recovery_target(&snapshot, &fragmented)
            .expect("partial exchange text should be preserved in the target");
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = crate::test_support::start_live_prompt_drift_ack_listener(
            dir.path(),
            recovery_target.clone(),
        );
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(recovery_target.as_str()),
            "editor IPC recovery should accept the operator-preserving target"
        );
        let visible = fs::read_to_string(&doc).unwrap();
        assert!(
            visible.contains("operator-partial-wo") && visible.contains("### Re: do #fix"),
            "the fake editor listener must retain the partial word and land the response:\n{visible}"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true")
                && !log.contains("transport=disk_fallback"),
            "partial-word recovery must go through editor IPC without disk fallback:\n{log}"
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
            "auto-recovery must not write the merged target behind the editor"
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
            "auto-recovery must write the realtime merge target to disk"
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
    fn stale_snapshot_reset_drift_rebases_historical_exchange_trim_and_sibling_queue_add() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let kept_block = "### Re: kept - gpt-5\n\nKept response.\n";
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: opencode\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n<!-- /agent:queue -->\n"
        );
        let current = format!(
            "---\nagent_doc_session: test\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n- do [#sibling]\n<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let active_node_key = queue_node_key_for_id(&snapshot, "active");
        let scope = agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(
            Some(agent_doc_core::turn_scope::Address::node(
                "queue",
                0,
                &active_node_key,
            )),
            Some(0),
        );
        crate::turn_scope_store::save(&doc, &scope).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "preflight")
                .expect("historical trim plus sibling queue add should rebase");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(current));
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("historical_exchange_trim_unrelated_drift"),
            "rebase marker should explain the scoped drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_replacement_on_stream_write() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let scope =
            agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(None, Some(0));
        crate::turn_scope_store::save(&doc, &scope).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write")
                .expect("compact summary replacement should rebase stale pre-compact snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            crate::snapshot::load(&doc).unwrap(),
            Some(current.to_string())
        );
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("phase=stream write")
                && ops_log.contains("historical_exchange_trim"),
            "stream-write rebase marker should explain compact-summary drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_after_clear_via_binary_origin_marker() {
        // `#provauth3`: a session resumed after `/clear` has NO turn scope, but the
        // binary-authored compaction marker survives on disk. The guard must treat
        // the pre-compact snapshot vs compacted file shrink as authoritative
        // binary-origin state and rebase, instead of tripping "looks like a manual
        // cleanup" (the bug hit at the start of this dogfood session).
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        // No turn_scope saved (post-`/clear`). The binary-origin signal is the
        // recorded compaction marker.
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
                .expect("binary-origin compaction marker should rebase the stale snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            crate::snapshot::load(&doc).unwrap(),
            Some(current.to_string())
        );
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("historical_exchange_trim"),
            "post-clear compaction rebase marker should explain the drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_compact_summary_without_scope_or_marker() {
        // `#provauth3` safety rail: an exchange shrink to a compaction-shaped block
        // with NEITHER a live turn scope NOR a recorded binary compaction has no
        // provenance, so it must still fail closed (a genuine accidental cleanup
        // that happens to look like a summary must not be auto-adopted).
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        // No turn_scope and no compaction marker → no provenance signal.

        let err = guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
            .expect_err("compaction-shaped shrink without provenance must fail closed");
        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unproven shrink should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_fake_session_summary_without_compact_marker() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- /agent:exchange -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\nOperator-authored replacement without compact archive proof.\n<!-- /agent:exchange -->\n";
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let scope =
            agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(None, Some(0));
        crate::turn_scope_store::save(&doc, &scope).unwrap();

        let err =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write")
                .expect_err("non-compact exchange rewrite must still fail closed");

        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unsafe exchange rewrite should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_when_active_queue_driver_changes() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let kept_block = "### Re: kept - gpt-5\n\nKept response.\n";
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: opencode\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n<!-- /agent:queue -->\n"
        );
        let current = format!(
            "---\nagent_doc_session: test\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#sibling]\n<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let active_node_key = queue_node_key_for_id(&snapshot, "active");
        let scope = agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(
            Some(agent_doc_core::turn_scope::Address::node(
                "queue",
                0,
                &active_node_key,
            )),
            Some(0),
        );
        crate::turn_scope_store::save(&doc, &scope).unwrap();
        let (_, snapshot_body) = crate::frontmatter::parse(&snapshot).unwrap();
        let (_, current_body) = crate::frontmatter::parse(&current).unwrap();
        let queue_events: Vec<_> =
            agent_doc_markdown_ast::events::diff_node_events(snapshot_body, current_body)
                .into_iter()
                .filter(|event| event.component == "queue")
                .collect();
        assert!(
            !component_change_is_turn_independent(snapshot_body, current_body, "queue", &scope),
            "fixture should affect the active queue driver; events={queue_events:?}"
        );

        let err = guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "preflight")
            .expect_err("active queue driver edit must stay structural");

        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unsafe drift should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
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

    // `#ipctruncrecover`: the containment guard the preflight editor-buffer recovery
    // uses to refuse trusting an editor buffer that itself lost the committed response.
    fn doc_with_exchange(exchange_body: &str, queue_body: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n<!-- agent:exchange -->\n{exchange_body}\n<!-- /agent:exchange -->\n## Queue\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_accepts_buffer_with_head_response_plus_editor_edits() {
        // HEAD committed a response; the flushed editor buffer keeps that whole response
        // and adds an editor-owned queue edit. The response was not lost → trust it.
        let head = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");
        let flushed = doc_with_exchange(
            "### Re: topic\n\nThe committed answer.",
            "- do [#a]\n- a new operator queue line",
        );
        assert!(editor_buffer_preserved_head_exchange(&flushed, &head));
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_rejects_buffer_that_dropped_committed_response() {
        // The flushed buffer is itself truncated — it lost a committed response line.
        // Recovery must refuse and fall through to the safe bail.
        let head = doc_with_exchange(
            "### Re: topic\n\nThe committed answer.\n\nA second committed paragraph.",
            "- do [#a]",
        );
        let flushed = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");
        assert!(!editor_buffer_preserved_head_exchange(&flushed, &head));
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_ignores_boundary_markers() {
        // A `(HEAD)` boundary annotation on the flushed side must not perturb the match.
        let head = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");
        let flushed =
            doc_with_exchange("### Re: topic (HEAD)\n\nThe committed answer.", "- do [#a]");
        assert!(editor_buffer_preserved_head_exchange(&flushed, &head));
    }
}
