//! `#workerresponsestructure`: a worker hand-back's **Action items** become
//! follow-up queue rows.
//!
//! `agent-doc queue report-complete` renders a labelled hand-back into the
//! response structure (`agent_doc_document::worker_handback`) and, in the
//! same document successor as the completion cell, inserts one follow-up row
//! per action item immediately after the originating head in its origin queue
//! (the queue the dispatch brief named), through the existing follow-up
//! placement ([`crate::queue_inbound::plan_queue_followups_targeted`]).
//! Rows the queue already holds, live or struck, are not added again, so a
//! repeated report is idempotent and a follow-up the operator struck stays
//! closed.
//!
//! `#handbackopenitemstriage`: a row can dispatch in a `go` queue with no
//! review, so only an entry the worker labelled as work becomes one. **Notes**
//! never do. An **Open items** entry says nothing about which it is, so it
//! stays in the cell and the report line asks the coordinator to triage it.

/// What became of a hand-back's item sections.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HandbackFollowups {
    /// Queue that received (or would have received) the rows.
    pub queue_id: Option<String>,
    /// Row texts inserted after the originating head, in order.
    pub inserted: Vec<String>,
    /// Action items whose row the queue already holds (live or struck).
    pub already_present: usize,
    /// Open items held for coordinator triage, not queued.
    pub held_for_triage: usize,
    /// Why the action items could not be queued. The worker result is still
    /// recorded; the coordinator queues them with `queue follow-up`.
    pub refused: Option<String>,
}

/// Longest origin label a follow-up row quotes for an id-less queue item.
const FOLLOWUP_ORIGIN_LABEL_CHARS: usize = 80;

