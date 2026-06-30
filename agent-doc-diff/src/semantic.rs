use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::{PromptBearingChange, PromptBearingChangeKind};

/// AST-backed semantic summary for a document diff.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SemanticDiffSummary {
    /// Schema version for additive changes to this JSON object.
    pub schema_version: u8,
    /// Components touched by this diff, in stable sorted order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_components: Vec<String>,
    /// Component-level additions/removals/changes with bounded navigation spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_changes: Vec<SemanticComponentChange>,
    /// Node-keyed item events from the markdown AST overlay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_events: Vec<SemanticNodeEvent>,
    /// Prompt-bearing change previews, preserving encounter order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_changes: Vec<SemanticPromptChange>,
}

/// Component-level semantic operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticComponentOp {
    Added,
    Removed,
    Changed,
}

/// A changed agent component plus before/after navigation handles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticComponentChange {
    pub component: String,
    pub occurrence: usize,
    pub op: SemanticComponentOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<SemanticNavTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<SemanticNavTarget>,
}

/// Bounded source navigation target for an agent component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticNavTarget {
    pub handle: String,
    pub component: String,
    pub occurrence: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A node-keyed item event suitable for preflight JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticNodeEvent {
    pub component: String,
    pub node_key: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_preview: Option<String>,
}

/// Bounded preview of a prompt-bearing semantic change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticPromptChange {
    pub kind: PromptBearingChangeKind,
    pub text_preview: String,
}

#[derive(Debug, Clone)]
struct SemanticComponentSnapshot {
    name: String,
    occurrence: usize,
    attrs: HashMap<String, String>,
    content: String,
    nav: SemanticNavTarget,
}

pub fn semantic_diff_summary(
    previous: &str,
    current: &str,
    prompt_bearing_changes: &[PromptBearingChange],
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

pub fn semantic_component_changes(previous: &str, current: &str) -> Vec<SemanticComponentChange> {
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

fn semantic_component_snapshots(
    side: &str,
    source: &str,
) -> BTreeMap<(String, usize), SemanticComponentSnapshot> {
    let components = match agent_doc_element::element::parse(source) {
        Ok(components) => components,
        Err(_) => return BTreeMap::new(),
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

pub fn semantic_frontmatter_change(
    previous: &str,
    current: &str,
) -> Option<SemanticComponentChange> {
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

pub fn frontmatter_span(source: &str) -> Option<(usize, usize)> {
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

pub fn semantic_nav_target(
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

pub fn semantic_line_at(source: &str, byte: usize) -> usize {
    let end = byte.min(source.len());
    source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

pub fn semantic_node_event_kind(
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

pub fn semantic_preview(text: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let prompt_changes = vec![PromptBearingChange {
            kind: PromptBearingChangeKind::PromptTarget,
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
            PromptBearingChangeKind::PromptTarget
        );
        assert_eq!(summary.prompt_changes[0].text_preview, "do [#beta]");
    }

    #[test]
    fn semantic_diff_summary_omits_empty_summary() {
        assert!(semantic_diff_summary("same\n", "same\n", &[]).is_none());
    }
}
