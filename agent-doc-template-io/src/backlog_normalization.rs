use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use agent_doc_element::element::{self, is_backlog_component};
use agent_doc_frontmatter::frontmatter;
use agent_doc_template as template;
use agent_doc_template::response_materialization::{
    same_ignoring_trailing_newlines, serialize_template_response,
};

pub struct NormalizedTemplateResponse {
    pub response_for_capture: Option<String>,
    pub patches: Vec<template::PatchBlock>,
    pub unmatched: String,
}

pub fn pending_replace_escape_hatch_enabled() -> bool {
    std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Enforcement: reject full-replacement blocks targeting the `pending` component
/// unless the caller explicitly opts in.
///
/// Canonical form: `<!-- replace:pending -->...<!-- /replace:pending -->` with
/// `--allow-replace-pending` (or `AGENT_DOC_ALLOW_REPLACE_PENDING=1`).
///
/// The pending system requires mutations via granular flags
/// (`--pending-add/done/edit/clear/reorder`); a full-replace block on a list the
/// user concurrently edits enables silent-data-loss via concurrent-edit clobber
/// and hash instability.
///
/// Phase 3 inversion (2026-04-14): the default is now reject. Library callers
/// (FFI, tests, future SDK consumers) must opt in explicitly.
pub fn enforce_no_replace_pending(patches: &[template::PatchBlock], allow: bool) -> Result<()> {
    if allow {
        return Ok(());
    }
    if pending_replace_escape_hatch_enabled() {
        return Ok(());
    }
    if patches.iter().any(|p| {
        is_backlog_component(&p.name) || agent_doc_element::element::is_review_component(&p.name)
    }) {
        anyhow::bail!(
            "ERR: replace:pending/review block forbidden — use --pending-add/done/edit/clear/reorder or --review-add/edit. \
             See specs/pending-system.md."
        );
    }
    Ok(())
}

pub fn normalize_backlog_patch_response(
    file: &Path,
    current_content: &str,
    patches: Vec<template::PatchBlock>,
    unmatched: String,
    allow_replace: bool,
) -> Result<NormalizedTemplateResponse> {
    normalize_backlog_patch_response_with_application(
        file,
        current_content,
        patches,
        unmatched,
        allow_replace,
        true,
    )
}

fn normalize_backlog_patch_response_with_application(
    file: &Path,
    current_content: &str,
    mut patches: Vec<template::PatchBlock>,
    unmatched: String,
    allow_replace: bool,
    apply_visible_mutation: bool,
) -> Result<NormalizedTemplateResponse> {
    if allow_replace {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }
    if pending_replace_escape_hatch_enabled() {
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

    if apply_visible_mutation && !same_ignoring_trailing_newlines(current_body, &target_body) {
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
        agent_doc_document_realtime_io::guard_visible_write_current_transition(
            file,
            "normalize_pending_patch",
        )?;
        agent_doc_document_realtime_io::atomic_write_through_authority(file, &rewritten_doc)
            .with_context(|| {
                format!(
                    "failed to write normalized pending state {}",
                    file.display()
                )
            })?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "normalize_pending_patch file={} added={} done={}",
                file.display(),
                saw_pending_add,
                pending_done_ids.len()
            ),
        );
        if saw_pending_add {
            agent_doc_cycle_state_io::mark_pending_mutations(file)?;
        }
        if !pending_done_ids.is_empty() {
            agent_doc_cycle_state_io::record_pending_done_ids(file, &pending_done_ids)?;
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

    let current_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "canonicalize_response_for_capture",
    )?;
    canonicalize_response_for_capture_with_current_content(file, response, &current_content)
}

pub fn canonicalize_response_for_capture_with_current_content(
    file: &Path,
    response: &str,
    current_content: &str,
) -> Result<String> {
    if !response.contains("<!-- patch:") {
        return Ok(response.to_string());
    }

    let Ok((fm, _)) = frontmatter::parse(current_content) else {
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
    // Capture canonicalization is proof construction, not intent application.
    // The complete patch set remains in the state-backed turn intent and is
    // applied once by the document-write state machine.
    let normalized = normalize_backlog_patch_response_with_application(
        file,
        current_content,
        patches,
        unmatched,
        false,
        false,
    )?;
    Ok(normalized
        .response_for_capture
        .unwrap_or_else(|| response.to_string()))
}

#[cfg(test)]
mod pending_patch_normalization_tests {
    use super::{
        canonicalize_response_for_capture_with_current_content, enforce_no_replace_pending,
        normalize_backlog_patch_response,
    };
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
    fn capture_canonicalization_is_pure_and_retains_full_intent_for_later_application() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "- [ ] [#keep1] existing item\n");
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: prompt — gpt-5\n\nDone.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#keep1] existing item\n",
            "- [ ] [#new1] new item\n",
            "<!-- /patch:backlog -->\n"
        );

        let canonical =
            canonicalize_response_for_capture_with_current_content(&doc, response, &content)
                .expect("capture proof should canonicalize");

        assert!(canonical.contains("patch:exchange"));
        assert!(!canonical.contains("patch:backlog"));
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "proof construction must not apply any part of the turn intent"
        );
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
        enforce_no_replace_pending(&patches, true).expect("allow=true should bypass enforcement");
    }

    #[test]
    fn write_flags_default_rejects_replace_pending() {
        let tmp = TempDir::new().unwrap();
        let (_doc, _content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![agent_doc_template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        enforce_no_replace_pending(&patches, false)
            .expect_err("allow=false should reject backlog replacement");
    }
}
