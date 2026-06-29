//! Pure queue projection helpers.
//!
//! The `🚧` marker is a visible projection of active queue heads. It is not
//! queue identity and does not commit or admit a turn by itself.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub const PRIORITIZED_MARKERS: [&str; 7] = [
    "**prioritized**",
    "__prioritized__",
    "**pin**",
    "__pin__",
    ":pushpin:",
    ":pin:",
    "📌",
];
pub const AGENT_PRIORITIZED_MARKERS: [&str; 6] = [
    "*prioritized*",
    "_prioritized_",
    "*pin*",
    "_pin_",
    ":round_pushpin:",
    "📍",
];
pub const PRIORITIZED_MARKER: &str = ":pushpin:";
pub const AGENT_PRIORITIZED_MARKER: &str = ":round_pushpin:";
pub const IN_PROGRESS_MARKER: &str = "🚧";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuePromptRow {
    pub text: String,
    pub id: Option<String>,
    pub projectable_default: bool,
}

impl QueuePromptRow {
    pub fn new(text: impl Into<String>, id: Option<String>, projectable_default: bool) -> Self {
        Self {
            text: text.into(),
            id,
            projectable_default,
        }
    }

    pub fn unmarked_text(&self) -> String {
        strip_in_progress_marker(&self.text)
    }

    pub fn marked_in_progress(&self) -> bool {
        has_in_progress_marker(&self.text)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveQueuePromptProjection {
    pub prompts: Vec<String>,
    pub retargeted: bool,
    pub missing_dependency_ids: Vec<String>,
}

/// Strip the leading in-progress marker while preserving priority markers.
pub fn strip_in_progress_marker(text: &str) -> String {
    let mut rest = text.trim();
    let mut kept_prefixes = Vec::new();
    loop {
        let trimmed = rest.trim_start();
        if let Some(after_marker) = trimmed.strip_prefix(IN_PROGRESS_MARKER) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((marker, after_marker)) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| trimmed.strip_prefix(marker).map(|after| (*marker, after)))
        {
            kept_prefixes.push(marker);
            rest = after_marker.trim_start();
            continue;
        }
        rest = trimmed;
        break;
    }
    if kept_prefixes.is_empty() {
        rest.trim().to_string()
    } else {
        let mut out = kept_prefixes.join(" ");
        if !rest.trim().is_empty() {
            out.push(' ');
            out.push_str(rest.trim());
        }
        out
    }
}

pub fn apply_in_progress_marker(text: &str) -> String {
    format!("{IN_PROGRESS_MARKER} {}", strip_in_progress_marker(text))
}

pub fn has_in_progress_marker(text: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if rest.starts_with(IN_PROGRESS_MARKER) {
            return true;
        }
        if let Some(after_marker) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| rest.strip_prefix(marker))
        {
            rest = after_marker.trim_start();
            continue;
        }
        return false;
    }
}

pub fn strip_in_progress_marker_for_display(text: &str) -> Option<String> {
    let stripped = strip_in_progress_marker(text);
    (stripped != text.trim()).then_some(stripped)
}