/// The follow-up row text for one action item. The origin id is written as a
/// code span without `#`, so the row is free-text work that never aliases the
/// originating item's identity.
pub fn action_item_followup_text(origin_item: &str, action_item: &str) -> String {
    let origin = match agent_doc_element_queue::queue_head_identity(origin_item) {
        agent_doc_element_queue::QueueItemIdentity::Id(id) => format!("`{id}`"),
        agent_doc_element_queue::QueueItemIdentity::FreeText(_) => {
            let text = agent_doc_document::subagent_completion::heading_item_text(origin_item);
            let mut label: String = text.chars().take(FOLLOWUP_ORIGIN_LABEL_CHARS).collect();
            if label.len() < text.len() {
                label.push('…');
            }
            format!("queue item “{label}”")
        }
    };
    format!("follow up on an action item from {origin}: {action_item}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Insert one follow-up row per action item of `response` after the head
/// `origin_item` in its origin queue, skipping rows that queue already holds.
/// A placement that cannot be proven (no queue, origin missing or ambiguous)
/// leaves `content` unchanged and reports why.
pub fn plan_handback_followups(
    content: &str,
    origin_item: &str,
    response: &str,
) -> (String, HandbackFollowups) {
    let mut outcome = HandbackFollowups {
        held_for_triage: agent_doc_document::worker_handback::open_items(response).len(),
        ..HandbackFollowups::default()
    };
    if outcome.held_for_triage > 0 {
        outcome.queue_id = crate::subagent_brief::origin_queue_id_for_item(content, origin_item);
    }
    let items = agent_doc_document::worker_handback::action_items(response);
    if items.is_empty() {
        return (content.to_string(), outcome);
    }
    // A nest-vector claim key (`parent: leaf`) names the row by lineage; the
    // placement matches the row's own text.
    let origin_row = crate::queue_response::queue_row_text_for_head(content, origin_item)
        .unwrap_or_else(|| origin_item.to_string());
    let Some(queue_id) = crate::subagent_brief::origin_queue_id_for_item(content, origin_item)
    else {
        outcome.refused = Some("the document has no queue holding the originating head".into());
        return (content.to_string(), outcome);
    };
    outcome.queue_id = Some(queue_id.clone());
    let existing = queue_row_lines(content, &queue_id);
    let mut fresh: Vec<String> = Vec::new();
    let mut withheld = 0;
    for item in items {
        // The cell quotes a worker's component markers inert
        // (`#completionfeedbackscope`); a live queue row would not.
        if item.contains("<!--") || item.contains("-->") {
            withheld += 1;
            continue;
        }
        let row = action_item_followup_text(&origin_row, &item);
        if fresh.contains(&row) || existing.iter().any(|line| line.contains(&row)) {
            outcome.already_present += 1;
            continue;
        }
        fresh.push(row);
    }
    if withheld > 0 {
        outcome.refused = Some(format!(
            "{withheld} action item(s) carry an HTML comment or component marker, which a live queue row must not hold"
        ));
    }
    if fresh.is_empty() {
        return (content.to_string(), outcome);
    }
    match crate::queue_inbound::plan_queue_followups_targeted(
        content,
        &queue_id,
        &origin_row,
        &fresh,
    ) {
        Ok(plan) => {
            outcome.queue_id = Some(plan.queue_id);
            outcome.inserted = fresh;
            (plan.content, outcome)
        }
        Err(err) => {
            outcome.refused = Some(format!("{err:#}"));
            (content.to_string(), outcome)
        }
    }
}

/// Whitespace-collapsed lines of queue `queue_id`'s body.
fn queue_row_lines(content: &str, queue_id: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let Ok(Some(queue)) = crate::queue_graph::component_for_id(content, &components, queue_id)
    else {
        return Vec::new();
    };
    queue
        .content(content)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDBACK: &str = concat!(
        "**Summary:** stamped decision ids.\n",
        "**Verification:** make check exit=0\n",
        "**Action items:**\n",
        "- >512-commit documents can miss answers\n",
        "- the gutter does not show the stamp\n",
    );

    fn doc(queue: &str) -> String {
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue id=main -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn action_items_land_after_the_originating_head_with_its_subtree() {
        let content = doc("- ☑️ do [#decisionidstamp]\n  - push\n- later work\n");
        let (updated, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", HANDBACK);
        assert_eq!(outcome.queue_id.as_deref(), Some("main"));
        assert_eq!(outcome.refused, None);
        assert_eq!(
            outcome.inserted,
            vec![
                "follow up on an action item from `decisionidstamp`: >512-commit documents can miss answers"
                    .to_string(),
                "follow up on an action item from `decisionidstamp`: the gutter does not show the stamp"
                    .to_string(),
            ]
        );
        assert!(updated.contains(concat!(
            "- ☑️ do [#decisionidstamp]\n  - push\n",
            "- follow up on an action item from `decisionidstamp`: >512-commit documents can miss answers\n",
            "- follow up on an action item from `decisionidstamp`: the gutter does not show the stamp\n",
            "- later work\n",
        )));
        // Free-text rows: neither aliases the origin's identity.
        for row in &outcome.inserted {
            assert!(!agent_doc_element_queue::queue_head_identity(row).is_id_backed());
        }
    }

    #[test]
    fn a_repeated_report_or_a_struck_followup_adds_nothing() {
        let content = doc("- do [#decisionidstamp]\n");
        let (once, first) = plan_handback_followups(&content, "do [#decisionidstamp]", HANDBACK);
        assert_eq!(first.inserted.len(), 2);
        let (twice, second) = plan_handback_followups(&once, "do [#decisionidstamp]", HANDBACK);
        assert_eq!(twice, once);
        assert_eq!(second.inserted.len(), 0);
        assert_eq!(second.already_present, 2);

        let struck = once.replacen(
            "- follow up on an action item from `decisionidstamp`: the gutter does not show the stamp",
            "- ~~follow up on an action item from `decisionidstamp`: the gutter does not show the stamp~~",
            1,
        );
        let (after, third) = plan_handback_followups(&struck, "do [#decisionidstamp]", HANDBACK);
        assert_eq!(after, struck);
        assert_eq!(third.already_present, 2);
    }

    #[test]
    fn no_open_items_or_an_unplaceable_origin_leaves_the_document_unchanged() {
        let content = doc("- do [#decisionidstamp]\n");
        let none = "**Summary:** done\n**Action items:** none\n";
        let (same, outcome) = plan_handback_followups(&content, "do [#decisionidstamp]", none);
        assert_eq!(same, content);
        assert_eq!(outcome, HandbackFollowups::default());

        let unlabelled = "Done. Open items remain in prose only.";
        let (same, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", unlabelled);
        assert_eq!(same, content);
        assert_eq!(outcome, HandbackFollowups::default());

        let ambiguous = doc("- do [#decisionidstamp]\n- do [#decisionidstamp]\n");
        let (same, outcome) =
            plan_handback_followups(&ambiguous, "do [#decisionidstamp]", HANDBACK);
        assert_eq!(same, ambiguous);
        assert!(outcome.inserted.is_empty());
        assert!(
            outcome
                .refused
                .as_deref()
                .is_some_and(|why| why.contains("ambiguous")),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_component_marker_in_an_action_item_is_withheld_from_the_live_queue() {
        let content = doc("- do [#decisionidstamp]\n");
        let handback = concat!(
            "**Summary:** done\n",
            "**Action items:**\n",
            "- a stray <!-- /agent:queue --> closes the queue early\n",
            "- an ordinary limit\n",
        );
        let (updated, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", handback);
        assert_eq!(
            outcome.inserted,
            vec![
                "follow up on an action item from `decisionidstamp`: an ordinary limit".to_string()
            ]
        );
        assert!(
            outcome
                .refused
                .as_deref()
                .is_some_and(|why| why.contains("1 action item(s)")),
            "{outcome:?}"
        );
        assert_eq!(updated.matches("<!-- /agent:queue -->").count(), 1);
    }

    #[test]
    fn a_nested_row_named_by_its_lineage_key_places_after_that_row() {
        let content = doc("- parent work\n  - leaf work\n  - sibling work\n- later work\n");
        let (updated, outcome) =
            plan_handback_followups(&content, "parent work: leaf work", HANDBACK);
        assert_eq!(outcome.refused, None, "{outcome:?}");
        assert_eq!(outcome.inserted.len(), 2);
        // The siblings keep their parent; the rows follow the whole parent.
        let body = &updated[updated.find("- parent work").unwrap()..];
        let sibling = body.find("  - sibling work").unwrap();
        let row = body.find("- follow up on an action item").unwrap();
        assert!(sibling < row, "{updated}");
        assert!(row < body.find("- later work").unwrap(), "{updated}");
    }

    /// `#handbackopenitemstriage`: the #recommendationinrow hand-back labelled
    /// six notes as Open items, and each became a dispatchable row. Open items
    /// now wait for the coordinator, and only labelled work is queued.
    #[test]
    fn only_action_items_become_rows() {
        let content = doc("- do [#decisionidstamp]\n- later work\n");
        let notes_only = concat!(
            "**Summary:** done\n",
            "**Open items:**\n",
            "- **Cleanup.** `cargo clean` is done.\n",
            "- **Live document.** Nothing was applied to it.\n",
            "**Notes:**\n",
            "- a limit worth reading\n",
        );
        let (same, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", notes_only);
        assert_eq!(same, content);
        assert_eq!(
            outcome,
            HandbackFollowups {
                queue_id: Some("main".to_string()),
                held_for_triage: 2,
                ..HandbackFollowups::default()
            }
        );

        let mixed = format!("{notes_only}**Action items:**\n- wire the gutter stamp\n");
        let (updated, outcome) = plan_handback_followups(&content, "do [#decisionidstamp]", &mixed);
        assert_eq!(
            outcome.inserted,
            vec![
                "follow up on an action item from `decisionidstamp`: wire the gutter stamp"
                    .to_string()
            ]
        );
        assert_eq!(outcome.held_for_triage, 2);
        assert_eq!(updated.matches("follow up on").count(), 1, "{updated}");
    }

    #[test]
    fn an_idless_origin_is_quoted_and_bounded() {
        let long = "fix the thing ".repeat(12);
        let row = action_item_followup_text(&long, "a limit");
        assert!(row.starts_with("follow up on an action item from queue item “fix the thing"));
        assert!(row.contains("…”: a limit"), "{row}");
        assert_eq!(
            action_item_followup_text("finish sdk", "a  limit\nwrapped"),
            "follow up on an action item from queue item “finish sdk”: a limit wrapped"
        );
    }
}
