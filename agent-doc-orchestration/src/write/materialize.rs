//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub struct NormalizedTemplateResponse {
    pub response_for_capture: Option<String>,
    pub patches: Vec<template::PatchBlock>,
    pub unmatched: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemplateResponseWriteProof {
    pub(crate) explicit_components: Vec<String>,
    pub(crate) unmatched_len: usize,
}

impl TemplateResponseWriteProof {
    pub(crate) fn has_real_body(&self) -> bool {
        !self.explicit_components.is_empty() || self.unmatched_len > 0
    }
}

pub(crate) fn template_response_write_proof(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> TemplateResponseWriteProof {
    TemplateResponseWriteProof {
        explicit_components: patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !is_backlog_component(&patch.name))
            .filter(|patch| !agent_doc_element::element::is_review_component(&patch.name))
            .filter(|patch| !patch.content.trim().is_empty())
            .map(|patch| patch.name.clone())
            .collect(),
        unmatched_len: unmatched.trim().len(),
    }
}

pub fn ensure_template_response_write_proof(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    let proof = template_response_write_proof(patches, unmatched);
    if proof.has_real_body() {
        return Ok(());
    }

    anyhow::bail!(
        "template response contains no real response-body write — include at least one non-empty response patch or non-empty unmatched response body"
    );
}

pub fn ensure_strict_template_response_heading(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    if template_response_has_heading(patches, unmatched) {
        return Ok(());
    }

    anyhow::bail!(
        "strict template closeout response must include a `### Re:` response heading in `patch:exchange` or unmatched response body"
    );
}

pub fn ensure_strict_template_response_heading_for_current_doc(
    current_content: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    match ensure_strict_template_response_heading(patches, unmatched) {
        Ok(()) => Ok(()),
        Err(_)
            if live_exchange_tail_proves_streamed_response_heading(
                current_content,
                patches,
                unmatched,
            ) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn template_response_has_heading(patches: &[template::PatchBlock], unmatched: &str) -> bool {
    response_text_has_heading(unmatched)
        || patches.iter().any(|patch| {
            patch.name == "exchange"
                && !patch.content.trim().is_empty()
                && response_text_has_heading(&patch.content)
        })
}

fn live_exchange_tail_proves_streamed_response_heading(
    current_content: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> bool {
    if !unmatched.trim().is_empty() {
        return false;
    }

    let mut non_empty = patches
        .iter()
        .filter(|patch| !patch.content.trim().is_empty());
    let Some(patch) = non_empty.next() else {
        return false;
    };
    if non_empty.next().is_some() || patch.name != "exchange" {
        return false;
    }

    let Ok(components) = agent_doc_element::element::parse(current_content) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .rev()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let exchange_content = exchange.content(current_content);
    let Some(tail_start) = offset_after_last_prompt_line(exchange_content) else {
        return false;
    };

    response_text_has_heading(&exchange_content[tail_start..])
}

fn offset_after_last_prompt_line(text: &str) -> Option<usize> {
    let mut offset = 0usize;
    let mut last = None;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('❯') {
            last = Some(offset + line.len());
        }
        offset += line.len();
    }
    last
}

fn response_text_has_heading(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("##### Re:")
            || trimmed.starts_with("###### Re:")
            || trimmed.starts_with("## Re:")
    })
}

pub(crate) fn pending_replace_escape_hatch_enabled() -> bool {
    std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false)
        || std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
            .map(|v| v == "1")
            .unwrap_or(false)
}

pub(crate) fn same_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

pub(crate) fn serialize_template_response(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> String {
    let mut out = String::new();
    for patch in patches {
        out.push_str("<!-- patch:");
        out.push_str(&patch.name);
        if !patch.attrs.is_empty() {
            let mut attrs: Vec<_> = patch.attrs.iter().collect();
            attrs.sort_by_key(|(left, _)| *left);
            for (key, value) in attrs {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                out.push_str(&value.replace('"', "&quot;"));
                out.push('"');
            }
        }
        out.push_str(" -->\n");
        out.push_str(&patch.content);
        if !patch.content.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("<!-- /patch:");
        out.push_str(&patch.name);
        out.push_str(" -->\n");
    }
    if !unmatched.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(unmatched.trim());
        out.push('\n');
    }
    out
}

