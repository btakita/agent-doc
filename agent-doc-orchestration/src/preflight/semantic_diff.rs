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

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
use std::io::Write;
use std::process::Command;
use tempfile::TempDir;
#[test]
fn semantic_diff_summary_reports_components_nodes_and_prompt_previews() {
    let before = concat!(
        "---\n",
        "queue: stop\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#task] old wording\n",
        "<!-- /agent:backlog -->\n"
    );
    let current = concat!(
        "---\n",
        "queue: go\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#task] new wording\n",
        "<!-- /agent:backlog -->\n"
    );
    let prompt_changes = vec![crate::diff::PromptBearingChange {
        kind: crate::diff::PromptBearingChangeKind::PromptTarget,
        text: "do [#beta]".to_string(),
    }];

    let summary = semantic_diff_summary(before, current, &prompt_changes).unwrap();

    assert_eq!(summary.schema_version, 1);
    assert!(
        summary
            .changed_components
            .contains(&"frontmatter".to_string())
    );
    assert!(summary.changed_components.contains(&"queue".to_string()));
    assert!(summary.changed_components.contains(&"backlog".to_string()));
    assert!(summary.changed_components.contains(&"exchange".to_string()));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "frontmatter" && change.op == SemanticComponentOp::Changed
    }));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "queue"
            && change.op == SemanticComponentOp::Changed
            && change
                .after
                .as_ref()
                .is_some_and(|target| target.handle == "component:after:queue:0")
    }));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "backlog" && change.op == SemanticComponentOp::Changed
    }));
    assert!(summary.node_events.iter().any(|event| {
        event.component == "queue"
            && event.op == "insert"
            && event.node_key == "queue:0:beta:0"
            && event.after_preview.as_deref() == Some("- do [#beta]")
    }));
    assert_eq!(
        summary.prompt_changes[0].kind,
        crate::diff::PromptBearingChangeKind::PromptTarget
    );
    assert_eq!(summary.prompt_changes[0].text_preview, "do [#beta]");
}
#[test]
fn semantic_diff_summary_omits_empty_summary() {
    assert!(semantic_diff_summary("same\n", "same\n", &[]).is_none());
}
#[test]
fn sibling_queue_insert_beside_driver_is_independent() {
    // The motivating case: the turn answers queue item A while the user
    // inserts queue item B beside it. B must classify Independent and the
    // turn must not be affected (#op-scoped-drift-3).
    let before = "<!-- agent:queue -->\n- do [#driver-a]\n<!-- /agent:queue -->\n";
    let after =
        "<!-- agent:queue -->\n- do [#driver-a]\n- do [#sibling-b]\n<!-- /agent:queue -->\n";
    let summary = semantic_diff_summary(before, after, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
    // The turn is answering driver-a.
    let scope = derive_turn_scope(after, &["do [#driver-a]".to_string()]).unwrap();
    let affectedness = agent_doc_core::turn_scope::classify_cycle(&ops, &scope);
    assert!(
        !affectedness.turn_affected,
        "a sibling queue insert must not affect the turn"
    );
    assert!(
        affectedness
            .classified
            .iter()
            .all(|op| op.class == agent_doc_core::turn_scope::AffectednessClass::Independent)
    );
}
#[test]
fn exchange_old_block_edit_is_independent_but_tail_append_affects() {
    // #loop-guard-exchange-node-granularity end-to-end: while the turn answers
    // a queue driver, an edit to an OLD bulleted exchange block must classify
    // Independent (must not preempt the auto-loop drain), while a genuine new
    // bulleted prompt appended at the exchange tail must still affect the turn.
    let base = "\
<!-- agent:exchange -->
### Re: prior topic

- old context bullet one
- old context bullet two
<!-- agent:boundary:b1 -->
<!-- /agent:exchange -->

<!-- agent:queue go -->
- do [#driver]
<!-- /agent:queue -->
";
    let targets = vec!["do [#driver]".to_string()];
    let scope = derive_turn_scope(base, &targets).expect("scope derived");
    assert_eq!(
        scope.exchange_tail_floor,
        Some(2),
        "two committed exchange bullets => tail floor 2"
    );

    // Old-block edit: change the FIRST (index 0) exchange bullet.
    let old_edit = base.replace(
        "- old context bullet one",
        "- old context bullet one EDITED",
    );
    let summary = semantic_diff_summary(base, &old_edit, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
    let affectedness = agent_doc_core::turn_scope::classify_cycle(&ops, &scope);
    assert!(
        !affectedness.turn_affected,
        "editing an old exchange block must not affect the turn: {:?}",
        affectedness.classified
    );

    // Tail append: a new bulleted prompt after the last committed bullet.
    let tail_append = base.replace(
        "- old context bullet two\n",
        "- old context bullet two\n- please also cover the retry path\n",
    );
    let summary2 = semantic_diff_summary(base, &tail_append, &[]).unwrap();
    let ops2 = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary2);
    let affectedness2 = agent_doc_core::turn_scope::classify_cycle(&ops2, &scope);
    assert!(
        affectedness2.turn_affected,
        "a new tail-appended exchange prompt must still affect the turn: {:?}",
        affectedness2.classified
    );
}
#[test]
fn derive_turn_scope_resolves_queue_driver_and_sets() {
    let content =
        "<!-- agent:queue -->\n- do [#op-scoped-drift-2]\n- do [#later]\n<!-- /agent:queue -->\n";
    let targets = vec!["do [#op-scoped-drift-2]".to_string()];
    let scope = derive_turn_scope(content, &targets).expect("scope derived");
    let driver = scope.driver.as_ref().expect("driver resolved");
    assert_eq!(driver.component, "queue");
    assert_eq!(
        driver.node_key.as_deref(),
        Some("queue:0:op-scoped-drift-2:0")
    );
    // driver is read (input) and written (the strike).
    assert!(scope.read_set.contains(driver));
    assert!(scope.write_set.contains(driver));
    assert!(
        scope
            .write_set
            .contains(&agent_doc_core::turn_scope::Address::component(
                "backlog", 0
            ))
    );
}
#[test]
fn derive_turn_scope_none_without_prompt_targets() {
    let content = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
    assert!(derive_turn_scope(content, &[]).is_none());
}
#[test]
fn derive_turn_scope_without_matching_queue_node_has_no_driver() {
    // A prompt target whose id is not present in the queue still yields a
    // scope (output components) but no driver.
    let content = "<!-- agent:queue -->\n- do [#present]\n<!-- /agent:queue -->\n";
    let targets = vec!["do [#absent]".to_string()];
    let scope = derive_turn_scope(content, &targets).expect("scope derived");
    assert!(scope.driver.is_none());
    assert!(scope.write_set.iter().all(|a| a.component != "queue"));
}
#[test]
fn user_intent_empty_for_synthetic_queue_continuation() {
    // A pure auto-queue continuation is never user intent, regardless of the
    // affectedness verdict.
    let changes = vec![user_prompt_change("do [#next]")];
    assert!(
        compute_user_intent_prompt_changes(&changes, true, Some(&affectedness(true))).is_empty()
    );
}
#[test]
fn user_intent_drops_turn_independent_edits() {
    // #queue-no-stop-unrelated-edit: a real (non-managed) edit that the
    // classifier scoped as independent of the turn must NOT halt the drain.
    let changes = vec![user_prompt_change("a stray note in the parking lot")];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(false)));
    assert!(
        out.is_empty(),
        "independent edit should not preempt: {out:?}"
    );
}
#[test]
fn user_intent_keeps_turn_affecting_prompt() {
    // A genuine new user prompt edits the in-scope exchange tail, so the
    // classifier reports turn_affected — it must still preempt.
    let changes = vec![user_prompt_change("please also handle the error case")];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));
    assert_eq!(out.len(), 1, "turn-affecting prompt must preempt");
}
#[test]
fn user_intent_filters_managed_state_when_turn_affected() {
    // Even when the turn is affected, managed-component bookkeeping (a backlog
    // item line) stays filtered — it is not a real prompt.
    let changes = vec![crate::diff::PromptBearingChange {
        kind: crate::diff::PromptBearingChangeKind::ContentEdit,
        text: "- [ ] [#newitem] track a follow-up".to_string(),
    }];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));
    assert!(
        out.is_empty(),
        "managed-state edit must not preempt: {out:?}"
    );
}
#[test]
fn user_intent_conservative_without_classifier() {
    // No affectedness classifier (semantic-diff skip): fall back to the
    // managed-state filter only, so a real change still preempts.
    let changes = vec![user_prompt_change("a real prompt with no classifier")];
    let out = compute_user_intent_prompt_changes(&changes, false, None);
    assert_eq!(out.len(), 1, "without classifier, a real change preempts");
}
#[test]
fn extract_target_id_handles_bracket_and_bare_forms() {
    assert_eq!(
        extract_target_id("do [#op-scoped-drift-2]").as_deref(),
        Some("op-scoped-drift-2")
    );
    assert_eq!(extract_target_id("do #fix1").as_deref(), Some("fix1"));
    assert_eq!(extract_target_id("no id here"), None);
}
#[test]
fn build_ops_from_semantic_diff_tags_user_actor_and_session() {
    let before = "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n";
    let after = "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n";
    let summary = semantic_diff_summary(before, after, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "100", &summary);
    assert!(!ops.is_empty());
    let beta = ops
        .iter()
        .find(|op| op.node_key == "queue:0:beta:0")
        .expect("beta op present");
    assert_eq!(beta.actor, agent_doc_core::op_log::OpActor::User);
    assert_eq!(beta.op_kind, "insert");
    assert_eq!(beta.component, "queue");
    assert_eq!(beta.clock.origin_session.as_deref(), Some("sess-1"));
    // Lamport assignment is owned by the durable store; the builder leaves 0.
    assert_eq!(beta.clock.lamport, 0);
}
}
