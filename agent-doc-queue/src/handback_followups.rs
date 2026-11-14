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
//! closed. An id-less origin is quoted by its work text. A lifecycle marker
//! or exact-attempt annotation on that row is not part of the quote, and a
//! row that already includes one still matches after the marker changes
//! (`#followuporiginlabelmarker`).
//!
//! `#handbackopenitemstriage`: a row can dispatch in a `go` queue with no
//! review, so only an entry the worker labelled as work becomes one. **Notes**
//! never do. An **Open items** entry says nothing about which it is, so it
//! stays in the cell and the report line asks the coordinator to triage it.
//!
//! `#humanfollowupowner`: a row an agent cannot close must not read as agent
//! work. Each action item is classified by owner ([`crate::followup_owner`]):
//! the worker's `agent:` / `operator:` / `human (<who>):` label, else a
//! conservative vocabulary of actions only a person can take. A human-owned
//! row carries `[operator-verify]`, so no drain scope dispatches it, preflight
//! projects `⏳` onto it, and the Stop hook's `#transientcloseout` gate waits
//! on the operator instead of refusing every turn end over it.
//!
//! `#handbackrefusedcommandexact`: when that placement cannot be proven, the
//! report line prints one `queue follow-up` command on its own line. `--after`
//! is the originating head and each `--item` is a follow-up row that was not
//! placed, shell-quoted, so the line can be run as printed. An action item
//! withheld because it carries an HTML comment or component marker is not one
//! of those items: a live queue row must not hold it.

/// What became of a hand-back's item sections.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HandbackFollowups {
    /// Queue that received (or would have received) the rows.
    pub queue_id: Option<String>,
    /// Row texts inserted after the originating head, in order.
    pub inserted: Vec<String>,
    /// How many of `inserted` are human-owned and wait on the operator
    /// (`[operator-verify]`, `#humanfollowupowner`).
    pub operator_owned: usize,
    /// Action items whose row the queue already holds (live or struck).
    pub already_present: usize,
    /// Open items held for coordinator triage, not queued.
    pub held_for_triage: usize,
    /// Why the action items could not be queued. The worker result is still
    /// recorded; the coordinator queues the placeable rows with `queue follow-up`.
    pub refused: Option<String>,
    /// Origin row text (`--after`) for a placement the command must retry.
    pub refused_origin: Option<String>,
    /// Follow-up row texts (`--item`) that placement refused, in order.
    /// Marker-bearing action items are omitted (`#handbackrefusedcommandexact`).
    pub refused_items: Vec<String>,
}

/// One copy-pasteable `queue follow-up` for a refused placement.
///
/// The row texts are the same ones [`plan_handback_followups`] would have
/// inserted. Shell words follow [`crate::reported_closeout::shell_command`],
/// so a coordinator can run the line as printed.
pub fn refused_placement_command(
    file: &str,
    queue_id: &str,
    origin: &str,
    items: &[String],
) -> String {
    let mut argv = vec![
        "agent-doc".to_string(),
        "queue".to_string(),
        "follow-up".to_string(),
        file.to_string(),
        "--id".to_string(),
        queue_id.to_string(),
        "--after".to_string(),
        origin.to_string(),
    ];
    for item in items {
        argv.push("--item".to_string());
        argv.push(item.clone());
    }
    crate::reported_closeout::shell_command(&argv)
}

/// Longest origin label a follow-up row quotes for an id-less queue item.
const FOLLOWUP_ORIGIN_LABEL_CHARS: usize = 80;