pub(crate) fn response_materialization_probe(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> String {
    let mut selected = patches
        .iter()
        .filter(|patch| patch.name == "exchange")
        .cloned()
        .collect::<Vec<_>>();
    let selected_exchange = !selected.is_empty();
    if selected.is_empty() && unmatched.trim().is_empty() {
        selected = patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !is_backlog_component(&patch.name))
            .filter(|patch| !agent_doc_element::element::is_review_component(&patch.name))
            .cloned()
            .collect();
    }
    let probe_unmatched = if selected_exchange { "" } else { unmatched };
    materialized_template_response(&selected, probe_unmatched)
}

pub(crate) fn materialized_template_response(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> String {
    let mut out = String::new();
    for patch in patches {
        push_materialization_segment(&mut out, &patch.content);
    }
    push_materialization_segment(&mut out, unmatched);
    out
}

pub(crate) fn push_materialization_segment(out: &mut String, segment: &str) {
    let segment = segment.trim_matches(|c| c == '\n' || c == '\r');
    if segment.trim().is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(segment);
    out.push('\n');
}

pub fn response_materialization_probe_from_response(response: &str) -> String {
    let probe = match template::parse_patches(response) {
        Ok((patches, unmatched)) => response_materialization_probe(&patches, &unmatched),
        Err(_) => response.to_string(),
    };
    // Ephemeral per-cycle guard markers (`<!-- no-pending-done-guard -->`,
    // `<!-- no-pending-capture -->`) are stripped from committed blobs by
    // `git::strip_guard_markers`, so a captured response body that still carries
    // them would never match the committed HEAD/archive content and
    // `stuck_captured_cycle` would false-alarm on a response that is in fact
    // committed (#8j86). Strip them from the probe so the match mirrors commit.
    strip_ephemeral_markers(&probe)
}

pub fn response_materialized_in_content(response: &str, content: &str) -> bool {
    let probe = response_materialization_probe_from_response(response);
    if probe.trim().is_empty()
        || crate::repair::response_already_applied(content, &probe)
        || crate::repair::response_already_applied_after_prefix_strip(content, &probe)
    {
        return true;
    }
    let normalized_content = strip_ephemeral_markers(content);
    normalized_content != content
        && (crate::repair::response_already_applied(&normalized_content, &probe)
            || crate::repair::response_already_applied_after_prefix_strip(
                &normalized_content,
                &probe,
            ))
}

fn strip_ephemeral_markers(content: &str) -> String {
    crate::git::strip_guard_markers(content)
}

pub(crate) fn reject_marker_response_with_zero_patches(
    marker_count: usize,
    patch_count: usize,
) -> Result<()> {
    if patch_count == 0 && marker_count > 0 {
        anyhow::bail!(
            "template patchback parsed zero patches despite {marker_count} patch marker(s); refusing to capture a malformed response"
        );
    }
    Ok(())
}

pub(crate) fn ipc_response_materialized_or_fallback(
    file: &Path,
    source: &str,
    response: &str,
    content: &str,
) -> bool {
    if response_materialized_in_content(response, content) {
        return true;
    }
    let response_hash = crate::ops_log::content_hash(response);
    let content_hash = crate::ops_log::content_hash(content);
    eprintln!(
        "[write] IPC {} consumed a patch for {}, but the materialized content is missing the captured response body — retry required before snapshot/commit",
        source,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_materialization_missing_response file={} source={} response_sha256={} content_len={} content_hash={}",
            file.display(),
            source,
            response_hash,
            content.len(),
            content_hash
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        None,
        "missing_response_probe",
        "retry_without_disk_write",
        &format!(
            "response_sha256={} content_len={} content_hash={}",
            response_hash,
            content.len(),
            content_hash
        ),
    );
    false
}

pub(crate) fn log_ipc_proof_failure(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    eprintln!(
        "[write] IPC proof insufficient for {}: source={} patch_id={} invariant={} recovery={}{}{}",
        file.display(),
        source,
        patch_id.unwrap_or("-"),
        invariant,
        recovery,
        if detail.is_empty() { "" } else { " " },
        detail
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_proof_insufficient file={} source={} patch_id={} invariant={} recovery={}{}{}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            invariant,
            recovery,
            if detail.is_empty() { "" } else { " " },
            detail
        ),
    );
}

