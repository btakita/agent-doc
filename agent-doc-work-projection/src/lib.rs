//! Document-specific projections of authoritative work-item sources.
//!
//! Parsing markdown components belongs here rather than in the source-agnostic
//! work-graph crate. The projection is pure text-in/text-out and performs no IO.

use agent_doc_element_backlog::backlog::{self, PendingState};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// One operator-authored resolution command attached to a projected active item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveResolutionCommand {
    pub identity: String,
    pub state: String,
    pub command: String,
}

/// One authoritative location contributing to the active-work index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedActiveItemSource {
    pub component: String,
    pub state: String,
}

/// One stable work identity projected into the managed active-work list.
/// Multiple source locations are folded into this one row, so a runnable queue
/// projection and its tracked-work owner cannot look like duplicate work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedActiveItem {
    pub identity: String,
    pub display_id: Option<String>,
    pub summary: String,
    pub sources: Vec<ManagedActiveItemSource>,
    group: String,
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

fn push_managed_active_item(
    items: &mut Vec<ManagedActiveItem>,
    indexes: &mut HashMap<String, usize>,
    identity: String,
    display_id: Option<String>,
    text: &str,
    source: ManagedActiveItemSource,
    summary_priority: u8,
) {
    if let Some(index) = indexes.get(&identity).copied() {
        let item = &mut items[index];
        let source_component = source.component.clone();
        if !item.sources.contains(&source) {
            item.sources.push(source);
        }
        if summary_priority < item.summary_priority {
            item.summary = summarize_text(text);
            item.group = source_component;
            item.summary_priority = summary_priority;
        }
        return;
    }
    indexes.insert(identity.clone(), items.len());
    let group = source.component.clone();
    items.push(ManagedActiveItem {
        identity,
        display_id,
        summary: summarize_text(text),
        sources: vec![source],
        group,
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

fn lifecycle_state(
    marker: agent_doc_document::queue_projection::QueueLifecycleMarker,
) -> &'static str {
    use agent_doc_document::queue_projection::QueueLifecycleMarker::*;
    match marker {
        Pending => "pending",
        Blocked => "blocked",
        Claimed => "claimed",
        Running => "running",
        Reported => "reported",
        Lost => "lost",
        LostClaimed => "lost-claimed",
        LostRunning => "lost-running",
        LostReported => "lost-reported",
        Reconciling => "reconciling",
        Unknown => "unknown",
        Skipped => "skipped",
        AwaitingOperator => "awaiting-operator",
    }
}

fn is_started_and_unclosed(
    marker: agent_doc_document::queue_projection::QueueLifecycleMarker,
) -> bool {
    use agent_doc_document::queue_projection::QueueLifecycleMarker::*;
    !matches!(marker, Pending | Blocked | Skipped)
}

/// Collect work that has started and has not closed from its authoritative source.
///
/// A source enters the index when its lifecycle marker proves work has started.
/// Once an identity is active, matching unfinished tracked sources are retained
/// as provenance even when only the queue projection carries the lifecycle
/// marker. Completed/struck rows, pending/blocked/skipped work, todos, queue
/// group labels, and the done archive are excluded.
pub fn managed_active_items(content: &str) -> Result<Vec<ManagedActiveItem>> {
    use agent_doc_element::element;
    use agent_doc_queue::document_queue::{self, QueueEntry};

    let components = element::parse(content).context("active projection: parse components")?;
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
            let Some(marker) =
                agent_doc_document::queue_projection::lifecycle_marker(prompt.text.trim())
                    .filter(|marker| is_started_and_unclosed(*marker))
            else {
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
            push_managed_active_item(
                &mut items,
                &mut indexes,
                identity,
                display_id,
                &summary,
                ManagedActiveItemSource {
                    component: "queue".to_string(),
                    state: lifecycle_state(marker).to_string(),
                },
                3,
            );
        }
    }

    // Establish active tracked identities before adding plain tracked aliases.
    // This makes the result independent of component order: for example, an
    // open backlog row can still be linked to a matching active review row.
    for component_name in ["backlog", "review", "icebox"] {
        for component in components.iter().filter(|component| {
            element::canonical_component_name(&component.name) == component_name
                || (component_name == "backlog" && component.name == "pending")
        }) {
            let (_, parsed, _) = backlog::parse_items(component.content(content));
            for item in parsed.into_iter().filter(|item| !item.is_done()) {
                let Some(marker) = item
                    .lifecycle_marker
                    .filter(|marker| is_started_and_unclosed(*marker))
                else {
                    continue;
                };
                push_managed_active_item(
                    &mut items,
                    &mut indexes,
                    item.id.clone(),
                    Some(item.id),
                    &item.text,
                    ManagedActiveItemSource {
                        component: component_name.to_string(),
                        state: lifecycle_state(marker).to_string(),
                    },
                    1,
                );
            }
        }
    }

    // Preserve every unfinished tracked location for an already-active
    // identity. These rows are provenance, not an independent reason to make
    // otherwise dormant work active.
    for component_name in ["backlog", "review", "icebox"] {
        for component in components.iter().filter(|component| {
            element::canonical_component_name(&component.name) == component_name
                || (component_name == "backlog" && component.name == "pending")
        }) {
            let (_, parsed, _) = backlog::parse_items(component.content(content));
            for item in parsed.into_iter().filter(|item| !item.is_done()) {
                if item.lifecycle_marker.is_some() || !indexes.contains_key(&item.id) {
                    continue;
                }
                let state = match item.state {
                    PendingState::Open => "open",
                    PendingState::Gated => "gated",
                    PendingState::Done => unreachable!("done tracked work was filtered"),
                };
                push_managed_active_item(
                    &mut items,
                    &mut indexes,
                    item.id.clone(),
                    Some(item.id),
                    &item.text,
                    ManagedActiveItemSource {
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

/// Render active work grouped by authoritative source. Empty groups are omitted.
pub fn render_managed_active_items(items: &[ManagedActiveItem]) -> String {
    let mut out = String::new();
    for component in ["queue", "backlog", "review", "icebox"] {
        let grouped = items
            .iter()
            .filter(|item| item.group == component)
            .collect::<Vec<_>>();
        if grouped.is_empty() {
            continue;
        }
        let title = match component {
            "queue" => "Queue",
            "backlog" => "Backlog",
            "review" => "Review",
            "icebox" => "Icebox",
            _ => unreachable!(),
        };
        out.push_str(&format!("- **{title}** ({})\n", grouped.len()));
        for item in grouped {
            let identity = item
                .display_id
                .as_deref()
                .map(|id| format!("[#{id}]"))
                .unwrap_or_else(|| item.identity.clone());
            out.push_str(&format!("  - `{identity}` — {}\n", item.summary));
            if let Some(primary) = item
                .sources
                .iter()
                .find(|source| source.component == item.group)
            {
                out.push_str(&format!("    - **Status:** {}\n", primary.state));
            }
            let sources = item
                .sources
                .iter()
                .filter(|source| source.component != item.group)
                .map(|source| format!("{} ({})", source.component, source.state))
                .collect::<Vec<_>>()
                .join("; ");
            if !sources.is_empty() {
                out.push_str(&format!("    - **Sources:** {sources}\n"));
            }
        }
    }
    out
}

fn active_item_identity(line: &str) -> Option<(usize, String)> {
    let indent = line.len().saturating_sub(line.trim_start().len());
    let rest = line.trim_start().strip_prefix("- `[#")?;
    let (identity, _) = rest.split_once("]` — ")?;
    (!identity.trim().is_empty()).then(|| (indent, identity.trim().to_ascii_lowercase()))
}

fn active_status(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix("- **Status:**")
        .map(|state| state.trim().to_string())
}

fn is_binary_active_child(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("- **Status:**") || trimmed.starts_with("- **Sources:**")
}

/// Preserve operator-authored descendants while regenerating binary metadata.
///
/// `Status` and `Sources` are owned projection rows. Every other descendant of
/// an active item is operator input, so replacing the projection must carry it
/// forward until the command is promoted or otherwise resolved.
fn merge_active_operator_children(existing: &str, rendered: &str) -> String {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<(usize, String)> = None;
    for line in existing.lines() {
        if let Some(item) = active_item_identity(line) {
            current = Some(item);
            continue;
        }
        let Some((item_indent, identity)) = current.as_ref() else {
            continue;
        };
        let indent = line.len().saturating_sub(line.trim_start().len());
        if line.trim().is_empty() {
            continue;
        }
        if indent <= *item_indent {
            current = None;
            continue;
        }
        if !is_binary_active_child(line) {
            children
                .entry(identity.clone())
                .or_default()
                .push(line.to_string());
        }
    }
    if children.is_empty() {
        return rendered.to_string();
    }

    let lines = rendered.lines().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        out.push_str(line);
        out.push('\n');
        let Some((item_indent, identity)) = active_item_identity(line) else {
            index += 1;
            continue;
        };
        index += 1;
        while index < lines.len() {
            let child = lines[index];
            let indent = child.len().saturating_sub(child.trim_start().len());
            if child.trim().is_empty() || indent <= item_indent {
                break;
            }
            out.push_str(child);
            out.push('\n');
            index += 1;
        }
        if let Some(preserved) = children.get(&identity) {
            for child in preserved {
                out.push_str(child);
                out.push('\n');
            }
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// Promote leaf resolution commands from `agent:active` into ordinary queue
/// prompts, returning the document with those commands consumed.
///
/// The active row remains a derived observer of its authoritative source. The
/// promoted prompt is deliberately free text (it names the stable identity
/// without `#`) so it cannot alias or steal the claim identity of the work it
/// asks the coordinator to resolve.
pub fn promote_active_resolution_commands(
    content: &str,
) -> Result<Option<(String, Vec<ActiveResolutionCommand>)>> {
    use agent_doc_element::element;

    let components =
        element::parse(content).context("active command promotion: parse components")?;
    let Some(active) = components.iter().find(|component| {
        element::canonical_component_name(&component.name) == element::ACTIVE_COMPONENT
    }) else {
        return Ok(None);
    };
    let body = active.content(content);
    let lines = body.lines().collect::<Vec<_>>();
    let mut commands = Vec::new();
    let mut kept = Vec::new();
    let mut current: Option<(usize, String, String)> = None;
    for line in lines {
        if let Some((indent, identity)) = active_item_identity(line) {
            current = Some((indent, identity, String::new()));
            kept.push(line);
            continue;
        }
        if let Some((item_indent, identity, state)) = current.as_mut() {
            let indent = line.len().saturating_sub(line.trim_start().len());
            if !line.trim().is_empty() && indent <= *item_indent {
                current = None;
            } else if let Some(found) = active_status(line) {
                *state = found;
            } else if indent > *item_indent
                && line
                    .trim()
                    .strip_prefix("- ")
                    .is_some_and(|command| command.trim().eq_ignore_ascii_case("close"))
            {
                let identity = identity.clone();
                let state = state.clone();
                commands.push(ActiveResolutionCommand {
                    identity,
                    state,
                    command: "close".to_string(),
                });
                continue;
            }
        }
        kept.push(line);
    }
    if commands.is_empty() {
        return Ok(None);
    }
    let mut new_body = kept.join("\n");
    new_body.push('\n');
    Ok(Some((active.replace_content(content, &new_body), commands)))
}

/// Re-derive and reconcile the binary-owned `agent:active` projection.
pub fn reconcile_managed_active_items(content: &str) -> Result<Option<String>> {
    let items = managed_active_items(content)?;
    let mut rendered = render_managed_active_items(&items);
    let components = agent_doc_element::element::parse(content)?;
    if let Some(active) = components.iter().find(|component| {
        agent_doc_element::element::canonical_component_name(&component.name)
            == agent_doc_element::element::ACTIVE_COMPONENT
    }) {
        rendered = merge_active_operator_children(active.content(content), &rendered);
    }
    agent_doc_document::status_projection::reconcile_active_component_content(content, &rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_includes_only_started_unclosed_work_and_groups_by_source() {
        let content = concat!(
            "## Status <!-- agent:status -->\nold prose\n<!-- /agent:status -->\n",
            "## Run <!-- agent:run -->\n",
            "- ### 🚧 do [#shared]\n",
            "- ### 📥 pending [#q-pending]\n",
            "- ### ⛔ blocked [#q-blocked]\n",
            "- ### group\n",
            "  - 🟡 child [#q-child]\n",
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
            "- [/] ☑️ [#review-gated] verify externally\n",
            "<!-- /agent:review -->\n",
            "## Icebox <!-- agent:icebox -->\n",
            "- [ ] [#icebox-open] parked idea\n",
            "<!-- /agent:icebox -->\n",
        );

        let items = managed_active_items(content).unwrap();
        assert_eq!(items.len(), 3);
        let shared = items.iter().find(|item| item.identity == "shared").unwrap();
        assert_eq!(shared.summary, "authoritative tracked summary");
        assert_eq!(
            shared.sources,
            vec![
                ManagedActiveItemSource {
                    component: "queue".into(),
                    state: "running".into(),
                },
                ManagedActiveItemSource {
                    component: "backlog".into(),
                    state: "open".into(),
                },
            ]
        );
        assert!(items.iter().any(|item| item.identity == "q-child"));
        assert!(!items.iter().any(|item| item.identity == "q-pending"));
        assert!(!items.iter().any(|item| item.identity == "q-blocked"));
        assert!(!items.iter().any(|item| item.identity == "q-done"));
        assert!(!items.iter().any(|item| item.identity == "todo-done"));
        assert!(!items.iter().any(|item| item.identity == "backlog-done"));

        let rendered = render_managed_active_items(&items);
        assert!(rendered.starts_with("- **Queue** (1)\n"));
        assert!(rendered.contains("- **Backlog** (1)\n"));
        assert!(rendered.contains("- **Review** (1)\n"));
        assert!(!rendered.contains("- **Icebox**"));
        assert!(rendered.contains("`[#shared]` — authoritative tracked summary"));
        assert!(rendered.contains("`[#q-child]` — child"));
        assert!(rendered.contains("**Status:** claimed"));
        assert!(rendered.contains("**Status:** open"));
        assert!(rendered.contains("**Sources:** queue (running)"));
        assert!(!rendered.contains("**Sources:** backlog"));
        assert!(!rendered.contains("**Sources:** queue (claimed)"));
        assert!(!rendered.contains("`[#todo-gated]` — waiting action"));
        assert!(!rendered.contains("**Sources:** todo"));
    }

    #[test]
    fn reconciliation_is_idempotent_and_removes_stale_rows() {
        let doc = concat!(
            "## Status <!-- agent:status sync=\"release\" -->\n",
            "operator prose\n",
            "<!-- status:sync -->\nrelease prose\n<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n",
            "## Backlog <!-- agent:backlog -->\n",
            "- [ ] 🚧 [#live] still active\n",
            "<!-- /agent:backlog -->\n",
        );
        let one = reconcile_managed_active_items(doc)
            .unwrap()
            .expect("projection inserted");
        assert!(one.contains("<!-- agent:active priority -->"));
        assert!(one.contains("`[#live]` — still active"));
        assert!(one.contains("operator prose"));
        assert!(one.contains("<!-- status:sync -->\nrelease prose\n<!-- /status:sync -->"));
        assert!(reconcile_managed_active_items(&one).unwrap().is_none());

        let closed = one.replace(
            "- [ ] 🚧 [#live] still active",
            "- [x] 🚧 [#live] still active",
        );
        let updated = reconcile_managed_active_items(&closed)
            .unwrap()
            .expect("stale row removed");
        assert!(updated.contains("<!-- agent:active priority -->"));
        assert!(!updated.contains("- **Queue**"));
        assert!(!updated.contains("- **Backlog**"));
        assert!(!updated.contains("`[#live]`"));
    }

    #[test]
    fn active_projection_preserves_then_promotes_operator_close_command() {
        let content = concat!(
            "## Status <!-- agent:status -->\n<!-- /agent:status -->\n",
            "## Active <!-- agent:active priority -->\n",
            "- **Queue** (1)\n",
            "  - `[#lost-work]` — repair the lost attempt\n",
            "    - **Status:** lost\n",
            "    - close\n",
            "<!-- /agent:active -->\n",
            "## Run <!-- agent:run go -->\n",
            "- ⚠️ do [#lost-work]\n",
            "<!-- /agent:run -->\n",
        );

        let reconciled = reconcile_managed_active_items(content)
            .unwrap()
            .expect("source summary changes while the operator command survives");
        assert!(reconciled.contains("    - **Status:** lost\n    - close\n"));

        let (promoted, commands) = promote_active_resolution_commands(&reconciled)
            .unwrap()
            .expect("close command is promoted");
        assert_eq!(
            commands,
            vec![ActiveResolutionCommand {
                identity: "lost-work".into(),
                state: "lost".into(),
                command: "close".into(),
            }]
        );
        assert!(!promoted.contains("    - close\n"));
        assert!(promoted.contains("    - **Status:** lost\n"));
    }

    #[test]
    fn reported_queue_row_refreshes_stale_active_lifecycle() {
        let content = concat!(
            "## Active <!-- agent:active priority -->\n",
            "- **Backlog** (1)\n",
            "  - `[#work]` — the work\n",
            "    - **Status:** open\n",
            "    - **Sources:** queue (running)\n",
            "<!-- /agent:active -->\n",
            "<!-- agent:run subagents -->\n",
            "- ### ☑️ [#subagent:work#1] do [#work]\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#work] the work\n",
            "<!-- /agent:backlog -->\n",
        );

        let reconciled = reconcile_managed_active_items(content)
            .unwrap()
            .expect("reported lifecycle replaces stale running metadata");
        assert!(reconciled.contains("    - **Sources:** queue (reported)\n"));
        assert!(!reconciled.contains("queue (running)"));
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

        let items = managed_active_items(&content).unwrap();
        assert!(
            items.is_empty(),
            "non-executable queue ancestry must not remain in status: {items:#?}"
        );

        let reconciled = reconcile_managed_active_items(&content)
            .unwrap()
            .expect("stale queue-free status row must be removed");
        assert!(reconciled.contains("<!-- agent:active priority -->"));
        assert!(!reconciled.contains("- **Queue**"));
        assert!(!reconciled.contains(&stale_identity));
    }
}
