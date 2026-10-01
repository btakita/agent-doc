//! Pure queue projection helpers over parsed queue entries.
//!
//! Callers provide document text and parsed queue entries. File IO, state-event
//! persistence, and ops logging stay in orchestration.

use std::collections::HashMap;

use agent_doc_document::queue_projection::{
    ActiveQueuePromptProjection, QueuePromptRow, strip_in_progress_marker, strip_priority_markers,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedQueueHeadProjection {
    pub text: String,
    pub node_key: String,
    pub index: usize,
    pub backlog_id: Option<String>,
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
    let deferred_ids = crate::queue_continuation::deferred_backlog_ids_split(
        content,
        crate::queue_continuation::DrainScope::InSessionLoop,
    );
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
                let projectable_default = !crate::queue_continuation::is_noise_queue_head(
                    &text,
                    preset_supplies_directive,
                ) && !id.as_ref().is_some_and(|id| {
                    // `#opverifyanswered`: an answered operator-verify head is
                    // live work again, so it stays projectable.
                    deferred_ids.defers(
                        id,
                        crate::queue_continuation::head_carries_operator_verdict(&text),
                    )
                });
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
    skipped_ids: &std::collections::HashSet<String>,
) -> ActiveQueuePromptProjection {
    let rows = queue_prompt_projection_rows(content, entries);
    agent_doc_document::queue_projection::project_active_queue_prompts(
        &rows,
        deps,
        honor_in_progress_markers,
        skipped_ids,
    )
}

pub fn in_progress_marker_retarget_requested(
    diff: Option<&str>,
    content: &str,
    entries: &[QueueEntry],
    binary_projected: &std::collections::HashSet<String>,
) -> bool {
    let rows = queue_prompt_projection_rows(content, entries);
    agent_doc_document::queue_projection::in_progress_marker_retarget_requested(
        diff,
        &rows,
        binary_projected,
    )
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

/// Project authoritative struck queue rows into terminal queue-head identities.
///
/// A queue row can become struck through response capture or an editor-origin
/// mutation without passing through the direct queue-consumption proof path.
/// The durable node key survives the strike, so orchestration can record the
/// same terminal lifecycle fact before later maintenance considers selections.
pub fn completed_queue_head_projections(
    content: &str,
) -> Result<Vec<CompletedQueueHeadProjection>> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
        anyhow::anyhow!("queue completion projection: failed to parse queue nodes: {err}")
    })?;
    Ok(nodes
        .into_iter()
        .filter(|node| node.item.struck)
        .filter_map(|node| {
            let text = strip_in_progress_marker(&strip_priority_markers(node.item.text.trim()))
                .trim()
                .to_string();
            if text.is_empty() {
                return None;
            }
            Some(CompletedQueueHeadProjection {
                backlog_id: crate::queue_response::queue_prompt_done_id(&text),
                text,
                node_key: node.node_key,
                index: node.index,
            })
        })
        .collect())
}

pub fn queue_worklist_hash(entries: &[QueueEntry]) -> String {
    agent_doc_hash::content_hash(&document_queue::render(entries))
}

/// Hash only the live queue component so unrelated document edits do not
/// manufacture a new queue lifecycle generation.
pub fn queue_worklist_hash_for_document(content: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    let entries = document_queue::parse(&content[queue.open_end..queue.close_start]).ok()?;
    Some(queue_worklist_hash(&entries))
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

/// `#releaseskipcarried`: ids whose `⏭️` skip marker the operator removed.
///
/// `diff` is the snapshot→document diff, so a removed line carrying `⏭️` paired
/// with an added line for the same `#id` without it is an operator edit, not
/// preflight's own marker maintenance. That edit is the operator overruling the
/// stall claim behind the carried skip; without honoring it, a head that is
/// never "consumed" (a bare `#release` with no backlog item) is re-stamped
/// every cycle and can only be unstuck by rewriting the line.
pub fn operator_unskipped_queue_ids(diff: Option<&str>) -> std::collections::HashSet<String> {
    let Some(diff) = diff else {
        return std::collections::HashSet::new();
    };
    let mut removed_skipped = std::collections::HashSet::new();
    let mut added_unskipped = std::collections::HashSet::new();
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        let (body, added) = if let Some(body) = line.strip_prefix('+') {
            (body, true)
        } else if let Some(body) = line.strip_prefix('-') {
            (body, false)
        } else {
            continue;
        };
        let Some(id) = crate::queue_response::queue_prompt_done_id(body) else {
            continue;
        };
        let skipped = body.contains(agent_doc_document::queue_projection::SKIP_MARKER);
        match (added, skipped) {
            (false, true) => {
                removed_skipped.insert(id);
            }
            (true, false) => {
                added_unskipped.insert(id);
            }
            _ => {}
        }
    }
    removed_skipped
        .intersection(&added_unskipped)
        .cloned()
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
            indent: 0,
            ordered_marker: None,
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
        let projection = active_queue_prompt_projection(
            content,
            &entries,
            &HashMap::new(),
            true,
            &std::collections::HashSet::new(),
        );
        assert_eq!(projection.prompts, vec!["do [#beta]"]);
        assert!(projection.retargeted);
        assert!(in_progress_marker_retarget_requested(
            Some("+ - 🚧 do [#beta]"),
            content,
            &entries,
            &std::collections::HashSet::new()
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
    fn document_queue_hash_ignores_exchange_edits_but_tracks_queue_edits() {
        let first = "\
<!-- agent:queue -->
- do [#alpha]
<!-- /agent:queue -->
<!-- agent:exchange -->
first
<!-- /agent:exchange -->
";
        let exchange_edit = first.replace("first", "second");
        let queue_edit = first.replace("alpha", "beta");

        assert_eq!(
            queue_worklist_hash_for_document(first),
            queue_worklist_hash_for_document(&exchange_edit)
        );
        assert_ne!(
            queue_worklist_hash_for_document(first),
            queue_worklist_hash_for_document(&queue_edit)
        );
    }

    #[test]
    fn completed_projection_uses_durable_identity_for_only_struck_rows() {
        let content = "\
<!-- agent:queue -->
- ~~🚧 do [#completed-work]~~
- do [#ready-work]
<!-- /agent:queue -->
";

        let completed = completed_queue_head_projections(content).unwrap();

        assert_eq!(
            completed,
            vec![CompletedQueueHeadProjection {
                text: "do [#completed-work]".to_string(),
                node_key: "queue:0:completed-work:0".to_string(),
                index: 0,
                backlog_id: Some("completed-work".to_string()),
            }]
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

    #[test]
    fn operator_removing_a_skip_marker_is_an_unskip() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,2 @@\n-- \u{23ed}\u{fe0f} :pin: #release\n+- :pin: #release\n - do [#other]\n";
        assert_eq!(
            operator_unskipped_queue_ids(Some(diff)),
            ["release".to_string()].into_iter().collect()
        );
        // A moved line that keeps its marker, a bare removal, or no diff is not.
        let moved = "-- \u{23ed}\u{fe0f} do [#a]\n+- \u{23ed}\u{fe0f} do [#a]\n-- \u{23ed}\u{fe0f} do [#b]\n";
        assert!(operator_unskipped_queue_ids(Some(moved)).is_empty());
        assert!(operator_unskipped_queue_ids(None).is_empty());
    }
}