pub(crate) fn strip_partial_response_materialization_from_exchange(
    content: &str,
    response: &str,
) -> Option<String> {
    if response_materialized_in_content(response, content) {
        return None;
    }
    let headings = response
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .collect::<Vec<_>>();
    if headings.is_empty() {
        return None;
    }

    let components = element::parse(content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let exchange_body = &content[exchange.open_end..exchange.close_start];
    let mut repaired_exchange = String::with_capacity(exchange_body.len());
    let mut removed = false;
    let mut skipping_partial = false;

    for segment in exchange_body.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n');
        let trimmed = line.trim();
        let is_target_response_heading = headings.contains(&trimmed);
        let is_structural_boundary = trimmed.starts_with("<!-- agent:boundary:")
            || trimmed.starts_with("<!-- /agent:")
            || trimmed.starts_with("<!-- agent:");
        let is_other_response_heading =
            trimmed.starts_with("### Re:") && !is_target_response_heading;
        let is_user_prompt_line = trimmed.starts_with('❯');

        if skipping_partial
            && (is_structural_boundary || is_other_response_heading || is_user_prompt_line)
        {
            skipping_partial = false;
        }

        if is_target_response_heading {
            skipping_partial = true;
            removed = true;
            continue;
        }

        if skipping_partial {
            removed = true;
            continue;
        }

        repaired_exchange.push_str(segment);
    }

    if !removed {
        return None;
    }

    let mut repaired = String::with_capacity(content.len());
    repaired.push_str(&content[..exchange.open_end]);
    repaired.push_str(&repaired_exchange);
    repaired.push_str(&content[exchange.close_start..]);
    Some(repaired)
}

pub(crate) fn log_partial_response_materialization_for_retry(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    let Ok(current) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    let Some(repaired) = strip_partial_response_materialization_from_exchange(&current, response)
    else {
        return Ok(());
    };
    eprintln!(
        "[write] IPC {} partial response materialization left in editor buffer for retry for {}",
        source,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_partial_materialization_retained_for_retry file={} source={} response_sha256={} current_len={} stripped_len={}",
            file.display(),
            source,
            crate::ops_log::content_hash(response),
            current.len(),
            repaired.len()
        ),
    );
    Ok(())
}

pub(crate) fn response_materialization_probe_from_ipc_payload(
    payload: &serde_json::Value,
) -> String {
    let patches = payload
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
                    let content = item.get("content").and_then(|value| value.as_str())?;
                    Some(template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    response_materialization_probe(&patches, unmatched)
}

pub fn normalize_backlog_patch_response(
    file: &Path,
    current_content: &str,
    mut patches: Vec<template::PatchBlock>,
    unmatched: String,
    allow_replace: bool,
) -> Result<NormalizedTemplateResponse> {
    if allow_replace {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }
    let allow_canonical = std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    let allow_legacy = std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if allow_canonical || allow_legacy {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }

    let backlog_indexes: Vec<usize> = patches
        .iter()
        .enumerate()
        .filter_map(|(idx, patch)| is_backlog_component(&patch.name).then_some(idx))
        .collect();

    if backlog_indexes.is_empty() {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }
    if backlog_indexes.len() > 1 {
        anyhow::bail!(
            "ERR: multiple pending/backlog patches in one response are not supported — use --pending-* flags"
        );
    }

    let components = element::parse(current_content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let backlog_component = components
        .iter()
        .find(|component| is_backlog_component(&component.name))
        .with_context(|| {
            format!(
                "document has no pending/backlog component: {}",
                file.display()
            )
        })?;
    let current_body = backlog_component.content(current_content);
    let (_, current_items, _) = crate::pending::parse_items(current_body);
    let current_ids: HashSet<String> = current_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.clone())
        .collect();
    let current_states: HashMap<String, crate::pending::PendingState> = current_items
        .iter()
        .map(|item| (item.id.clone(), item.state))
        .collect();

    let backlog_index = backlog_indexes[0];
    let doc_id = crate::pending_cmd::doc_id_for(file);
    let (mut target_body, _) =
        crate::pending::backfill(&patches[backlog_index].content, &doc_id, &current_ids);
    if !crate::pending::preserves_non_item_structure(current_body, &target_body) {
        if let Some(merged_body) =
            crate::pending::merge_partial_backlog_prefix(current_body, &target_body)
        {
            target_body = merged_body;
        } else {
            anyhow::bail!(
                "ERR: pending/backlog patch changed non-list content — use granular --pending-* flags instead"
            );
        }
    }
    let (_, target_items, _) = crate::pending::parse_items(&target_body);
    let rendered_target = crate::pending::canonicalize_preserving_non_item_lines(&target_body);
    if !same_ignoring_trailing_newlines(&rendered_target, &target_body) {
        anyhow::bail!(
            "ERR: pending/backlog patch could not be normalized into supported --pending-* operations"
        );
    }

    if !same_ignoring_trailing_newlines(current_body, &target_body) {
        let normalized_body = target_body.clone();
        let mut saw_pending_add = false;
        let mut pending_done_ids = Vec::new();

        for item in &target_items {
            crate::pending::ensure_no_new_leading_custom_id_prefix(
                &item.id,
                &item.text,
                &current_ids,
                "ERR: pending/backlog patch",
            )?;
            if !current_ids.contains(&item.id) {
                saw_pending_add = true;
            }
            if item.state == crate::pending::PendingState::Done
                && current_states.get(&item.id).copied() != Some(crate::pending::PendingState::Done)
            {
                pending_done_ids.push(item.id.clone());
            }
        }

        let rewritten_doc = backlog_component.replace_content(current_content, &normalized_body);
        guard_visible_write_idle(file, "normalize_pending_patch")?;
        std::fs::write(file, &rewritten_doc).with_context(|| {
            format!(
                "failed to write normalized pending state {}",
                file.display()
            )
        })?;
        crate::ops_log::log_op(
            file,
            &format!(
                "normalize_pending_patch file={} added={} done={}",
                file.display(),
                saw_pending_add,
                pending_done_ids.len()
            ),
        );
        if saw_pending_add {
            crate::cycle_state::mark_pending_mutations(file)?;
        }
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(file, &pending_done_ids)?;
        }
    }

    patches.remove(backlog_index);
    let response_for_capture = Some(serialize_template_response(&patches, &unmatched));
    Ok(NormalizedTemplateResponse {
        response_for_capture,
        patches,
        unmatched,
    })
}

