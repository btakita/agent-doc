//! Pure queue projection helpers over parsed queue entries.
//!
//! Callers provide document text and parsed queue entries. File IO, state-event
//! persistence, and ops logging stay in orchestration.

use std::collections::HashMap;

use agent_doc_document::queue_projection::{
    ActiveQueuePromptProjection, QueuePromptRow, strip_in_progress_marker, strip_priority_markers,
};
use anyhow::Result;

use crate::document_queue::{self, QueueEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueWorklistEntryKind {
    Prompt,
    Preset,
    Dispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueWorklistEntry {
    pub kind: QueueWorklistEntryKind,
    pub text: String,
    pub node_key: Option<String>,
    pub backlog_id: Option<String>,
    pub drainable: bool,
}

pub fn queue_entry_do_id(entry: &QueueEntry) -> Option<String> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
            crate::queue_response::queue_prompt_done_id(&prompt.text)
        }
        _ => None,
    }
}

pub fn queue_prompt_projection_rows(content: &str, entries: &[QueueEntry]) -> Vec<QueuePromptRow> {
    let deferred_ids = crate::queue_continuation::deferred_backlog_ids(content);
    let preset_supplies_directive = agent_doc_element::element::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "queue")
                .map(|component| component.attrs.contains_key("preset"))
        })
        .unwrap_or(false);
    entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                let text = strip_in_progress_marker(&prompt.text);
                let id = crate::queue_response::queue_prompt_done_id(&text);
                let projectable_default =
                    !crate::queue_continuation::is_noise_queue_head(
                        &text,
                        preset_supplies_directive,
                    ) && !id.as_ref().is_some_and(|id| deferred_ids.contains(id));
                Some(QueuePromptRow::new(
                    prompt.text.clone(),
                    id,
                    projectable_default,
                ))
            }
            _ => None,
        })
        .collect()
}

pub fn active_queue_prompt_projection(
    content: &str,
    entries: &[QueueEntry],
    deps: &HashMap<String, Vec<String>>,
    honor_in_progress_markers: bool,
) -> ActiveQueuePromptProjection {
    let rows = queue_prompt_projection_rows(content, entries);
    agent_doc_document::queue_projection::project_active_queue_prompts(
        &rows,
        deps,
        honor_in_progress_markers,
    )
}

pub fn in_progress_marker_retarget_requested(
    diff: Option<&str>,
    content: &str,
    entries: &[QueueEntry],
) -> bool {
    let rows = queue_prompt_projection_rows(content, entries);
    agent_doc_document::queue_projection::in_progress_marker_retarget_requested(diff, &rows)
}

pub fn selected_queue_head_node_key(content: &str, head_text: &str) -> Option<String> {
    if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        && let Some(node) = nodes.into_iter().find(|node| {
            !node.item.struck
                && strip_priority_markers(node.item.text.trim())
                    == strip_priority_markers(head_text)
        })
    {
        return Some(node.node_key);
    }
    let trimmed = head_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let hash = agent_doc_hash::content_hash(trimmed);
    let short_hash = &hash[..hash.len().min(12)];
    Some(format!("queue:entry:0:{short_hash}"))
}

pub fn queue_worklist_hash(entries: &[QueueEntry]) -> String {
    agent_doc_hash::content_hash(&document_queue::render(entries))
}

/// Deduplicate queue item node keys before queue maintenance projects state
/// from the markdown AST.
pub fn dedup_queue_nodes_by_key(content: &str) -> Result<Option<(String, usize)>> {
    let before_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse queue node keys: {err}")
        })?;
    let updated =
        agent_doc_markdown_ast::mutations::dedup_node_keys(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to dedup queue node keys: {err}")
        })?;
    if updated == content {
        return Ok(None);
    }
    let after_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse deduped queue node keys: {err}")
        })?;
    let dropped = before_nodes.len().saturating_sub(after_nodes.len());
    Ok(Some((updated, dropped)))
}

fn queue_prompt_node_key(
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    used_nodes: &mut [bool],
    prompt_text: &str,
    index: usize,
) -> Option<String> {
    let normalized_prompt = strip_in_progress_marker(prompt_text).trim().to_string();
    for (node_index, node) in nodes.iter().enumerate() {
        if used_nodes.get(node_index).copied().unwrap_or(false) || node.item.struck {
            continue;
        }
        let node_text = strip_in_progress_marker(node.item.text.trim());
        if node_text.trim() == normalized_prompt {
            if let Some(used) = used_nodes.get_mut(node_index) {
                *used = true;
            }
            return Some(node.node_key.clone());
        }
    }
    let trimmed = normalized_prompt.trim();
    if trimmed.is_empty() {
        return None;
    }
    let hash = agent_doc_hash::content_hash(trimmed);
    let short_hash = &hash[..hash.len().min(12)];
    Some(format!("queue:entry:{index}:{short_hash}"))
}

