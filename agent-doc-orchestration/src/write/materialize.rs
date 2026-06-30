//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_template::response_materialization::{
    response_materialization_probe, response_materialization_probe_from_response,
    same_ignoring_trailing_newlines, serialize_template_response,
    strip_partial_response_materialization_from_exchange,
};
use std::collections::HashSet;

pub struct NormalizedTemplateResponse {
    pub response_for_capture: Option<String>,
    pub patches: Vec<template::PatchBlock>,
    pub unmatched: String,
}

pub(crate) fn pending_replace_escape_hatch_enabled() -> bool {
    std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false)
        || std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
            .map(|v| v == "1")
            .unwrap_or(false)
}

pub fn response_materialized_in_content(response: &str, content: &str) -> bool {
    let probe = response_materialization_probe_from_response(response);
    if probe.trim().is_empty()
        || agent_doc_turn::response_replay::response_already_applied(content, &probe)
        || agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
            content, &probe,
        )
    {
        return true;
    }
    let normalized_content = agent_doc_document::transient_markers::strip_guard_markers(content);
    normalized_content != content
        && (agent_doc_turn::response_replay::response_already_applied(&normalized_content, &probe)
            || agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
                &normalized_content,
                &probe,
            ))
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
    let response_hash = agent_doc_hash::content_hash(response);
    let content_hash = agent_doc_hash::content_hash(content);
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

pub(crate) fn log_partial_response_materialization_for_retry(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    let Ok(current) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    if response_materialized_in_content(response, &current) {
        return Ok(());
    }
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
            agent_doc_hash::content_hash(response),
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
    let (_, current_items, _) = agent_doc_element_backlog::backlog::parse_items(current_body);
    let current_ids: HashSet<String> = current_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.clone())
        .collect();
    let current_states: HashMap<String, agent_doc_element_backlog::backlog::PendingState> =
        current_items
            .iter()
            .map(|item| (item.id.clone(), item.state))
            .collect();

    let backlog_index = backlog_indexes[0];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let (mut target_body, _) = agent_doc_element_backlog::backlog::backfill(
        &patches[backlog_index].content,
        &doc_id,
        &current_ids,
    );
    if !agent_doc_element_backlog::backlog::preserves_non_item_structure(current_body, &target_body)
    {
        if let Some(merged_body) = agent_doc_element_backlog::backlog::merge_partial_backlog_prefix(
            current_body,
            &target_body,
        ) {
            target_body = merged_body;
        } else {
            anyhow::bail!(
                "ERR: pending/backlog patch changed non-list content — use granular --pending-* flags instead"
            );
        }
    }
    let (_, target_items, _) = agent_doc_element_backlog::backlog::parse_items(&target_body);
    let rendered_target =
        agent_doc_element_backlog::backlog::canonicalize_preserving_non_item_lines(&target_body);
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
            agent_doc_element_backlog::backlog::ensure_no_new_leading_custom_id_prefix(
                &item.id,
                &item.text,
                &current_ids,
                "ERR: pending/backlog patch",
            )?;
            if !current_ids.contains(&item.id) {
                saw_pending_add = true;
            }
            if item.state == agent_doc_element_backlog::backlog::PendingState::Done
                && current_states.get(&item.id).copied()
                    != Some(agent_doc_element_backlog::backlog::PendingState::Done)
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

    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);
    let normalized =
        normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    Ok(normalized
        .response_for_capture
        .unwrap_or_else(|| response.to_string()))
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
        let patches = vec![agent_doc_template::PatchBlock::new(
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
}