pub fn canonicalize_response_for_capture(file: &Path, response: &str) -> Result<String> {
    if !response.contains("<!-- patch:") {
        return Ok(response.to_string());
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for response capture", file.display()))?;
    let Ok((fm, _)) = frontmatter::parse(&current_content) else {
        return Ok(response.to_string());
    };
    if !fm.resolve_mode().is_template() {
        return Ok(response.to_string());
    }

    let Ok((mut patches, mut unmatched)) = template::parse_patches(response) else {
        return Ok(response.to_string());
    };
    if !patches
        .iter()
        .any(|patch| is_backlog_component(&patch.name))
    {
        return Ok(response.to_string());
    }

    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);
    let normalized =
        normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    Ok(normalized
        .response_for_capture
        .unwrap_or_else(|| response.to_string()))
}

pub(crate) fn sanitize_template_patchback_response_for_write(response: &mut String) -> Result<()> {
    let Ok((patches, unmatched)) = template::parse_patches(response) else {
        return Ok(());
    };
    if unmatched.trim().is_empty() || !patches.iter().any(|patch| patch.name == "exchange") {
        return Ok(());
    }

    match crate::replay_guard::classify_replay_payload(response) {
        crate::replay_guard::ReplayPayloadClassification::Replayable(payload) => {
            let sanitized = payload.into_owned();
            if sanitized != response.trim() {
                *response = sanitized;
            }
            Ok(())
        }
        crate::replay_guard::ReplayPayloadClassification::Empty => {
            anyhow::bail!("empty response — nothing to write")
        }
        crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            anyhow::bail!(
                "template response contains unsafe unmatched content around patch blocks: {reason}"
            )
        }
    }
}

pub(crate) fn patchback_marker_count_outside_code(response: &str) -> usize {
    crate::flow::document_mutation::patchback_marker_count_outside_code(response)
}

