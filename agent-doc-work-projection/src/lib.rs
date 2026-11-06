//! Document-specific projections of authoritative work-item sources.
//!
//! Parsing markdown components belongs here rather than in the source-agnostic
//! work-graph crate. The projection is pure text-in/text-out and performs no IO.

pub mod active_lifecycle;
mod active_summary;
pub mod managed_list;
pub mod tracked_lists;

use active_summary::summarize_text;
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
    /// `#managedlistnodrift`: the `**Age:**` child of a needs-integration
    /// entry rendered from durable state.
    pub age: Option<String>,
    group: String,
    summary_priority: u8,
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
        age: None,
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
/// A queue row enters the index when its lifecycle marker proves work has
/// started. Once an identity is active, matching unfinished tracked sources are
/// retained as provenance; tracked-work rows never carry a lifecycle marker of
/// their own (`#backlogrowmarker`), so they never start an identity. Completed/struck rows, pending/blocked/skipped work, todos, queue
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

    // Preserve every unfinished tracked location for an already-active
    // identity. These rows are provenance, not an independent reason to make
    // otherwise dormant work active. `#backlogrowmarker`: a tracked row carries
    // no attempt marker of its own, so its source reports the lifecycle the
    // queue row projects for the same identity.
    for component_name in ["backlog", "review", "icebox"] {
        for component in components.iter().filter(|component| {
            element::canonical_component_name(&component.name) == component_name
                || (component_name == "backlog" && component.name == "pending")
        }) {
            let (_, parsed, _) = backlog::parse_items(component.content(content));
            for item in parsed.into_iter().filter(|item| !item.is_done()) {
                let Some(&index) = indexes.get(&item.id) else {
                    continue;
                };
                let Some(state) = items[index]
                    .sources
                    .iter()
                    .find(|source| source.component == "queue")
                    .map(|source| source.state.clone())
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
                        state,
                    },
                    1,
                );
            }
        }
    }
    Ok(items)
}

