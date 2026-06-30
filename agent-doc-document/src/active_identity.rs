//! Active document identity projection.
//!
//! This module owns pure `#id` collision policy across prompt presets and
//! active tracked-work components. Runtime callers decide how to render warnings
//! or reject mutations.

use std::collections::BTreeMap;

/// Build the active identity registry for a document.
///
/// Each normalized `#id` (leading `#` stripped) maps to the active sources that
/// define it: a frontmatter `prompt_presets` key, or an active `agent:backlog`,
/// `agent:review`, or `agent:icebox` item id. `agent:done` and checked items are
/// intentionally excluded because they are not active lookup targets.
pub fn document_active_identities(content: &str) -> BTreeMap<String, Vec<String>> {
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) {
        for key in fm.prompt_presets.keys() {
            let norm = key.trim().trim_start_matches('#').to_string();
            if !norm.is_empty() {
                sources
                    .entry(norm)
                    .or_default()
                    .push("prompt_presets".to_string());
            }
        }
    }
    if let Ok(components) = agent_doc_element::element::parse(content) {
        for component in &components {
            let label = if agent_doc_element::element::is_backlog_component(&component.name) {
                "agent:backlog"
            } else if agent_doc_element::element::is_review_component(&component.name) {
                "agent:review"
            } else if agent_doc_element::element::is_icebox_component(&component.name) {
                "agent:icebox"
            } else {
                continue;
            };
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(component.content(content));
            for item in items.iter().filter(|item| !item.is_done()) {
                if !item.id.is_empty() {
                    sources
                        .entry(item.id.clone())
                        .or_default()
                        .push(label.to_string());
                }
            }
        }
    }
    sources
}

/// Collect identities that resolve under more than one active source.
///
/// When the same `#id` exists in two active sources, `do #id`, queue generation,
/// and "top backlog item: #id" are ambiguous between preset expansion and item
/// execution.
pub fn detect_identity_collisions(content: &str) -> Vec<String> {
    document_active_identities(content)
        .into_iter()
        .filter(|(_, srcs)| srcs.len() > 1)
        .map(|(id, srcs)| format!("#{id} ({})", srcs.join(" + ")))
        .collect()
}

/// Return existing active sources that a new explicit id would collide with.
///
/// The candidate is normalized by trimming whitespace, stripping a leading `#`,
/// and lowercasing before lookup.
pub fn identity_collision_for_new_id(content: &str, candidate_id: &str) -> Option<Vec<String>> {
    let norm = candidate_id
        .trim()
        .trim_start_matches('#')
        .to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }
    document_active_identities(content)
        .get(&norm)
        .filter(|srcs| !srcs.is_empty())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_identity_collisions_flags_preset_vs_backlog_id() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items?\n",
            "  '#commit-push': commit + push\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#next-steps] do the next steps\n",
            "- [ ] [#other1] unrelated work\n",
            "<!-- /agent:backlog -->\n",
        );
        let collisions = detect_identity_collisions(content);
        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert!(collisions[0].contains("#next-steps"), "{collisions:?}");
        assert!(collisions[0].contains("prompt_presets"), "{collisions:?}");
        assert!(collisions[0].contains("agent:backlog"), "{collisions:?}");
    }

    #[test]
    fn detect_identity_collisions_flags_duplicate_active_ids_across_components() {
        let content = concat!(
            "---\nagent_doc_session: t\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#dup7] in backlog\n<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n- [/] [#dup7] also gated in review\n<!-- /agent:review -->\n",
        );
        let collisions = detect_identity_collisions(content);
        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert!(collisions[0].contains("#dup7"), "{collisions:?}");
    }

    #[test]
    fn detect_identity_collisions_ignores_done_ids_and_clean_docs() {
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n- [x] [#next-steps] completed long ago\n<!-- /agent:review -->\n",
        );
        assert!(
            detect_identity_collisions(content).is_empty(),
            "done ids and unique active ids must not collide"
        );
    }

    #[test]
    fn identity_collision_for_new_id_reports_existing_sources() {
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n",
        );
        assert_eq!(
            identity_collision_for_new_id(content, "next-steps"),
            Some(vec!["prompt_presets".to_string()])
        );
        assert_eq!(
            identity_collision_for_new_id(content, "#ALPHA"),
            Some(vec!["agent:backlog".to_string()])
        );
        assert_eq!(identity_collision_for_new_id(content, "fresh01"), None);
        assert_eq!(identity_collision_for_new_id(content, ""), None);
    }
}
