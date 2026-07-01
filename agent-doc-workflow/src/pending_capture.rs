//! Pure pending-capture closeout policy.
//!
//! Callers gather filesystem/cycle-state evidence. This module parses response
//! text and evaluates promised backlog items, plan-reference candidates, and
//! shortfall/missing-id decisions without touching disk.

use std::collections::HashSet;

use agent_doc_element_backlog::backlog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryShortfall {
    pub expected: usize,
    pub actual: usize,
}

impl InventoryShortfall {
    pub const fn as_tuple(self) -> (usize, usize) {
        (self.expected, self.actual)
    }
}

pub fn promised_backlog_item_ids_from_response<I, S>(
    response_text: &str,
    baseline_item_ids: I,
) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let baseline_ids: HashSet<String> = baseline_item_ids
        .into_iter()
        .map(|id| backlog::normalize_pending_id(id.as_ref()))
        .collect();
    let (_, items, _) = backlog::parse_items(response_text);
    let mut promised = Vec::new();
    for item in items.into_iter().filter(|item| !item.is_done()) {
        let id = backlog::normalize_pending_id(&item.id);
        if id.is_empty()
            || baseline_ids.contains(&id)
            || promised.iter().any(|existing| existing == &id)
        {
            continue;
        }
        promised.push(id);
    }
    promised
}

pub fn promised_backlog_item_inventory_shortfall<I, S>(
    response_text: &str,
    baseline_item_ids: I,
    required_target_count: usize,
    required_explicit_item_count: usize,
) -> Option<InventoryShortfall>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if required_target_count == 0 || required_explicit_item_count == 0 {
        return None;
    }

    let promised_count =
        promised_backlog_item_ids_from_response(response_text, baseline_item_ids).len();
    inventory_shortfall(required_explicit_item_count, promised_count)
}

pub fn inventory_shortfall(expected: usize, actual: usize) -> Option<InventoryShortfall> {
    (actual < expected).then_some(InventoryShortfall { expected, actual })
}

pub fn promised_plan_reference_candidate_lines(response_text: &str) -> Vec<String> {
    response_text
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("<!--")
                && !line.starts_with('>')
                && line.to_ascii_lowercase().contains("plan")
        })
        .map(ToOwned::to_owned)
        .collect()
}

pub fn promised_plan_reference_shortfall(
    required_plan_reference_count: usize,
    promised_plan_reference_count: usize,
) -> Option<InventoryShortfall> {
    if required_plan_reference_count == 0 {
        return None;
    }
    inventory_shortfall(required_plan_reference_count, promised_plan_reference_count)
}

pub fn missing_promised_backlog_item_ids<I, S, J, T>(
    promised_item_ids: I,
    current_target_ids: J,
) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    J: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let current_target_ids: HashSet<String> = current_target_ids
        .into_iter()
        .map(|id| backlog::normalize_pending_id(id.as_ref()))
        .collect();
    let mut missing = Vec::new();
    for id in promised_item_ids {
        let id = backlog::normalize_pending_id(id.as_ref());
        if id.is_empty()
            || current_target_ids.contains(&id)
            || missing.iter().any(|existing| existing == &id)
        {
            continue;
        }
        missing.push(id);
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promised_backlog_ids_exclude_baseline_done_empty_and_duplicates() {
        let response = concat!(
            "- [ ] [#New-Item] transfer first bug\n",
            "- [ ] [#new-item] duplicate promise\n",
            "- [x] [#done-item] completed already\n",
            "- [ ] no id\n",
            "- [/] [#Gate-Me] wait for release\n",
            "- [ ] [#Existing] already in target\n",
        );

        assert_eq!(
            promised_backlog_item_ids_from_response(response, ["existing"]),
            vec!["new-item".to_string(), "gate-me".to_string()]
        );
    }

    #[test]
    fn backlog_inventory_shortfall_requires_target_and_required_count() {
        let response = "- [ ] [#one] first\n";

        assert_eq!(
            promised_backlog_item_inventory_shortfall(response, std::iter::empty::<&str>(), 1, 2),
            Some(InventoryShortfall {
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(
            promised_backlog_item_inventory_shortfall(response, std::iter::empty::<&str>(), 0, 2),
            None
        );
        assert_eq!(
            promised_backlog_item_inventory_shortfall(response, std::iter::empty::<&str>(), 1, 0),
            None
        );
        assert_eq!(
            promised_backlog_item_inventory_shortfall(
                "- [ ] [#one] first\n- [ ] [#two] second\n",
                std::iter::empty::<&str>(),
                1,
                2,
            ),
            None
        );
    }

    #[test]
    fn plan_reference_candidates_ignore_comments_quotes_and_non_plan_lines() {
        let response = concat!(
            "<!-- Plan: tasks/agent-doc/plan-hidden.md -->\n",
            "> Plan: tasks/agent-doc/plan-quoted.md\n",
            "See tasks/agent-doc/plan-extract.md for details\n",
            "Backlog: tasks/agent-doc/bug.md\n",
            "PLAN: docs/plan-upper.md\n",
        );

        assert_eq!(
            promised_plan_reference_candidate_lines(response),
            vec![
                "See tasks/agent-doc/plan-extract.md for details".to_string(),
                "PLAN: docs/plan-upper.md".to_string(),
            ]
        );
    }

    #[test]
    fn plan_reference_shortfall_compares_required_and_promised_counts() {
        assert_eq!(
            promised_plan_reference_shortfall(2, 1),
            Some(InventoryShortfall {
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(promised_plan_reference_shortfall(2, 2), None);
        assert_eq!(promised_plan_reference_shortfall(0, 0), None);
    }

    #[test]
    fn missing_promised_ids_normalize_and_deduplicate() {
        assert_eq!(
            missing_promised_backlog_item_ids(
                ["#New-One", "new-one", "Existing", ""],
                ["existing", "other"]
            ),
            vec!["new-one".to_string()]
        );
    }
}
