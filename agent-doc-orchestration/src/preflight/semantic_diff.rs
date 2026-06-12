//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone)]
pub(crate) struct SemanticComponentSnapshot {
    name: String,
    occurrence: usize,
    attrs: HashMap<String, String>,
    content: String,
    nav: SemanticNavTarget,
}

pub(crate) fn semantic_diff_summary(
    previous: &str,
    current: &str,
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
) -> Option<SemanticDiffSummary> {
    let mut changed_components = BTreeSet::new();
    let mut component_changes = semantic_component_changes(previous, current);
    let mut node_events = agent_doc_markdown_ast::events::diff_node_events(previous, current)
        .into_iter()
        .map(|event| {
            changed_components.insert(event.component.clone());
            SemanticNodeEvent {
                component: event.component,
                node_key: event.node_key,
                op: semantic_node_event_kind(event.kind).to_string(),
                item_id: event.item_id,
                before_index: event.before_index,
                after_index: event.after_index,
                previous_node_key: event.previous_node_key,
                next_node_key: event.next_node_key,
                before_preview: event.before.as_deref().and_then(semantic_preview),
                after_preview: event.after.as_deref().and_then(semantic_preview),
            }
        })
        .collect::<Vec<_>>();
    let prompt_changes = prompt_bearing_changes
        .iter()
        .filter_map(|change| {
            changed_components.insert("exchange".to_string());
            semantic_preview(&change.text).map(|text_preview| SemanticPromptChange {
                kind: change.kind.clone(),
                text_preview,
            })
        })
        .collect::<Vec<_>>();

    for change in &component_changes {
        changed_components.insert(change.component.clone());
    }
    node_events.sort_by(|a, b| {
        a.component
            .cmp(&b.component)
            .then_with(|| a.after_index.cmp(&b.after_index))
            .then_with(|| a.before_index.cmp(&b.before_index))
            .then_with(|| a.node_key.cmp(&b.node_key))
    });

    if component_changes.is_empty() && node_events.is_empty() && prompt_changes.is_empty() {
        return None;
    }

    component_changes.sort_by(|a, b| {
        a.component
            .cmp(&b.component)
            .then_with(|| a.occurrence.cmp(&b.occurrence))
    });

    Some(SemanticDiffSummary {
        schema_version: 1,
        changed_components: changed_components.into_iter().collect(),
        component_changes,
        node_events,
        prompt_changes,
    })
}

/// Build durable op-log records from this cycle's semantic node events
/// (`#op-scoped-drift-1`). Preflight observes a snapshot↔document diff, so every
/// node op is classified as a `user` edit (the agent's committed output already
/// lives in the snapshot). Pure so it can be unit-tested without a database.
pub(crate) fn build_ops_from_semantic_diff(
    document_path: &str,
    origin_session: Option<&str>,
    recorded_at: &str,
    summary: &SemanticDiffSummary,
) -> Vec<agent_doc_core::op_log::DocumentOp> {
    use agent_doc_core::op_log::{CausalClock, DocumentOp, OpSource, classify_actor};
    let actor = classify_actor(OpSource::SnapshotDiff);
    summary
        .node_events
        .iter()
        .map(|event| DocumentOp {
            document_path: document_path.to_string(),
            component: event.component.clone(),
            node_key: event.node_key.clone(),
            // Within-component node index: after-index for inserts/replaces,
            // before-index for removes. Feeds the exchange-tail narrowing in the
            // affectedness classifier (`#loop-guard-exchange-node-granularity`).
            node_index: event.after_index.or(event.before_index),
            item_id: event.item_id.clone(),
            op_kind: event.op.clone(),
            actor,
            clock: CausalClock {
                lamport: 0,
                origin_session: origin_session.map(str::to_string),
            },
            before_preview: event.before_preview.clone(),
            after_preview: event.after_preview.clone(),
            recorded_at: Some(recorded_at.to_string()),
        })
        .collect()
}

/// Persist the cycle's node ops to the durable sqlite op log. Best effort:
/// failures are logged to stderr and never propagate, so the durable substrate
/// can never block a preflight cycle.
pub(crate) fn persist_op_log(
    file: &Path,
    rc: &crate::graph::RunContext,
    origin_session: Option<&str>,
    summary: &SemanticDiffSummary,
) {
    if summary.node_events.is_empty() {
        return;
    }
    let Some(project_root) = rc.project_root() else {
        return;
    };
    let document_path = file.to_string_lossy().to_string();
    let recorded_at = op_log_timestamp().to_string();
    let ops = build_ops_from_semantic_diff(&document_path, origin_session, &recorded_at, summary);
    if let Err(err) = agent_doc_sqlite::op_log::append_ops(&project_root, &ops) {
        eprintln!("[preflight] op-log persist skipped: {err}");
    }
}