/// `#managedlistnodrift`: active work rendered from durable lifecycle state.
///
/// An entry exists exactly when a live executable queue row joins a durable
/// attempt in `lifecycle` by work identity; the row's visible lifecycle
/// marker is never read. Response-packet rows under a Waiting-on-you item are
/// decision evidence, never work, so an attempt dispatched onto one is not
/// listed. Entries group under `Running` and `Needs integration`; unfinished
/// backlog/review/icebox locations of a listed identity are provenance.
pub fn managed_active_items_durable(
    content: &str,
    lifecycle: &active_lifecycle::ActiveLifecycle,
) -> Result<Vec<ManagedActiveItem>> {
    use agent_doc_element::element;
    use agent_doc_queue::document_queue::{self, QueueEntry};

    let components = element::parse(content).context("active projection: parse components")?;
    let mut items: Vec<ManagedActiveItem> = Vec::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();
    let queue_set = agent_doc_queue::queue_set::QueueSet::parse(content, &components)?;
    for block in queue_set.blocks() {
        let entries =
            agent_doc_queue::nested_claim_steer::expand_slice_if_installed(&block.entries);
        let entries = entries.as_ref();
        for index in document_queue::executable_prompt_indexes(entries) {
            let Some(QueueEntry::Prompt(prompt)) = entries.get(index) else {
                continue;
            };
            if agent_doc_queue::waiting_packet::is_packet_row(entries, index) {
                continue;
            }
            let identity = agent_doc_queue::queue_claim::claim_identity_at(entries, index)
                .unwrap_or_else(|| agent_doc_queue::queue_claim::claim_identity(&prompt.text));
            let Some(attempt) = lifecycle.attempts.get(&identity) else {
                continue;
            };
            let prompt_text =
                agent_doc_document::queue_projection::strip_in_progress_marker(prompt.text.trim());
            if prompt_text.trim().is_empty() {
                continue;
            }
            let display_id = agent_doc_element_queue::queue_prompt_stable_id(&prompt_text);
            let key = display_id.clone().unwrap_or_else(|| {
                source_free_text_identity(
                    "queue",
                    &agent_doc_element_queue::normalize_multiline_dedup_text(&prompt_text),
                )
            });
            if indexes.contains_key(&key) {
                continue;
            }
            let group = active_lifecycle::ActiveGroup::of_marker(attempt.marker);
            let mut state = lifecycle_state(attempt.marker).to_string();
            if attempt.integrated {
                state.push_str(" (integrated; closeout pending)");
            }
            let summary = agent_doc_element_queue::strip_queue_prompt_stable_id_annotation(
                &agent_doc_element_queue::strip_priority_markers(&prompt_text),
            );
            indexes.insert(key.clone(), items.len());
            items.push(ManagedActiveItem {
                identity: key,
                display_id,
                summary: summarize_text(&summary),
                sources: vec![ManagedActiveItemSource {
                    component: "queue".to_string(),
                    state,
                }],
                age: (group == active_lifecycle::ActiveGroup::NeedsIntegration)
                    .then(|| active_lifecycle::age_line(attempt)),
                group: group.key().to_string(),
                summary_priority: 3,
            });
        }
    }
    for component_name in ["backlog", "review", "icebox"] {
        for component in components.iter().filter(|component| {
            element::canonical_component_name(&component.name) == component_name
                || (component_name == "backlog" && component.name == "pending")
        }) {
            let (_, parsed, _) = backlog::parse_items(component.content(content));
            for item in parsed.into_iter().filter(|item| !item.is_done()) {
                let Some(index) = indexes.get(&item.id).copied() else {
                    continue;
                };
                let state = match item.state {
                    PendingState::Open => "open",
                    PendingState::Gated => "gated",
                    PendingState::Done => unreachable!("done tracked work was filtered"),
                };
                let entry = &mut items[index];
                let source = ManagedActiveItemSource {
                    component: component_name.to_string(),
                    state: state.to_string(),
                };
                if !entry.sources.contains(&source) {
                    entry.sources.push(source);
                }
                if entry.summary_priority > 1 {
                    entry.summary = summarize_text(&item.text);
                    entry.summary_priority = 1;
                }
            }
        }
    }
    Ok(items)
}

