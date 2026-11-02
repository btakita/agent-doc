//! Document-specific projections of authoritative work-item sources.
//!
//! Parsing markdown components belongs here rather than in the source-agnostic
//! work-graph crate. The projection is pure text-in/text-out and performs no IO.

use agent_doc_element_backlog::backlog::{self, PendingState};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// One authoritative location contributing to the open-items index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedOpenItemSource {
    pub component: String,
    pub state: String,
}

/// One stable work identity projected into the managed nested list in status.
/// Multiple source locations are folded into this one row, so a runnable queue
/// projection and its tracked-work owner cannot look like duplicate work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedOpenItem {
    pub identity: String,
    pub display_id: Option<String>,
    pub summary: String,
    pub sources: Vec<ManagedOpenItemSource>,
    summary_priority: u8,
}

fn summarize_text(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let bounded: String = first.chars().take(100).collect();
    if first.chars().count() > 100 {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn push_managed_open_item(
    items: &mut Vec<ManagedOpenItem>,
    indexes: &mut HashMap<String, usize>,
    identity: String,
    display_id: Option<String>,
    text: &str,
    source: ManagedOpenItemSource,
    summary_priority: u8,
) {
    if let Some(index) = indexes.get(&identity).copied() {
        let item = &mut items[index];
        if !item.sources.contains(&source) {
            item.sources.push(source);
        }
        if summary_priority < item.summary_priority {
            item.summary = summarize_text(text);
            item.summary_priority = summary_priority;
        }
        return;
    }
    indexes.insert(identity.clone(), items.len());
    items.push(ManagedOpenItem {
        identity,
        display_id,
        summary: summarize_text(text),
        sources: vec![source],
        summary_priority,
    });
}

fn source_free_text_identity(component: &str, text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "{component}-free:{}",
        agent_doc_hash::short_content_hash(&normalized)
    )
}

/// Collect every unfinished managed work item from its authoritative source.
///
/// Runnable queue leaves, open/gated todos, and open/gated tracked work are
/// included. Completed/struck rows, queue group labels, and the done archive
/// are excluded. Shared ids fold into one row with all source states retained.
pub fn managed_open_items(content: &str) -> Result<Vec<ManagedOpenItem>> {
    use agent_doc_element::element;
    use agent_doc_element::todo::{TodoState, parse_todo_line};
    use agent_doc_queue::document_queue::{self, QueueEntry};

    let components = element::parse(content).context("status open items: parse components")?;
    let mut items = Vec::new();
    let mut indexes = HashMap::new();

    // Queue order is operational priority, so queue identities establish the
    // outer ordering. Parent/group labels are intentionally excluded by the
    // canonical executable-leaf selector.
    let queue_set = agent_doc_queue::queue_set::QueueSet::parse(content, &components)?;
    for block in queue_set.blocks() {
        let entries =
            agent_doc_queue::nested_claim_steer::expand_slice_if_installed(&block.entries);
        for index in document_queue::executable_prompt_indexes(entries.as_ref()) {
            let Some(QueueEntry::Prompt(prompt)) = entries.get(index) else {
                continue;
            };
            let prompt_text =
                agent_doc_document::queue_projection::strip_in_progress_marker(prompt.text.trim());
            if prompt_text.trim().is_empty() {
                continue;
            }
            let display_id = agent_doc_element_queue::queue_prompt_stable_id(&prompt_text);
            let identity = display_id.clone().unwrap_or_else(|| {
                source_free_text_identity(
                    "queue",
                    &agent_doc_element_queue::normalize_multiline_dedup_text(&prompt_text),
                )
            });
            let summary = agent_doc_element_queue::strip_queue_prompt_stable_id_annotation(
                &agent_doc_element_queue::strip_priority_markers(&prompt_text),
            );
            push_managed_open_item(
                &mut items,
                &mut indexes,
                identity,
                display_id,
                &summary,
                ManagedOpenItemSource {
                    component: "queue".to_string(),
                    state: "open".to_string(),
                },
                3,
            );
        }
    }

    for component in components
        .iter()
        .filter(|component| component.name == "todo")
    {
        for line in component.content(content).lines() {
            let Some(todo) = parse_todo_line(line).filter(|todo| todo.state.is_open()) else {
                continue;
            };
            let display_id = todo.own_id.map(str::to_ascii_lowercase);
            let identity = display_id
                .clone()
                .unwrap_or_else(|| source_free_text_identity("todo", todo.action));
            let state = match todo.state {
                TodoState::Open => "open",
                TodoState::Gated => "gated",
                TodoState::Done => unreachable!("done todos were filtered"),
            };
            push_managed_open_item(
                &mut items,
                &mut indexes,
                identity,
                display_id,
                todo.action,
                ManagedOpenItemSource {
                    component: "todo".to_string(),
                    state: state.to_string(),
                },
                2,
            );
        }
    }

    for component_name in ["backlog", "review", "icebox"] {
        for component in components.iter().filter(|component| {
            element::canonical_component_name(&component.name) == component_name
                || (component_name == "backlog" && component.name == "pending")
        }) {
            let (_, parsed, _) = backlog::parse_items(component.content(content));
            for item in parsed.into_iter().filter(|item| !item.is_done()) {
                let state = match item.state {
                    PendingState::Open => "open",
                    PendingState::Gated => "gated",
                    PendingState::Done => unreachable!("done tracked work was filtered"),
                };
                push_managed_open_item(
                    &mut items,
                    &mut indexes,
                    item.id.clone(),
                    Some(item.id),
                    &item.text,
                    ManagedOpenItemSource {
                        component: component_name.to_string(),
                        state: state.to_string(),
                    },
                    1,
                );
            }
        }
    }
    Ok(items)
}

/// Render the open-items index as one deterministic managed nested list.
pub fn render_managed_open_items(items: &[ManagedOpenItem]) -> String {
    let mut out = format!("- **Open items** ({})\n", items.len());
    for item in items {
        let identity = item
            .display_id
            .as_deref()
            .map(|id| format!("[#{id}]"))
            .unwrap_or_else(|| item.identity.clone());
        out.push_str(&format!("  - `{identity}` — {}\n", item.summary));
        let sources = item
            .sources
            .iter()
            .map(|source| format!("{} ({})", source.component, source.state))
            .collect::<Vec<_>>()
            .join("; ");
        out.push_str(&format!("    - **Sources:** {sources}\n"));
    }
    out
}

/// Re-derive and reconcile the binary-owned `agent:open-items` projection.
pub fn reconcile_managed_open_items(content: &str) -> Result<Option<String>> {
    let items = managed_open_items(content)?;
    let rendered = render_managed_open_items(&items);
    agent_doc_document::status_projection::reconcile_open_items_component_content(
        content, &rendered,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_includes_all_open_sources_and_folds_shared_identity() {
        let content = concat!(
            "## Status <!-- agent:status -->\nold prose\n<!-- /agent:status -->\n",
            "## Run <!-- agent:run -->\n",
            "- ### 🚧 do [#shared]\n",
            "- ### group\n",
            "  - child [#q-child]\n",
            "- ### ~~done [#q-done]~~\n",
            "<!-- /agent:run -->\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#todo-open] operator action\n",
            "- [/] [#todo-gated] waiting action\n",
            "- [x] [#todo-done] finished action\n",
            "<!-- /agent:todo -->\n",
            "## Backlog <!-- agent:backlog -->\n",
            "- [ ] [#shared] authoritative tracked summary\n",
            "- [x] [#backlog-done] done\n",
            "<!-- /agent:backlog -->\n",
            "## Review <!-- agent:review -->\n",
            "- [/] [#review-gated] verify externally\n",
            "<!-- /agent:review -->\n",
            "## Icebox <!-- agent:icebox -->\n",
            "- [ ] [#icebox-open] parked idea\n",
            "<!-- /agent:icebox -->\n",
        );

        let items = managed_open_items(content).unwrap();
        assert_eq!(items.len(), 6);
        let shared = items.iter().find(|item| item.identity == "shared").unwrap();
        assert_eq!(shared.summary, "authoritative tracked summary");
        assert_eq!(
            shared.sources,
            vec![
                ManagedOpenItemSource {
                    component: "queue".into(),
                    state: "open".into(),
                },
                ManagedOpenItemSource {
                    component: "backlog".into(),
                    state: "open".into(),
                },
            ]
        );
        assert!(items.iter().any(|item| item.identity == "q-child"));
        assert!(!items.iter().any(|item| item.identity == "q-done"));
        assert!(!items.iter().any(|item| item.identity == "todo-done"));
        assert!(!items.iter().any(|item| item.identity == "backlog-done"));

        let rendered = render_managed_open_items(&items);
        assert!(rendered.starts_with("- **Open items** (6)\n"));
        assert!(rendered.contains("`[#shared]` — authoritative tracked summary"));
        assert!(rendered.contains("**Sources:** queue (open); backlog (open)"));
        assert!(rendered.contains("`[#todo-gated]` — waiting action"));
        assert!(rendered.contains("**Sources:** todo (gated)"));
    }

    #[test]
    fn reconciliation_is_idempotent_and_removes_stale_rows() {
        let doc = concat!(
            "## Status <!-- agent:status sync=\"release\" -->\n",
            "operator prose\n",
            "<!-- status:sync -->\nrelease prose\n<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n",
            "## Backlog <!-- agent:backlog -->\n",
            "- [ ] [#live] still open\n",
            "<!-- /agent:backlog -->\n",
        );
        let one = reconcile_managed_open_items(doc)
            .unwrap()
            .expect("projection inserted");
        assert!(one.contains("<!-- agent:open-items -->"));
        assert!(one.contains("`[#live]` — still open"));
        assert!(one.contains("operator prose"));
        assert!(one.contains("<!-- status:sync -->\nrelease prose\n<!-- /status:sync -->"));
        assert!(reconcile_managed_open_items(&one).unwrap().is_none());

        let closed = one.replace("- [ ] [#live] still open", "- [x] [#live] still open");
        let updated = reconcile_managed_open_items(&closed)
            .unwrap()
            .expect("stale row removed");
        assert!(updated.contains("- **Open items** (0)"));
        assert!(!updated.contains("`[#live]`"));
    }

    #[test]
    fn status_drops_queue_free_identity_after_executable_leaf_is_consumed() {
        let stale_identity =
            source_free_text_identity("queue", "Complete the remaining work for the parent item.");
        let content = format!(
            concat!(
                "## Status <!-- agent:status -->\n",
                "<!-- status:open-items -->\n",
                "- **Open items** (1)\n",
                "  - `{}` — Complete the remaining work for the parent item.\n",
                "    - **Sources:** queue (open)\n",
                "<!-- /status:open-items -->\n",
                "<!-- /agent:status -->\n",
                "## Run <!-- agent:run -->\n",
                "- Waiting on you — answer by adding a child line under an item\n",
                "  - [#ctg7] Verify the live pane transition\n",
                "    - ~~Complete the remaining work for the parent item.~~\n",
                "<!-- /agent:run -->\n",
            ),
            stale_identity
        );

        let items = managed_open_items(&content).unwrap();
        assert!(
            items.is_empty(),
            "non-executable queue ancestry must not remain in status: {items:#?}"
        );

        let reconciled = reconcile_managed_open_items(&content)
            .unwrap()
            .expect("stale queue-free status row must be removed");
        assert!(reconciled.contains("- **Open items** (0)"));
        assert!(!reconciled.contains(&stale_identity));
    }
}