/// Strip priority and in-progress markers from the visible prompt identity.
pub fn strip_priority_markers(text: &str) -> String {
    let mut t = text.trim();
    loop {
        let trimmed = t.trim_start();
        let stripped = trimmed.strip_prefix(IN_PROGRESS_MARKER).or_else(|| {
            PRIORITIZED_MARKERS
                .iter()
                .chain(AGENT_PRIORITIZED_MARKERS.iter())
                .find_map(|m| trimmed.strip_prefix(m))
        });
        match stripped {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    t.trim().to_string()
}

pub fn in_progress_marker_retarget_requested(diff: Option<&str>, rows: &[QueuePromptRow]) -> bool {
    if !rows.iter().any(QueuePromptRow::marked_in_progress) {
        return false;
    }
    let Some(diff) = diff else {
        return false;
    };
    diff.lines().any(|line| {
        (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
            && line.contains(IN_PROGRESS_MARKER)
    })
}

pub fn project_active_queue_prompts(
    rows: &[QueuePromptRow],
    after_deps: &HashMap<String, Vec<String>>,
    honor_in_progress_markers: bool,
) -> ActiveQueuePromptProjection {
    let normalized = rows
        .iter()
        .map(|row| {
            (
                row.unmarked_text(),
                row.id.clone(),
                row.marked_in_progress(),
                row.projectable_default,
            )
        })
        .collect::<Vec<_>>();

    let marked = if honor_in_progress_markers {
        normalized
            .iter()
            .filter(|(_, _, marked, _)| *marked)
            .map(|(text, id, _, _)| (text.clone(), id.clone()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if marked.is_empty() {
        return ActiveQueuePromptProjection {
            prompts: normalized
                .iter()
                .find(|(_, _, _, projectable)| *projectable)
                .map(|(text, _, _, _)| text.clone())
                .into_iter()
                .collect(),
            retargeted: false,
            missing_dependency_ids: Vec::new(),
        };
    }

    let mut id_to_text = HashMap::<String, String>::new();
    let mut id_deps = HashMap::<String, Vec<String>>::new();
    for (text, id, _, _) in &normalized {
        if let Some(id) = id {
            id_to_text.entry(id.clone()).or_insert_with(|| text.clone());
            let mut merged = after_deps.get(id).cloned().unwrap_or_default();
            for inline in agent_doc_element_backlog::backlog::item_after_deps(text) {
                if !merged.contains(&inline) {
                    merged.push(inline);
                }
            }
            if !merged.is_empty() {
                id_deps.entry(id.clone()).or_insert(merged);
            }
        }
    }

    let mut selected_ids = HashSet::<String>::new();
    let mut selected_id_order = Vec::<String>::new();
    let mut selected_free_texts = HashSet::<String>::new();
    let mut missing_dependency_ids = BTreeSet::<String>::new();

    fn visit(
        id: &str,
        id_to_text: &HashMap<String, String>,
        id_deps: &HashMap<String, Vec<String>>,
        selected_ids: &mut HashSet<String>,
        selected_id_order: &mut Vec<String>,
        visiting: &mut HashSet<String>,
        missing_dependency_ids: &mut BTreeSet<String>,
    ) {
        if selected_ids.contains(id) {
            return;
        }
        if !visiting.insert(id.to_string()) {
            return;
        }
        if let Some(dep_ids) = id_deps.get(id) {
            for dep in dep_ids {
                if id_to_text.contains_key(dep) {
                    visit(
                        dep,
                        id_to_text,
                        id_deps,
                        selected_ids,
                        selected_id_order,
                        visiting,
                        missing_dependency_ids,
                    );
                } else {
                    missing_dependency_ids.insert(dep.clone());
                }
            }
        }
        visiting.remove(id);
        if selected_ids.insert(id.to_string()) {
            selected_id_order.push(id.to_string());
        }
    }

    for (text, id) in marked {
        if let Some(id) = id {
            visit(
                &id,
                &id_to_text,
                &id_deps,
                &mut selected_ids,
                &mut selected_id_order,
                &mut HashSet::new(),
                &mut missing_dependency_ids,
            );
        } else {
            selected_free_texts.insert(strip_priority_markers(&text));
        }
    }

    let mut prompts = Vec::new();
    for id in selected_id_order {
        if let Some(text) = id_to_text.get(&id) {
            prompts.push(text.clone());
        }
    }
    for (text, id, _, _) in &normalized {
        if id.is_none() && selected_free_texts.contains(&strip_priority_markers(text)) {
            prompts.push(text.clone());
        }
    }

    ActiveQueuePromptProjection {
        prompts,
        retargeted: true,
        missing_dependency_ids: missing_dependency_ids.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_identity_preserves_priority_marker() {
        assert_eq!(
            strip_in_progress_marker("🚧 :round_pushpin: do [#alpha]"),
            ":round_pushpin: do [#alpha]"
        );
        assert!(has_in_progress_marker(":round_pushpin: 🚧 do [#alpha]"));
        assert_eq!(
            strip_priority_markers(":round_pushpin: 🚧 do [#alpha]"),
            "do [#alpha]"
        );
    }

    #[test]
    fn default_projection_uses_first_projectable_row() {
        let rows = vec![
            QueuePromptRow::new("do [#verify]", Some("verify".to_string()), false),
            QueuePromptRow::new("do [#ready]", Some("ready".to_string()), true),
        ];

        let projection = project_active_queue_prompts(&rows, &HashMap::new(), false);

        assert_eq!(projection.prompts, vec!["do [#ready]"]);
        assert!(!projection.retargeted);
    }

    #[test]
    fn retarget_projection_expands_auto_dag_dependencies() {
        let rows = vec![
            QueuePromptRow::new("do [#ops]", Some("ops".to_string()), true),
            QueuePromptRow::new("🚧 do [#ship]", Some("ship".to_string()), true),
            QueuePromptRow::new(
                ":round_pushpin: do [#setup]",
                Some("setup".to_string()),
                true,
            ),
        ];
        let deps = HashMap::from([("ship".to_string(), vec!["setup".to_string()])]);
        let projection = project_active_queue_prompts(&rows, &deps, true);

        assert_eq!(
            projection.prompts,
            vec![":round_pushpin: do [#setup]", "do [#ship]"]
        );
        assert!(projection.retargeted);
    }

    #[test]
    fn retarget_requires_marker_diff_signal() {
        let rows = vec![QueuePromptRow::new(
            "🚧 do [#ship]",
            Some("ship".to_string()),
            true,
        )];
        assert!(!in_progress_marker_retarget_requested(None, &rows));
        assert!(!in_progress_marker_retarget_requested(
            Some("+- do [#ship]"),
            &rows
        ));
        assert!(in_progress_marker_retarget_requested(
            Some("+- 🚧 do [#ship]"),
            &rows
        ));
    }
}