pub(crate) fn op_log_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Compute `user_intent_prompt_changes` — the set the Claude Code auto-loop
/// guard treats as "real user intent that must preempt an active queue drain".
/// The loop halts only when this is non-empty, so it must exclude everything
/// that is *not* fresh intent for the current turn:
///
/// - a synthetic queue-continuation diff (`diff_from_queue_head_only`) is queue
///   bookkeeping, never user intent;
/// - managed-component edits (queue/backlog/review/done items, activity toggles)
///   are routine session bookkeeping (`change_is_managed_state_only`);
/// - edits the affectedness classifier scoped as **independent** of the current
///   turn — i.e. targeting no address in the turn's read/write set
///   (`#queue-no-stop-unrelated-edit`). When `op_affectedness` ran and reports
///   `turn_affected == false`, every user op this cycle was independent or
///   provenance-spoofed, so the drain must not stop.
///
/// A genuine user prompt typed mid-loop edits the in-scope `exchange` tail, which
/// classifies as turn-affecting (`InputAffecting`/`OutputContended`), so
/// `turn_affected` is `true` and the prompt still preempts. When the classifier
/// did not run (`op_affectedness` is `None`, e.g. a semantic-diff parse skip),
/// this stays conservative and falls back to the managed-state filter only.
pub(crate) fn compute_user_intent_prompt_changes(
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
    diff_from_queue_head_only: bool,
    op_affectedness: Option<&agent_doc_core::turn_scope::CycleAffectedness>,
) -> Vec<crate::diff::PromptBearingChange> {
    if diff_from_queue_head_only {
        // Synthetic auto-queue continuation only — no user intent this cycle.
        return Vec::new();
    }
    if op_affectedness.is_some_and(|affectedness| !affectedness.turn_affected) {
        // The classifier ran and scoped every user op this cycle as independent
        // of the turn — nothing affects it, so the drain must not halt.
        return Vec::new();
    }
    prompt_bearing_changes
        .iter()
        .filter(|change| !crate::diff::change_is_managed_state_only(change))
        .cloned()
        .collect()
}

