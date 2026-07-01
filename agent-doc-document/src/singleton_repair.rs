//! Pure repair for duplicate singleton agent-doc components.

use std::collections::HashMap;

use agent_doc_element::element;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateSingletonComponentRepair {
    pub content: String,
    pub groups: Vec<String>,
    pub removed: usize,
}

fn canonical_singleton_component_name(name: &str) -> Option<&'static str> {
    match name {
        "exchange" => Some("exchange"),
        "status" => Some("status"),
        "queue" => Some("queue"),
        element::BACKLOG_DONE_COMPONENT => Some(element::BACKLOG_DONE_COMPONENT),
        _ if element::is_backlog_component(name) => Some(element::BACKLOG_COMPONENT),
        _ if element::is_review_component(name) => Some(element::REVIEW_COMPONENT),
        _ if element::is_icebox_component(name) => Some(element::ICEBOX_COMPONENT),
        _ => None,
    }
}

fn singleton_components_by_name(
    doc: &str,
) -> Option<HashMap<&'static str, Vec<element::Component>>> {
    let components = element::parse(doc).ok()?;
    let mut by_name: HashMap<&'static str, Vec<element::Component>> = HashMap::new();
    for component in components {
        if let Some(canonical) = canonical_singleton_component_name(&component.name) {
            by_name.entry(canonical).or_default().push(component);
        }
    }
    Some(by_name)
}

fn component_block<'a>(doc: &'a str, component: &element::Component) -> &'a str {
    &doc[component.open_start..component.close_end]
}

pub fn repair_duplicate_singleton_components(
    before: Option<&str>,
    content: &str,
) -> Option<DuplicateSingletonComponentRepair> {
    let before = before?;
    let content_groups = singleton_components_by_name(content)?;
    let duplicate_groups: Vec<(&'static str, Vec<element::Component>)> = content_groups
        .iter()
        .filter(|(_, components)| components.len() > 1)
        .map(|(name, components)| (*name, components.clone()))
        .collect();
    if duplicate_groups.is_empty() {
        return None;
    }

    let before_groups = singleton_components_by_name(before)?;

    let mut remove_ranges: Vec<(usize, usize)> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    for (name, components) in duplicate_groups {
        let group_len = components.len();
        let before_components = before_groups.get(name)?;
        if before_components.len() != 1 {
            return None;
        }
        let canonical_block = component_block(before, &before_components[0]);
        let canonical_matches: Vec<&element::Component> = components
            .iter()
            .filter(|component| component_block(content, component) == canonical_block)
            .collect();
        if canonical_matches.len() != 1 {
            return None;
        }
        let keep = (
            canonical_matches[0].open_start,
            canonical_matches[0].close_end,
        );
        for component in components {
            let range = (component.open_start, component.close_end);
            if range != keep {
                remove_ranges.push(range);
            }
        }
        groups.push(format!("{name}={group_len}"));
    }

    remove_ranges.sort_by_key(|range| std::cmp::Reverse(range.0));
    remove_ranges.dedup();
    let removed = remove_ranges.len();
    if removed == 0 {
        return None;
    }

    let mut repaired = content.to_string();
    for (start, end) in remove_ranges {
        repaired.replace_range(start..end, "");
    }

    Some(DuplicateSingletonComponentRepair {
        content: repaired,
        groups,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_duplicate_singleton_component_from_before_content() {
        let before = concat!(
            "<!-- agent:status -->\n",
            "ready\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#canonical]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#canonical] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before.replace(
            "<!-- agent:backlog -->",
            "<!-- agent:queue preset=\"#stale\" priority go -->\n- do [#stale]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->",
        );

        let repair = repair_duplicate_singleton_components(Some(before), &after).expect("repair");

        assert_eq!(repair.groups, vec!["queue=2"]);
        assert_eq!(repair.removed, 1);
        assert_eq!(repair.content.matches("<!-- agent:queue").count(), 1);
        assert!(repair.content.contains("- do [#canonical]"));
        assert!(!repair.content.contains("- do [#stale]"));
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&repair.content),
            None
        );
    }

    #[test]
    fn leaves_ambiguous_duplicates_unrepaired() {
        let before = concat!(
            "<!-- agent:queue -->\n",
            "- do [#canonical]\n",
            "<!-- /agent:queue -->\n"
        );
        let after = concat!(
            "<!-- agent:queue -->\n",
            "- do [#stale]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#other]\n",
            "<!-- /agent:queue -->\n"
        );

        assert!(repair_duplicate_singleton_components(Some(before), after).is_none());
    }
}