pub fn queue_worklist_entries(content: &str, entries: &[QueueEntry]) -> Vec<QueueWorklistEntry> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").unwrap_or_default();
    let mut used_nodes = vec![false; nodes.len()];
    let mut prompt_index = 0usize;
    entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                let text = strip_in_progress_marker(&prompt.text);
                let node_key = queue_prompt_node_key(&nodes, &mut used_nodes, &text, prompt_index);
                prompt_index += 1;
                Some(QueueWorklistEntry {
                    kind: QueueWorklistEntryKind::Prompt,
                    text,
                    node_key,
                    backlog_id: crate::queue_response::queue_prompt_done_id(&prompt.text),
                    drainable: true,
                })
            }
            QueueEntry::Preset(preset) => Some(QueueWorklistEntry {
                kind: QueueWorklistEntryKind::Preset,
                text: preset.clone(),
                node_key: None,
                backlog_id: None,
                drainable: false,
            }),
            QueueEntry::Dispatch(preset) => Some(QueueWorklistEntry {
                kind: QueueWorklistEntryKind::Dispatch,
                text: preset.clone(),
                node_key: None,
                backlog_id: None,
                drainable: false,
            }),
            QueueEntry::Completed(_)
            | QueueEntry::StartFence(_)
            | QueueEntry::StopFence
            | QueueEntry::Freeform(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue::{QueuePrompt, parse};

    fn prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    #[test]
    fn projection_rows_filter_noise_and_deferred_ids() {
        let content = "\
<!-- agent:queue -->
- agent:boundary
- do [#alpha]
<!-- /agent:queue -->
<!-- agent:backlog -->
- [ ] [#alpha] [operator-verify] blocked
<!-- /agent:backlog -->
";
        let entries = parse("- agent:boundary\n- do [#alpha]\n").unwrap();
        let rows = queue_prompt_projection_rows(content, &entries);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].projectable_default);
        assert!(!rows[1].projectable_default);
        assert_eq!(rows[1].id.as_deref(), Some("alpha"));
    }

    #[test]
    fn active_projection_and_retarget_use_queue_rows() {
        let content = "\
<!-- agent:queue -->
- do [#alpha]
- 🚧 do [#beta]
<!-- /agent:queue -->
";
        let entries = parse("- do [#alpha]\n- 🚧 do [#beta]\n").unwrap();
        let projection = active_queue_prompt_projection(content, &entries, &HashMap::new(), true);
        assert_eq!(projection.prompts, vec!["do [#beta]"]);
        assert!(projection.retargeted);
        assert!(in_progress_marker_retarget_requested(
            Some("+ - 🚧 do [#beta]"),
            content,
            &entries
        ));
    }

    #[test]
    fn queue_worklist_projection_preserves_node_keys_and_hashes_rendered_queue() {
        let content = "\
<!-- agent:queue -->
- do [#alpha]
- #preset
- @dispatch
<!-- /agent:queue -->
";
        let entries = vec![
            prompt("do [#alpha]"),
            QueueEntry::Preset("#preset".to_string()),
            QueueEntry::Dispatch("@dispatch".to_string()),
        ];
        let worklist = queue_worklist_entries(content, &entries);
        assert_eq!(worklist.len(), 3);
        assert_eq!(worklist[0].kind, QueueWorklistEntryKind::Prompt);
        assert_eq!(worklist[0].backlog_id.as_deref(), Some("alpha"));
        assert_eq!(worklist[0].node_key.as_deref(), Some("queue:0:alpha:0"));
        assert!(worklist[0].drainable);
        assert_eq!(worklist[1].kind, QueueWorklistEntryKind::Preset);
        assert_eq!(worklist[2].kind, QueueWorklistEntryKind::Dispatch);
        assert_eq!(
            queue_worklist_hash(&entries),
            agent_doc_hash::content_hash(&document_queue::render(&entries))
        );
    }

    #[test]
    fn dedup_queue_nodes_by_key_preserves_intentional_duplicate_prompts() {
        let content = "\
<!-- agent:queue -->
- do [#alpha]
- do [#alpha]
- do deploy
- do deploy
<!-- /agent:queue -->
";

        let deduped = dedup_queue_nodes_by_key(content).unwrap();

        assert!(
            deduped.is_none(),
            "intentional duplicate queue prompt text must not be collapsed"
        );
    }
}