/// Render active work grouped by authoritative source. Empty groups are omitted.
pub fn render_managed_active_items(items: &[ManagedActiveItem]) -> String {
    let mut out = String::new();
    for component in [
        "running",
        "lost",
        "quota-blocked",
        "needs-integration",
        "queue",
        "backlog",
        "review",
        "icebox",
    ] {
        let grouped = items
            .iter()
            .filter(|item| item.group == component)
            .collect::<Vec<_>>();
        if grouped.is_empty() {
            continue;
        }
        let title = match component {
            "running" => active_lifecycle::ActiveGroup::Running.title(),
            "lost" => active_lifecycle::ActiveGroup::Lost.title(),
            "quota-blocked" => active_lifecycle::ActiveGroup::QuotaBlocked.title(),
            "needs-integration" => active_lifecycle::ActiveGroup::NeedsIntegration.title(),
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
            let primary_component = if matches!(
                item.group.as_str(),
                "running" | "lost" | "quota-blocked" | "needs-integration"
            ) {
                "queue"
            } else {
                item.group.as_str()
            };
            if let Some(primary) = item
                .sources
                .iter()
                .find(|source| source.component == primary_component)
            {
                out.push_str(&format!("    - **Status:** {}\n", primary.state));
            }
            if let Some(age) = &item.age {
                out.push_str(&format!("    - {age}\n"));
            }
            let sources = item
                .sources
                .iter()
                .filter(|source| source.component != primary_component)
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
    trimmed.starts_with("- **Status:**")
        || trimmed.starts_with("- **Sources:**")
        || trimmed.starts_with("- **Age:**")
}

fn is_binary_active_group(line: &str) -> bool {
    let trimmed = line.trim();
    [
        "Running",
        "Lost",
        "Blocked by quota",
        "Needs integration",
        "Queue",
        "Backlog",
        "Review",
        "Icebox",
    ]
    .iter()
    .any(|group| trimmed.starts_with(&format!("- **{group}** (")))
}

fn is_binary_active_item(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("- `") && trimmed.contains("` — ")
}

/// Exact binary-owned rows currently materialized in `agent:active`.
///
/// The component heading and every non-binary descendant are operator-owned,
/// so retained-delivery landing checks compare this signature instead of the
/// whole component body. `None` distinguishes a missing projection component
/// from a present, correctly empty one.
pub fn managed_active_projection_signature(content: &str) -> Result<Option<Vec<String>>> {
    let components = agent_doc_element::element::parse(content)?;
    Ok(components
        .iter()
        .find(|component| {
            agent_doc_element::element::canonical_component_name(&component.name)
                == agent_doc_element::element::ACTIVE_COMPONENT
        })
        .map(|active| {
            active
                .content(content)
                .lines()
                .filter(|line| {
                    is_binary_active_group(line)
                        || is_binary_active_item(line)
                        || is_binary_active_child(line)
                })
                .map(str::trim)
                .map(str::to_string)
                .collect()
        }))
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

/// `#managedlistnodrift`: re-derive `agent:active` for `file` from durable
/// lifecycle state through the installed loader
/// ([`active_lifecycle::install_loader`]). Every production caller uses this.
/// When the durable state cannot be read the component is left unchanged
/// (`Ok(None)`): an uncertain ledger never erases or invents active work.
/// Without an installed loader (unit tests of crates that never start the
/// binary) it falls back to [`reconcile_managed_active_items`].
pub fn reconcile_managed_active_items_for(
    file: &std::path::Path,
    content: &str,
) -> Result<Option<String>> {
    let read = active_lifecycle::read(file);
    if let active_lifecycle::ActiveLifecycleRead::Unreadable(err) = &read {
        eprintln!(
            "[active] durable lifecycle unreadable for {} ({err}); agent:active left unchanged",
            file.display()
        );
    }
    reconcile_managed_active_items_from_read(content, &read)
}

/// [`reconcile_managed_active_items_for`] over an already-captured read.
pub fn reconcile_managed_active_items_from_read(
    content: &str,
    read: &active_lifecycle::ActiveLifecycleRead,
) -> Result<Option<String>> {
    match read {
        active_lifecycle::ActiveLifecycleRead::NoLoader => reconcile_managed_active_items(content),
        active_lifecycle::ActiveLifecycleRead::Loaded(lifecycle) => {
            reconcile_managed_active_items_with(content, lifecycle)
        }
        active_lifecycle::ActiveLifecycleRead::Unreadable(_) => Ok(None),
    }
}

/// [`reconcile_managed_active_items_for`] with an explicit lifecycle.
pub fn reconcile_managed_active_items_with(
    content: &str,
    lifecycle: &active_lifecycle::ActiveLifecycle,
) -> Result<Option<String>> {
    let items = managed_active_items_durable(content, lifecycle)?;
    reconcile_rendered_active_items(content, &items)
}

/// Re-derive and reconcile the binary-owned `agent:active` projection from
/// the queue markers in `content` (the pre-`#managedlistnodrift` derivation,
/// kept only for callers with no installed durable loader).
pub fn reconcile_managed_active_items(content: &str) -> Result<Option<String>> {
    let items = managed_active_items(content)?;
    reconcile_rendered_active_items(content, &items)
}

fn reconcile_rendered_active_items(
    content: &str,
    items: &[ManagedActiveItem],
) -> Result<Option<String>> {
    let mut rendered = render_managed_active_items(items);
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
            // `#backlogrowmarker`: a stray attempt marker on a tracked row
            // never starts an identity; only queue rows do.
            "- [/] ☑️ [#review-gated] verify externally\n",
            "<!-- /agent:review -->\n",
            "## Icebox <!-- agent:icebox -->\n",
            "- [ ] [#icebox-open] parked idea\n",
            "<!-- /agent:icebox -->\n",
        );

        let items = managed_active_items(content).unwrap();
        assert_eq!(items.len(), 2);
        assert!(!items.iter().any(|item| item.identity == "review-gated"));
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
                    state: "running".into(),
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
        assert!(!rendered.contains("- **Review**"));
        assert!(!rendered.contains("- **Icebox**"));
        assert!(rendered.contains("`[#shared]` — authoritative tracked summary"));
        assert!(rendered.contains("`[#q-child]` — child"));
        assert!(rendered.contains("**Status:** claimed"));
        assert!(rendered.contains("**Status:** running"));
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
            "## Run <!-- agent:run -->\n",
            "- 🚧 do [#live]\n",
            "<!-- /agent:run -->\n",
            "## Backlog <!-- agent:backlog -->\n",
            "- [ ] [#live] still active\n",
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

        let closed = one
            .replace("- 🚧 do [#live]", "- ~~do [#live]~~")
            .replace("- [ ] [#live] still active", "- [x] [#live] still active");
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
    fn reported_work_stays_active_until_its_source_is_integrated_and_closed() {
        let open = concat!(
            "## Active <!-- agent:active priority -->\n",
            "- **Queue** (1)\n",
            "  - `[#worker-report]` — integrate the worker report\n",
            "    - **Status:** running\n",
            "<!-- /agent:active -->\n",
            "<!-- agent:run subagents -->\n",
            "- ### ☑️ [#subagent:worker_report#3] do [#worker-report]\n",
            "<!-- /agent:run -->\n",
        );

        let reported = reconcile_managed_active_items(open)
            .unwrap()
            .expect("the stale running status must refresh to reported");
        assert!(reported.contains("`[#worker-report]`"));
        assert!(reported.contains("- **Status:** reported"));

        let integrated = reported.replace(
            "- ### ☑️ [#subagent:worker_report#3] do [#worker-report]",
            "- ### ~~do [#worker-report]~~",
        );
        let settled = reconcile_managed_active_items(&integrated)
            .unwrap()
            .expect("the terminal source must retire its active row");
        assert!(!settled.contains("`[#worker-report]`"));
        assert!(!settled.contains("- **Status:** reported"));
    }

    #[test]
    fn live_running_and_unresolved_lost_work_are_not_hidden() {
        let content = concat!(
            "## Status <!-- agent:status -->\n<!-- /agent:status -->\n",
            "<!-- agent:run subagents -->\n",
            "- ### 🚧 do [#live]\n",
            "- ### ⚠️ do [#lost]\n",
            "<!-- /agent:run -->\n",
        );

        let reconciled = reconcile_managed_active_items(content)
            .unwrap()
            .expect("both unresolved attempts must be projected");
        assert!(reconciled.contains("`[#live]`"));
        assert!(reconciled.contains("- **Status:** running"));
        assert!(reconciled.contains("`[#lost]`"));
        assert!(reconciled.contains("- **Status:** lost"));
    }

    #[test]
    fn a_terminal_running_attempt_leaves_active_while_unrelated_live_work_remains() {
        let content = concat!(
            "## Active <!-- agent:active priority -->\n",
            "- **Queue** (2)\n",
            "  - `[#terminal]` — terminal worker\n",
            "    - **Status:** running\n",
            "  - `[#live]` — live worker\n",
            "    - **Status:** running\n",
            "<!-- /agent:active -->\n",
            "<!-- agent:run subagents -->\n",
            "- ### ~~do [#terminal]~~\n",
            "- ### 🚧 do [#live]\n",
            "<!-- /agent:run -->\n",
        );

        let reconciled = reconcile_managed_active_items(content)
            .unwrap()
            .expect("the stale terminal row must be removed");
        assert!(!reconciled.contains("`[#terminal]`"));
        assert!(reconciled.contains("`[#live]`"));
        assert_eq!(reconciled.matches("- **Status:** running").count(), 1);
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