/// The follow-up row text for one action item. The origin id is written as a
/// code span without `#`, so the row is free-text work that never aliases the
/// originating item's identity. An id-less origin is quoted by its work text
/// ([`origin_work_text`]), bounded to [`FOLLOWUP_ORIGIN_LABEL_CHARS`], so a
/// later report still produces the same row after the origin's lifecycle
/// marker changes (`#followuporiginlabelmarker`).
pub fn action_item_followup_text(origin_item: &str, action_item: &str) -> String {
    let origin = match agent_doc_element_queue::queue_head_identity(origin_item) {
        agent_doc_element_queue::QueueItemIdentity::Id(id) => format!("`{id}`"),
        agent_doc_element_queue::QueueItemIdentity::FreeText(_) => {
            let text = agent_doc_document::subagent_completion::heading_item_text(
                &origin_work_text(origin_item),
            );
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

/// Work text of an origin row. A lifecycle marker, pin, or exact-attempt
/// owner annotation is projection, not the quote. The raw row is kept when
/// stripping would leave it empty.
fn origin_work_text(origin_item: &str) -> String {
    let stripped = agent_doc_element_queue::strip_attempt_projection_prefix(origin_item);
    if stripped.trim().is_empty() {
        origin_item.to_string()
    } else {
        stripped.to_string()
    }
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
    let mut queueable: Vec<String> = Vec::new();
    let mut withheld = 0;
    for item in items {
        // The cell quotes a worker's component markers inert
        // (`#completionfeedbackscope`); a live queue row would not.
        if item.contains("<!--") || item.contains("-->") {
            withheld += 1;
            continue;
        }
        // `#humanfollowupowner`: a human-owned item's row waits on the
        // operator instead of reading as dispatchable agent work.
        queueable.push(crate::followup_owner::owned_followup_row(
            action_item_followup_text(&origin_row, &item),
            &item,
        ));
    }
    let withheld_reason = (withheld > 0).then(|| {
        format!(
            "{withheld} action item(s) carry an HTML comment or component marker, which a live queue row must not hold"
        )
    });
    let Some(queue_id) = crate::subagent_brief::origin_queue_id_for_item(content, origin_item)
    else {
        outcome.refused = Some(match &withheld_reason {
            Some(withheld) => {
                format!("the document has no queue holding the originating head; {withheld}")
            }
            None => "the document has no queue holding the originating head".to_string(),
        });
        if !queueable.is_empty() {
            outcome.refused_origin = Some(origin_row);
            outcome.refused_items = queueable;
        }
        return (content.to_string(), outcome);
    };
    outcome.queue_id = Some(queue_id.clone());
    let existing = queue_row_lines(content, &queue_id);
    let mut fresh: Vec<String> = Vec::new();
    for row in queueable {
        if fresh.iter().any(|held| followup_rows_match(held, &row))
            || existing.iter().any(|line| followup_rows_match(line, &row))
        {
            outcome.already_present += 1;
            continue;
        }
        fresh.push(row);
    }
    if let Some(withheld) = withheld_reason {
        outcome.refused = Some(withheld);
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
            outcome.operator_owned = fresh
                .iter()
                .filter(|row| crate::followup_owner::carries_operator_owned_tag(row))
                .count();
            outcome.inserted = fresh;
            (plan.content, outcome)
        }
        Err(err) => {
            let placement = format!("{err:#}");
            outcome.refused = Some(match outcome.refused.take() {
                Some(prior) => format!("{placement}; {prior}"),
                None => placement,
            });
            outcome.refused_origin = Some(origin_row);
            outcome.refused_items = fresh;
            (content.to_string(), outcome)
        }
    }
}

/// True when `line` already holds the follow-up `row`, live or struck.
///
/// A quote written before `#followuporiginlabelmarker` embedded the origin's
/// lifecycle marker (`☑️ [#subagent:leaf#1]`). That marker changes between
/// reports, and it used to consume the 80-character bound, so the check
/// compares the quote's work text. A struck row still matches.
fn followup_rows_match(line: &str, row: &str) -> bool {
    let line_key = followup_hold_key(line);
    let row_key = followup_hold_key(row);
    if line_key.contains(&row_key) {
        return true;
    }
    let (Some((old_label, old_action)), Some((new_label, new_action))) = (
        quoted_origin_parts(&line_key),
        quoted_origin_parts(&row_key),
    ) else {
        return false;
    };
    if old_action != new_action {
        return false;
    }
    // Only a truncated quote may match a longer one. Two short, different
    // work texts that share a prefix stay distinct.
    if !(old_label.ends_with('…') || new_label.ends_with('…')) {
        return false;
    }
    let old_work = old_label.trim_end_matches('…');
    let new_work = new_label.trim_end_matches('…');
    !old_work.is_empty()
        && !new_work.is_empty()
        && (new_work.starts_with(old_work) || old_work.starts_with(new_work))
}

fn followup_hold_key(line: &str) -> String {
    // `#humanfollowupowner`: the owner tag is not part of the row's identity.
    // A row written before the tag existed, or one the reclassification has
    // since tagged, still holds the same action item.
    let collapsed = line
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case(crate::followup_owner::OPERATOR_OWNED_TAG))
        .collect::<Vec<_>>()
        .join(" ");
    normalize_quoted_origin(&collapsed.replace("~~", ""))
}

const ORIGIN_QUOTE_OPEN: &str = "queue item “";

/// `queue item “<label>”` with a leading attempt projection removed from
/// `<label>`. Other lines are unchanged.
fn normalize_quoted_origin(line: &str) -> String {
    let Some((label, end)) = quoted_label_span(line) else {
        return line.to_string();
    };
    let stripped = agent_doc_element_queue::strip_attempt_projection_prefix(label).trim();
    let work = if stripped.is_empty() { label } else { stripped };
    if work == label {
        return line.to_string();
    }
    let start = end - label.len();
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..start]);
    out.push_str(work);
    out.push_str(&line[end..]);
    out
}