#[cfg(test)]
mod pending_patch_normalization_tests {
    use super::normalize_backlog_patch_response;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn doc_with_backlog(root: &TempDir, backlog_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("doc.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, &content).unwrap();
        (doc, content)
    }

    fn doc_with_todo(root: &TempDir, todo_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("todo.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:todo patch=replace -->\n{todo_body}<!-- /agent:todo -->\n"
        );
        fs::write(&doc, &content).unwrap();
        (doc, content)
    }

    #[test]
    fn normalize_pending_patch_repairs_lone_bare_placeholder() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] repair placeholder\n",
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("lone bare placeholder should be normalized");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("repair placeholder"));
        assert!(rewritten.contains("- [ ] [#"));
        assert!(
            !rewritten.contains("- [ ] [#] repair placeholder"),
            "bare placeholder must not persist: {}",
            rewritten
        );
    }

    #[test]
    fn normalize_pending_patch_rejects_stacked_leading_id_prefixes() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] [#ship1] release checklist\n",
        )];

        let err =
            match normalize_backlog_patch_response(&doc, &content, patches, String::new(), false) {
                Ok(_) => panic!("stacked leading id prefixes should be rejected"),
                Err(err) => err,
            };
        let msg = err.to_string();
        assert!(
            msg.contains("pending/backlog patch"),
            "unexpected error: {}",
            msg
        );
        assert!(
            msg.contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn normalize_pending_patch_allows_existing_alias_tag_items() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!("### Active\n", "- [ ] [#yckq] [#ss01] ShipStation fix\n");
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#new1] add phone confirmation item\n",
                "- [ ] [#yckq] [#ss01] ShipStation fix\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("existing alias-tag items should not block normalization");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] add phone confirmation item"));
        assert!(rewritten.contains("[#yckq] [#ss01] ShipStation fix"));
    }

    #[test]
    fn normalize_pending_patch_preserves_interleaved_headers() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n",
                "\n",
                "### Later\n",
                "- [ ] [#keep2] later item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("header-preserving patch should normalize");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(
            rewritten
                .contains("### Active\n- [ ] [#keep1] existing item\n- [ ] [#new1] new top item\n")
        );
        assert!(rewritten.contains("\n\n### Later\n- [ ] [#keep2] later item\n"));
    }

    #[test]
    fn normalize_pending_patch_merges_partial_structured_prefix() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("prefix-only structured patch should merge with later sections");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] new top item"));
        assert!(rewritten.contains("### Later\n- [ ] [#keep2] later item\n"));
    }

    #[test]
    fn write_flags_allow_replace_bypasses_enforcement() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        normalize_backlog_patch_response(&doc, &content, patches.clone(), String::new(), true)
            .expect("allow_replace=true should bypass enforcement");
        super::enforce_no_replace_pending(&patches, true)
            .expect("allow=true should bypass enforcement");
    }

    #[test]
    fn write_flags_default_rejects_replace_pending() {
        let tmp = TempDir::new().unwrap();
        let (_doc, _content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        super::enforce_no_replace_pending(&patches, false)
            .expect_err("allow=false should reject backlog replacement");
    }

    #[test]
    fn destructive_todo_patch_is_rejected_when_it_drops_checklist_items() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n\n",
                "### Phase 2\n\n",
                "- [ ] Expand git signal extraction\n",
                "- [ ] Re-score sessions\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n",
            ),
        )];

        let err = super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect_err("subset todo patch should fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("patch:todo would reduce total checklist item count from 4 to 2"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn todo_patch_with_same_checklist_count_is_allowed() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [ ] Original item 1\n",
                "- [ ] Original item 2\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Updated item 1\n",
                "- [ ] Updated item 2\n",
            ),
        )];

        super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect("same-size todo rewrite should remain allowed");
    }
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
    fn materialization_probe_uses_patch_body_not_patch_markers() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: materialized — gpt-5\n\n",
            "Committed through boundary insertion.\n",
            "<!-- /patch:exchange -->\n",
        );

        let probe = response_materialization_probe_from_response(response);

        assert!(probe.contains("### Re: materialized — gpt-5"));
        assert!(!probe.contains("<!-- patch:exchange -->"));
        assert!(!probe.contains("<!-- /patch:exchange -->"));
    }
    #[test]
    fn patch_wrapped_response_is_materialized_by_visible_patch_body() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }
    #[test]
    fn patch_wrapped_response_matches_visible_body_with_transient_markers() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "No follow-up. <!-- no-pending-capture -->\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5 (HEAD)\n\n",
            "The document contains the applied body only.\n",
            "No follow-up. <!-- no-pending-capture -->\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }
    #[test]
    fn marker_bearing_zero_patch_parse_is_rejected_before_capture() {
        let err = reject_marker_response_with_zero_patches(1, 0).unwrap_err();

        assert!(
            err.to_string()
                .contains("parsed zero patches despite 1 patch marker")
        );
        assert!(reject_marker_response_with_zero_patches(0, 0).is_ok());
        assert!(reject_marker_response_with_zero_patches(2, 1).is_ok());
    }
}