/// Derive the TurnScope manifest for the current turn (`#op-scoped-drift-2`).
/// Resolves the driver queue node from `prompt_targets`, then builds the
/// canonical read/write sets. Returns `None` when the turn answers no prompt.
pub(crate) fn derive_turn_scope(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_core::turn_scope::TurnScope> {
    if prompt_targets.is_empty() {
        return None;
    }
    let driver = resolve_driver_address(content, prompt_targets);
    let exchange_tail_floor = exchange_node_count(content);
    Some(
        agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(
            driver,
            exchange_tail_floor,
        ),
    )
}

/// Count of `exchange` item nodes present at turn start — the tail floor for the
/// affectedness classifier (`#loop-guard-exchange-node-granularity`). An op at an
/// index at or above this count is a tail append/edit (affects the turn); below it
/// is committed history. Returns `None` when there are no exchange nodes so the
/// classifier keeps its coarse whole-component behavior.
pub(crate) fn exchange_node_count(content: &str) -> Option<usize> {
    let count = agent_doc_markdown_ast::mutations::all_item_nodes(content)
        .iter()
        .filter(|node| node.component == "exchange")
        .count();
    (count > 0).then_some(count)
}

/// Find the queue item node a prompt target refers to and address it.
pub(crate) fn resolve_driver_address(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_core::turn_scope::Address> {
    let nodes = agent_doc_markdown_ast::mutations::all_item_nodes(content);
    for target in prompt_targets {
        let Some(id) = extract_target_id(target) else {
            continue;
        };
        if let Some(node) = nodes
            .iter()
            .find(|node| node.component == "queue" && node.item.id == id)
        {
            let occurrence = component_occurrence_from_node_key(&node.node_key);
            return Some(agent_doc_core::turn_scope::Address::node(
                "queue",
                occurrence,
                &node.node_key,
            ));
        }
    }
    None
}

/// Extract a backlog/queue id (`[#id]` or bare `#id`) from a prompt target.
pub(crate) fn extract_target_id(target: &str) -> Option<String> {
    if let Some(start) = target.find("[#") {
        let rest = &target[start + 2..];
        if let Some(close) = rest.find(']') {
            let id = &rest[..close];
            if agent_doc_core::pending::is_valid_pending_id(id) {
                return Some(id.to_string());
            }
        }
    }
    if let Some(start) = target.find('#') {
        let rest = &target[start + 1..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !id.is_empty() && agent_doc_core::pending::is_valid_pending_id(&id) {
            return Some(id);
        }
    }
    None
}

/// Component occurrence index encoded in a node key (`component:index:id:dup`).
pub(crate) fn component_occurrence_from_node_key(node_key: &str) -> usize {
    node_key
        .split(':')
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0)
}

pub(crate) fn semantic_component_changes(previous: &str, current: &str) -> Vec<SemanticComponentChange> {
    let before = semantic_component_snapshots("before", previous);
    let after = semantic_component_snapshots("after", current);
    let mut keys = BTreeSet::new();
    keys.extend(before.keys().cloned());
    keys.extend(after.keys().cloned());

    let mut changes = Vec::new();
    for key in keys {
        match (before.get(&key), after.get(&key)) {
            (None, Some(after_snapshot)) => changes.push(SemanticComponentChange {
                component: after_snapshot.name.clone(),
                occurrence: after_snapshot.occurrence,
                op: SemanticComponentOp::Added,
                before: None,
                after: Some(after_snapshot.nav.clone()),
            }),
            (Some(before_snapshot), None) => changes.push(SemanticComponentChange {
                component: before_snapshot.name.clone(),
                occurrence: before_snapshot.occurrence,
                op: SemanticComponentOp::Removed,
                before: Some(before_snapshot.nav.clone()),
                after: None,
            }),
            (Some(before_snapshot), Some(after_snapshot))
                if before_snapshot.content != after_snapshot.content
                    || before_snapshot.attrs != after_snapshot.attrs =>
            {
                changes.push(SemanticComponentChange {
                    component: after_snapshot.name.clone(),
                    occurrence: after_snapshot.occurrence,
                    op: SemanticComponentOp::Changed,
                    before: Some(before_snapshot.nav.clone()),
                    after: Some(after_snapshot.nav.clone()),
                });
            }
            _ => {}
        }
    }

    if let Some(change) = semantic_frontmatter_change(previous, current) {
        changes.push(change);
    }

    changes
}

pub(crate) fn semantic_component_snapshots(
    side: &str,
    source: &str,
) -> BTreeMap<(String, usize), SemanticComponentSnapshot> {
    let components = match crate::component::parse(source) {
        Ok(components) => components,
        Err(err) => {
            eprintln!("[preflight] semantic_diff: component parse skipped: {err}");
            return BTreeMap::new();
        }
    };
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    let mut snapshots = BTreeMap::new();
    for component in components {
        let occurrence = occurrences.entry(component.name.clone()).or_insert(0);
        let occurrence_value = *occurrence;
        *occurrence += 1;
        let nav = semantic_nav_target(
            side,
            &component.name,
            occurrence_value,
            source,
            component.open_start,
            component.close_end,
        );
        snapshots.insert(
            (component.name.clone(), occurrence_value),
            SemanticComponentSnapshot {
                name: component.name.clone(),
                occurrence: occurrence_value,
                attrs: component.attrs.clone(),
                content: component.content(source).to_string(),
                nav,
            },
        );
    }
    snapshots
}

pub(crate) fn semantic_frontmatter_change(previous: &str, current: &str) -> Option<SemanticComponentChange> {
    let before_span = frontmatter_span(previous);
    let after_span = frontmatter_span(current);
    let before_text = before_span.and_then(|(start, end)| previous.get(start..end));
    let after_text = after_span.and_then(|(start, end)| current.get(start..end));
    if before_text == after_text {
        return None;
    }

    let before = before_span
        .map(|(start, end)| semantic_nav_target("before", "frontmatter", 0, previous, start, end));
    let after = after_span
        .map(|(start, end)| semantic_nav_target("after", "frontmatter", 0, current, start, end));
    let op = match (before.is_some(), after.is_some()) {
        (false, true) => SemanticComponentOp::Added,
        (true, false) => SemanticComponentOp::Removed,
        _ => SemanticComponentOp::Changed,
    };
    Some(SemanticComponentChange {
        component: "frontmatter".to_string(),
        occurrence: 0,
        op,
        before,
        after,
    })
}

pub(crate) fn frontmatter_span(source: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        if index == 0 {
            if line.trim_end() != "---" {
                return None;
            }
            continue;
        }
        if line.trim_end() == "---" {
            return Some((0, offset));
        }
        if line_start == source.len() {
            break;
        }
    }
    None
}

pub(crate) fn semantic_nav_target(
    side: &str,
    component: &str,
    occurrence: usize,
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> SemanticNavTarget {
    let start_byte = start_byte.min(source.len());
    let end_byte = end_byte.min(source.len()).max(start_byte);
    let start_line = semantic_line_at(source, start_byte);
    let end_line = if end_byte == start_byte {
        start_line
    } else {
        semantic_line_at(source, end_byte.saturating_sub(1))
    };
    SemanticNavTarget {
        handle: format!("component:{side}:{component}:{occurrence}"),
        component: component.to_string(),
        occurrence,
        start_line,
        end_line,
        start_byte,
        end_byte,
    }
}

pub(crate) fn semantic_line_at(source: &str, byte: usize) -> usize {
    let end = byte.min(source.len());
    source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

pub(crate) fn semantic_node_event_kind(
    kind: agent_doc_markdown_ast::events::DocumentNodeEventKind,
) -> &'static str {
    match kind {
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Insert => "insert",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Remove => "remove",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Replace => "replace",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Move => "move",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Strike => "strike",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Unstrike => "unstrike",
    }
}

pub(crate) fn semantic_preview(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 200;
    let mut preview = trimmed.chars().take(MAX_CHARS).collect::<String>();
    if trimmed.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    Some(preview)
}

pub(crate) fn push_unique_strings(target: &mut Vec<String>, extras: Vec<String>) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

pub(crate) fn push_unique_prompt_bearing_changes(
    target: &mut Vec<crate::diff::PromptBearingChange>,
    extras: Vec<crate::diff::PromptBearingChange>,
) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}