/// The quoted origin label and the byte index of its closing `”`.
fn quoted_label_span(line: &str) -> Option<(&str, usize)> {
    let start = line.find(ORIGIN_QUOTE_OPEN)? + ORIGIN_QUOTE_OPEN.len();
    let rest = line.get(start..)?;
    let rel = rest.find('”')?;
    Some((&rest[..rel], start + rel))
}

/// `(label, action)` of `queue item “<label>”: <action>`.
fn quoted_origin_parts(line: &str) -> Option<(&str, &str)> {
    let (label, end) = quoted_label_span(line)?;
    let action = line[end..].strip_prefix("”: ")?;
    Some((label, action))
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

    /// `#followuporiginmarkers`: on `agent-doc-bugs.md` the dispatched row was
    /// `- ### ☑️ [#subagent:x#1] do [#id] ⟦published⟧`. Placement refused it,
    /// so `report-complete` printed a `queue follow-up` command that refused
    /// the same origin. Heading and list rows place directly, by the claim
    /// text and by the plain id form.
    #[test]
    fn action_items_place_after_a_decorated_heading_or_list_row() {
        for row in [
            "### ☑️ [#subagent:x#1] do [#decisionidstamp] ⟦published⟧",
            "### 🚧 [#subagent:x#1] do [#decisionidstamp]",
            "☑️ [#subagent:x#1] do [#decisionidstamp] ⟦published⟧",
        ] {
            let content = doc(&format!(
                "- ### ~~do [#done]~~\n- {row}\n  - push\n- ### later\n"
            ));
            for origin in ["do [#decisionidstamp]", "[#decisionidstamp]"] {
                let (updated, outcome) = plan_handback_followups(&content, origin, HANDBACK);
                assert_eq!(outcome.refused, None, "row {row:?} origin {origin:?}");
                assert_eq!(outcome.refused_origin, None);
                assert_eq!(outcome.inserted.len(), 2, "row {row:?} origin {origin:?}");
                assert!(
                    updated.contains(&format!(
                        "- {row}\n  - push\n\
                         - follow up on an action item from `decisionidstamp`: >512-commit documents can miss answers\n\
                         - follow up on an action item from `decisionidstamp`: the gutter does not show the stamp\n\
                         - ### later\n"
                    )),
                    "row {row:?} origin {origin:?}:\n{updated}"
                );
                let (again, repeat) = plan_handback_followups(&updated, origin, HANDBACK);
                assert_eq!(again, updated);
                assert_eq!(repeat.already_present, 2);
            }
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

        // The 80-character bound is work text. A lifecycle marker must not
        // spend it, or a later marker would cut the quote in a new place.
        let marked = format!("☑️ [#subagent:leaf#1] {}", "x".repeat(90));
        let quoted = action_item_followup_text(&marked, "wire it");
        assert!(
            !quoted.contains("[#subagent:leaf#1]") && !quoted.contains('☑'),
            "{quoted}"
        );
        let label = quoted
            .split_once("queue item “")
            .and_then(|(_, rest)| rest.split_once('”'))
            .map(|(label, _)| label)
            .unwrap();
        assert_eq!(
            label.chars().count(),
            FOLLOWUP_ORIGIN_LABEL_CHARS + 1,
            "{label}"
        );
        assert!(
            label.starts_with(&"x".repeat(FOLLOWUP_ORIGIN_LABEL_CHARS)),
            "{label}"
        );
    }

    /// `#followuporiginlabelmarker`: the nested-leaf hand-back quoted
    /// `☑️ [#subagent:leaf#1]`. A repeat after that marker changes, and a row
    /// already written with the marker, are the same follow-up.
    #[test]
    fn a_repeat_after_the_origin_marker_changes_does_not_add_a_second_row() {
        let handback = concat!(
            "**Summary:** done\n",
            "**Action items:**\n",
            "- wire the gutter stamp\n",
        );
        let content = doc("- ☑️ [#subagent:leaf#1] leaf work\n- later work\n");
        let (once, first) = plan_handback_followups(&content, "leaf work", handback);
        assert_eq!(
            first.inserted,
            vec![
                "follow up on an action item from queue item “leaf work”: wire the gutter stamp"
                    .to_string()
            ]
        );
        assert_eq!(once.matches("[#subagent:leaf#1]").count(), 1, "{once}");

        let changed = once.replacen(
            "☑️ [#subagent:leaf#1] leaf work",
            "🚧 [#subagent:leaf#2] leaf work",
            1,
        );
        let (twice, second) = plan_handback_followups(&changed, "leaf work", handback);
        assert_eq!(twice, changed);
        assert_eq!(second.already_present, 1);
        assert!(second.inserted.is_empty(), "{second:?}");

        // A row the previous binary wrote, quoting the old marker, stays the
        // one follow-up after the origin is re-projected. A struck copy stays
        // closed. A different action item is still queued.
        let legacy = doc(concat!(
            "- 🚧 [#subagent:leaf#2] leaf work\n",
            "- follow up on an action item from queue item “☑️ [#subagent:leaf#1] leaf work”: wire the gutter stamp\n",
            "- later work\n",
        ));
        let (same, outcome) = plan_handback_followups(&legacy, "leaf work", handback);
        assert_eq!(same, legacy);
        assert_eq!(outcome.already_present, 1, "{outcome:?}");
        assert!(outcome.inserted.is_empty(), "{outcome:?}");

        let struck = legacy.replacen(
            "- follow up on an action item from queue item “☑️ [#subagent:leaf#1] leaf work”: wire the gutter stamp",
            "- ~~follow up on an action item from queue item “☑️ [#subagent:leaf#1] leaf work”: wire the gutter stamp~~",
            1,
        );
        let (after, again) = plan_handback_followups(&struck, "leaf work", handback);
        assert_eq!(after, struck);
        assert_eq!(again.already_present, 1, "{again:?}");

        let other = "**Summary:** done\n**Action items:**\n- a different limit\n";
        let (added, extra) = plan_handback_followups(&legacy, "leaf work", other);
        assert_eq!(
            extra.inserted,
            vec![
                "follow up on an action item from queue item “leaf work”: a different limit"
                    .to_string()
            ]
        );
        assert_eq!(added.matches("follow up on").count(), 2, "{added}");
    }

    /// The old quote spent the 80-character bound on the marker, so its work
    /// text is a shorter truncated prefix of the stable quote.
    #[test]
    fn a_marker_inside_the_old_quote_bound_still_matches() {
        let work = "x".repeat(90);
        let legacy_origin = format!("☑️ [#subagent:leaf#1] {work}");
        let legacy_text =
            agent_doc_document::subagent_completion::heading_item_text(&legacy_origin);
        let mut legacy_label: String = legacy_text
            .chars()
            .take(FOLLOWUP_ORIGIN_LABEL_CHARS)
            .collect();
        assert!(legacy_label.len() < legacy_text.len());
        legacy_label.push('…');
        assert!(
            legacy_label.contains("[#subagent:leaf#1]"),
            "{legacy_label}"
        );
        let content = doc(&format!(
            "- 🚧 [#subagent:leaf#2] {work}\n- follow up on an action item from queue item “{legacy_label}”: wire the gutter stamp\n"
        ));
        let handback = "**Summary:** done\n**Action items:**\n- wire the gutter stamp\n";
        let origin = format!("🚧 [#subagent:leaf#2] {work}");
        let (updated, outcome) = plan_handback_followups(&content, &origin, handback);
        assert_eq!(updated, content);
        assert_eq!(outcome.already_present, 1, "{outcome:?}");
        assert!(outcome.inserted.is_empty(), "{outcome:?}");
    }

    /// `#handbackrefusedcommandexact`: a refused placement keeps the origin
    /// and the row texts the report command must print. A marker-bearing
    /// action item is not among them.
    #[test]
    fn a_refused_placement_records_the_origin_and_the_unplaced_rows() {
        let ambiguous = doc("- do [#decisionidstamp]\n- do [#decisionidstamp]\n");
        let handback = concat!(
            "**Summary:** done\n",
            "**Action items:**\n",
            "- a stray <!-- /agent:queue --> closes the queue early\n",
            "- an ordinary limit\n",
        );
        let (same, outcome) =
            plan_handback_followups(&ambiguous, "do [#decisionidstamp]", handback);
        assert_eq!(same, ambiguous);
        assert!(outcome.inserted.is_empty());
        assert_eq!(
            outcome.refused_origin.as_deref(),
            Some("do [#decisionidstamp]")
        );
        assert_eq!(
            outcome.refused_items,
            vec![
                "follow up on an action item from `decisionidstamp`: an ordinary limit".to_string()
            ]
        );
        assert!(
            outcome
                .refused
                .as_deref()
                .is_some_and(|why| why.contains("ambiguous") && why.contains("1 action item")),
            "{outcome:?}"
        );
        let command = refused_placement_command(
            "tasks/session.md",
            "main",
            outcome.refused_origin.as_deref().unwrap(),
            &outcome.refused_items,
        );
        assert_eq!(
            command,
            "agent-doc queue follow-up tasks/session.md --id main --after 'do [#decisionidstamp]' --item 'follow up on an action item from `decisionidstamp`: an ordinary limit'"
        );
        assert!(!command.contains("<origin>") && !command.contains("<text>"));

        let bare = "---\nagent_doc_session: s\n---\n\nno queue here\n";
        let (_, missing) = plan_handback_followups(bare, "do [#decisionidstamp]", HANDBACK);
        assert!(
            missing
                .refused
                .as_deref()
                .is_some_and(|why| why.contains("no queue")),
            "{missing:?}"
        );
        assert_eq!(
            missing.refused_origin.as_deref(),
            Some("do [#decisionidstamp]")
        );
        assert_eq!(missing.refused_items.len(), 2, "{missing:?}");

        let quoted = refused_placement_command(
            "tasks/my session.md",
            "main",
            "do [#alpha]",
            &["follow up on an action item from `alpha`: it's `done`".to_string()],
        );
        assert_eq!(
            quoted,
            "agent-doc queue follow-up 'tasks/my session.md' --id main --after 'do [#alpha]' --item 'follow up on an action item from `alpha`: it'\\''s `done`'"
        );
    }

    /// `#humanfollowupowner`: an owner-labelled hand-back. The agent bullet
    /// stays dispatchable work; the human and operator bullets, and an
    /// unlabelled approval, wait on the operator with `[operator-verify]`.
    #[test]
    fn human_owned_action_items_wait_on_the_operator() {
        let handback = concat!(
            "**Summary:** opened the PR.\n",
            "**Action items:**\n",
            "- agent: merge #240 once hosted CI is green\n",
            "- human (tmstorm): approve PR #100 at head `38075e7`\n",
            "- operator: pick stable `0.1.0` or a preview\n",
            "- A non-author approves PR 9\n",
            "- Fix the typo in README.md\n",
        );
        let content = doc("- do [#decisionidstamp]\n- later work\n");
        let (updated, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", handback);
        let from = "follow up on an action item from `decisionidstamp`:";
        assert_eq!(
            outcome.inserted,
            vec![
                format!("{from} agent: merge #240 once hosted CI is green"),
                format!(
                    "{from} human (tmstorm): approve PR #100 at head `38075e7` [operator-verify]"
                ),
                format!("{from} operator: pick stable `0.1.0` or a preview [operator-verify]"),
                format!("{from} A non-author approves PR 9 [operator-verify]"),
                format!("{from} Fix the typo in README.md"),
            ]
        );
        assert_eq!(outcome.operator_owned, 3);
        for row in &outcome.inserted {
            assert_eq!(
                crate::queue_continuation::head_awaits_operator_inline(row),
                row.contains("[operator-verify]"),
                "{row}"
            );
        }
        let (again, repeat) = plan_handback_followups(&updated, "do [#decisionidstamp]", handback);
        assert_eq!(again, updated);
        assert_eq!(repeat.already_present, 5);
    }

    /// A row written before the owner tag existed, live or struck, still holds
    /// its action item: a repeated report adds no tagged duplicate.
    #[test]
    fn an_untagged_legacy_row_holds_its_human_owned_item() {
        let handback = "**Summary:** done\n**Action items:**\n- A non-author approves PR 9\n";
        for legacy in [
            "- 📥 follow up on an action item from `decisionidstamp`: A non-author approves PR 9 [#q-0123456789ab]\n",
            "- ~~follow up on an action item from `decisionidstamp`: A non-author approves PR 9~~\n",
        ] {
            let content = doc(&format!("- do [#decisionidstamp]\n{legacy}"));
            let (same, outcome) =
                plan_handback_followups(&content, "do [#decisionidstamp]", handback);
            assert_eq!(same, content, "{legacy}");
            assert_eq!(outcome.already_present, 1, "{legacy}");
        }
    }
}
