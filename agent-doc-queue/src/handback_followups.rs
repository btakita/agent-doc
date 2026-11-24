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
//! rests it as `🙋` rather than the agent inbox `📥`, and the Stop hook's
//! `#transientcloseout` gate waits on the operator instead of refusing every
//! turn end over it.
//!
//! `#selfrepaircleanup`: an `operator:` item that asks to delete or clean
//! one of agent-doc's own document artifacts (an extra struck copy, a copied
//! response tree, a stale claim label) is not queued as a `🙋` row; the
//! binary's self-repair pass does it ([`crate::self_repair`]).
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
    /// `#selfrepaircleanup`: operator asks to clean up agent-doc's own
    /// document artifacts (duplicate struck rows, a copied response tree, a
    /// stale claim label). agent-doc repairs those itself on the next sync
    /// ([`crate::self_repair`]), so they are not queued as `🙋` rows.
    pub self_repair_items: Vec<String>,
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

/// Longest origin label a legacy follow-up row quotes for an id-less queue
/// item.
const FOLLOWUP_ORIGIN_LABEL_CHARS: usize = 80;

/// The follow-up row text for one action item (`#followupshorttitle`):
/// `[#<origin-id>] follow-up: <item>`.
///
/// The origin is named by its stable id ([`followup_origin_id`]): an authored
/// `[#slug]`, else the binary-admitted `[#q-…]`. The item, owner label
/// included (`operator:`, `agent:`, `human (<who>):`), follows verbatim, so
/// the action leads the row instead of a quote of the origin. A follow-up of
/// a follow-up names only its immediate origin's own id, so titles never
/// nest. `[#origin] follow-up:` is a reference, never this row's identity:
/// every leading-id parser checks
/// [`agent_doc_element_queue::is_followup_reference_tail`] first.
///
/// An origin with no stable id gets one from [`plan_handback_followups_in`]
/// before any row is built. Only when that is impossible (no document, or the
/// admission is ambiguous) does the row fall back to the legacy quoted form
/// ([`legacy_action_item_followup_text`]).
pub fn action_item_followup_text(origin_item: &str, action_item: &str) -> String {
    match followup_origin_id(origin_item) {
        Some(id) => format!(
            "[#{id}] {} {action_item}",
            agent_doc_element_queue::FOLLOWUP_REFERENCE_KEYWORD
        )
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "),
        None => legacy_action_item_followup_text(origin_item, action_item),
    }
}

/// The stable id a follow-up row names its origin by: the origin's authored
/// id (`do [#slug]`, `[#slug] …`), else its trailing binary-admitted
/// `[#q-…]`. The origin's lifecycle projection, `### ` heading, and strike
/// are ignored. `None` for an id-less origin.
pub fn followup_origin_id(origin_item: &str) -> Option<String> {
    let text = origin_item.trim();
    let text = text.strip_prefix("### ").unwrap_or(text).trim();
    // A struck origin (`~~work [#q-…] ⟦published⟧~~`); its work text may
    // quote a strike of its own, so the closing `~~` is the last one.
    let text = text
        .strip_prefix("~~")
        .and_then(|inner| inner.rfind("~~").map(|close| &inner[..close]))
        .unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    agent_doc_element_queue::queue_prompt_stable_id(work)
}

/// The row text an earlier binary wrote for one action item:
/// `follow up on an action item from <origin>: <item>`, the origin an
/// id-backed `` `id` `` or an id-less `queue item “<label>”` quote. New rows
/// use [`action_item_followup_text`]; this form stays parseable
/// ([`crate::followup_owner::followup_row_action_item`]) and is still
/// matched by the dedupe check, so a repeated report never re-adds an item
/// an old row already holds. The origin id is written as a code span without
/// `#`. An id-less origin is quoted by its work text ([`origin_work_text`]),
/// bounded to [`FOLLOWUP_ORIGIN_LABEL_CHARS`], so a later report still
/// produces the same row after the origin's lifecycle marker changes
/// (`#followuporiginlabelmarker`).
pub fn legacy_action_item_followup_text(origin_item: &str, action_item: &str) -> String {
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
    format!(
        "{}{origin}: {action_item}",
        crate::followup_owner::FOLLOWUP_ROW_PREFIX
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

/// Every row text that holds `action_item` for `origin_item`: the current
/// form, then the legacy form an earlier binary wrote. A row in either form,
/// live or struck, means the item is already queued.
pub fn followup_row_forms(origin_item: &str, action_item: &str) -> Vec<String> {
    let current = action_item_followup_text(origin_item, action_item);
    let legacy = legacy_action_item_followup_text(origin_item, action_item);
    if current == legacy {
        vec![current]
    } else {
        vec![current, legacy]
    }
}

/// True when `line` holds the follow-up row of `action_item` for
/// `origin_item` in any form ([`followup_row_forms`]).
pub fn followup_row_held(line: &str, origin_item: &str, action_item: &str) -> bool {
    followup_row_forms(origin_item, action_item)
        .iter()
        .any(|row| followup_rows_match(line, row))
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
    plan_handback_followups_in(content, FALLBACK_DOCUMENT_ID, origin_item, response)
}

/// Document id seed for [`plan_handback_followups`] callers that have no
/// document path. It only seeds the generated origin id; any value is valid.
const FALLBACK_DOCUMENT_ID: &str = "agent-doc:followup-origin";

/// [`plan_handback_followups`] for the document `document_id`
/// (`agent_doc_hash::document_id_for_path`). An id-less origin row is first
/// given its `[#q-…]` id with the same deterministic mint preflight uses
/// ([`crate::prompt_identity::admit_queue_row_id`]), in the same write, so
/// the follow-up rows can name it (`#followupshorttitle`).
pub fn plan_handback_followups_in(
    content: &str,
    document_id: &str,
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
    let original_origin_row = origin_row_text(content, origin_item);
    let mut queueable_items: Vec<String> = Vec::new();
    let mut withheld = 0;
    for item in items {
        // The cell quotes a worker's component markers inert
        // (`#completionfeedbackscope`); a live queue row would not.
        if item.contains("<!--") || item.contains("-->") {
            withheld += 1;
            continue;
        }
        if crate::self_repair::is_self_repair_cleanup_item(&item) {
            outcome.self_repair_items.push(item);
            continue;
        }
        queueable_items.push(item);
    }
    // `#humanfollowupowner`: a human-owned item's row waits on the operator
    // instead of reading as dispatchable agent work.
    let rows_for = |origin_row: &str, items: &[String]| -> Vec<String> {
        items
            .iter()
            .map(|item| {
                crate::followup_owner::owned_followup_row(
                    action_item_followup_text(origin_row, item),
                    item,
                )
            })
            .collect()
    };
    let withheld_reason = (withheld > 0).then(|| {
        format!(
            "{withheld} action item(s) carry an HTML comment or component marker, which a live queue row must not hold"
        )
    });
    // A refused placement leaves the document unchanged, so the rows it
    // reports name the origin as it still reads there.
    let Some(queue_id) = crate::subagent_brief::origin_queue_id_for_item(content, origin_item)
    else {
        outcome.refused = Some(match &withheld_reason {
            Some(withheld) => {
                format!("the document has no queue holding the originating head; {withheld}")
            }
            None => "the document has no queue holding the originating head".to_string(),
        });
        if !queueable_items.is_empty() {
            outcome.refused_items = rows_for(&original_origin_row, &queueable_items);
            outcome.refused_origin = Some(original_origin_row);
        }
        return (content.to_string(), outcome);
    };
    outcome.queue_id = Some(queue_id.clone());
    // `#followupshorttitle`: an id-less origin gets its `[#q-…]` first, so
    // the rows name it by id.
    let (admitted, origin_row) = with_origin_id(content, document_id, original_origin_row.clone());
    let existing = queue_row_lines(&admitted, &queue_id);
    let mut fresh: Vec<String> = Vec::new();
    let mut fresh_items: Vec<String> = Vec::new();
    for item in &queueable_items {
        let row = crate::followup_owner::owned_followup_row(
            action_item_followup_text(&origin_row, item),
            item,
        );
        // A row an earlier binary wrote in the legacy form holds the same
        // item. It quoted the origin as it read before any id was admitted.
        let legacy = [
            legacy_action_item_followup_text(&original_origin_row, item),
            legacy_action_item_followup_text(&origin_row, item),
        ];
        if fresh.iter().any(|held| followup_rows_match(held, &row))
            || existing.iter().any(|line| {
                followup_rows_match(line, &row)
                    || legacy.iter().any(|old| followup_rows_match(line, old))
            })
        {
            outcome.already_present += 1;
            continue;
        }
        fresh.push(row);
        fresh_items.push(item.clone());
    }
    if let Some(withheld) = withheld_reason {
        outcome.refused = Some(withheld);
    }
    if fresh.is_empty() {
        return (content.to_string(), outcome);
    }
    match crate::queue_inbound::plan_queue_followups_targeted(
        &admitted,
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
            outcome.refused_items = rows_for(&original_origin_row, &fresh_items);
            outcome.refused_origin = Some(original_origin_row);
            (content.to_string(), outcome)
        }
    }
}

/// `#followupshorttitle`: `queue follow-up` items written in the legacy title
/// form (`follow up on an action item from <origin>: <item>`), rewritten to
/// `[#<origin-id>] follow-up: <item>` for the `--after` origin. Returns the
/// document (with the origin's `[#q-…]` admitted when it had none and an
/// item needed it), the origin row text placement must match, and the items.
/// Items in any other form are kept verbatim, and when the origin cannot be
/// given an id every item is.
pub fn normalize_followup_items(
    content: &str,
    document_id: &str,
    origin: &str,
    items: &[String],
) -> (String, String, Vec<String>) {
    let legacy_item = |item: &str| -> Option<String> {
        let work = agent_doc_element_queue::strip_attempt_projection_prefix(item);
        if !work
            .trim_start()
            .starts_with(crate::followup_owner::FOLLOWUP_ROW_PREFIX)
        {
            return None;
        }
        crate::followup_owner::followup_row_action_item(work).map(str::to_string)
    };
    if !items.iter().any(|item| legacy_item(item).is_some()) {
        return (content.to_string(), origin.to_string(), items.to_vec());
    }
    let origin_row = origin_row_text(content, origin);
    let (admitted, origin_row) = with_origin_id(content, document_id, origin_row);
    if followup_origin_id(&origin_row).is_none() {
        return (content.to_string(), origin.to_string(), items.to_vec());
    }
    let items = items
        .iter()
        .map(|item| match legacy_item(item) {
            Some(action) => action_item_followup_text(&origin_row, &action),
            None => item.clone(),
        })
        .collect();
    (admitted, origin_row, items)
}

/// The queue row text of the originating head `origin_item`: the row the
/// queue resolver names, else the one live row whose work text equals it once
/// the binary's trailing `[#q-…]` is set aside. A claim keeps the text it was
/// dispatched with, so a repeated report still finds its origin after
/// `#followupshorttitle` (or preflight) admitted the origin's id. Falls back
/// to `origin_item` itself.
fn origin_row_text(content: &str, origin_item: &str) -> String {
    if let Some(row) = crate::queue_response::queue_row_text_for_head(content, origin_item) {
        return row;
    }
    let key = |text: &str| {
        followup_match_key(
            &agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(
                agent_doc_element_queue::strip_attempt_projection_prefix(
                    text.trim().strip_prefix("### ").unwrap_or(text.trim()),
                ),
            ),
        )
    };
    let wanted = key(origin_item);
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return origin_item.to_string();
    };
    let mut found: Option<String> = None;
    for component in components.iter().filter(|c| c.name == "queue") {
        let Ok(entries) = crate::document_queue::parse(component.content(content)) else {
            continue;
        };
        for prompt in crate::document_queue::prompts(&entries) {
            if key(&prompt.text) != wanted {
                continue;
            }
            if found.is_some() {
                return origin_item.to_string();
            }
            found = Some(prompt.text.clone());
        }
    }
    found.unwrap_or_else(|| origin_item.to_string())
}

fn followup_match_key(text: &str) -> String {
    agent_doc_element_queue::normalize_multiline_dedup_text(
        &agent_doc_element_queue::strip_priority_markers(text.trim()),
    )
}

/// `content` with the origin row given its stable id when it has none, and
/// the origin row's text as it now reads (`#followupshorttitle`). The id is
/// admitted only when exactly one queue row carries the origin's work text;
/// otherwise both are returned unchanged and the rows fall back to the legacy
/// quoted form.
fn with_origin_id(content: &str, document_id: &str, origin_row: String) -> (String, String) {
    if followup_origin_id(&origin_row).is_some() {
        return (content.to_string(), origin_row);
    }
    let admitted = crate::prompt_identity::admit_queue_row_id(content, document_id, &origin_row);
    let [assignment] = admitted.assignments.as_slice() else {
        return (content.to_string(), origin_row);
    };
    match queue_row_with_stable_id(&admitted.content, &assignment.id) {
        Some(row) => (admitted.content, row),
        None => (content.to_string(), origin_row),
    }
}

/// The text (after its list bullet) of the one queue row whose stable id is
/// `id`.
fn queue_row_with_stable_id(content: &str, id: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let mut found = None;
    for component in components.iter().filter(|c| c.name == "queue") {
        let entries = crate::document_queue::parse(component.content(content)).ok()?;
        for prompt in crate::document_queue::prompts(&entries) {
            if followup_origin_id(&prompt.text).as_deref() == Some(id) {
                if found.is_some() {
                    return None;
                }
                found = Some(prompt.text.clone());
            }
        }
    }
    found
}

/// True when `line` already holds the follow-up `row`, live or struck.
///
/// A quote written before `#followuporiginlabelmarker` embedded the origin's
/// lifecycle marker (`☑️ [#subagent:leaf#1]`). That marker changes between
/// reports, and it used to consume the 80-character bound, so the check
/// compares the quote's work text. A struck row still matches.
pub(crate) fn followup_rows_match(line: &str, row: &str) -> bool {
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

    const FOLLOWUP_LEGACY_PREFIX: &str = crate::followup_owner::FOLLOWUP_ROW_PREFIX;

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

    /// `#selfrepaircleanup`: a worker that reports an agent-doc-caused
    /// duplicate as an `operator: delete …` item gets no `🙋` row for it;
    /// the self-repair pass removes the duplicate. Other operator work in the
    /// same hand-back still waits on the operator.
    #[test]
    fn agent_doc_duplicate_cleanup_items_are_never_queued_as_operator_rows() {
        let handback = concat!(
            "**Summary:** fixed the merge.\n",
            "**Action items:**\n",
            "- operator: in the live `sample-bugs.md` queue, delete the extra struck copies of `running [#subagent:effectvoid#1] amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`.\n",
            "- operator: in the editor, delete the misplaced copy under ` - ~~Complete the work.~~`: the block from ` - **Response [#r-8eb6516470b1]**` through its Guidance line.\n",
            "- operator: decide whether to revoke the PyPI API token.\n",
        );
        let content = doc("- ☑️ do [#decisionidstamp]\n- later work\n");
        let (updated, outcome) =
            plan_handback_followups(&content, "do [#decisionidstamp]", handback);
        assert_eq!(outcome.self_repair_items.len(), 2, "{outcome:?}");
        assert_eq!(outcome.inserted.len(), 1, "{outcome:?}");
        assert!(outcome.inserted[0].contains("revoke the PyPI API token"));
        assert_eq!(outcome.operator_owned, 1);
        assert!(
            !updated.contains("delete the extra struck copies"),
            "{updated}"
        );
        assert!(!updated.contains("misplaced copy"), "{updated}");
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
                "[#decisionidstamp] follow-up: >512-commit documents can miss answers".to_string(),
                "[#decisionidstamp] follow-up: the gutter does not show the stamp".to_string(),
            ]
        );
        assert!(updated.contains(concat!(
            "- ☑️ do [#decisionidstamp]\n  - push\n",
            "- [#decisionidstamp] follow-up: >512-commit documents can miss answers\n",
            "- [#decisionidstamp] follow-up: the gutter does not show the stamp\n",
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
                         - [#decisionidstamp] follow-up: >512-commit documents can miss answers\n\
                         - [#decisionidstamp] follow-up: the gutter does not show the stamp\n\
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
            "- [#decisionidstamp] follow-up: the gutter does not show the stamp",
            "- ~~[#decisionidstamp] follow-up: the gutter does not show the stamp~~",
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
            vec!["[#decisionidstamp] follow-up: an ordinary limit".to_string()]
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
        // The follow-ups stay at the leaf's depth, after that leaf and before
        // its sibling. They are not hoisted out to the parent.
        let body = &updated[updated.find("- parent work").unwrap()..];
        let leaf = body.find("  - leaf work").unwrap();
        // The id-less leaf is given its `[#q-…]`, and the rows name it.
        let row = body.find("\n  - [#q-").unwrap();
        assert!(body[row..].contains("] follow-up: "), "{updated}");
        let sibling = body.find("  - sibling work").unwrap();
        assert!(leaf < row && row < sibling, "{updated}");
        assert!(sibling < body.find("- later work").unwrap(), "{updated}");
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
            vec!["[#decisionidstamp] follow-up: wire the gutter stamp".to_string()]
        );
        assert_eq!(outcome.held_for_triage, 2);
        assert_eq!(updated.matches("follow-up:").count(), 1, "{updated}");
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
        // `#followupshorttitle`: the id-less origin is given its `[#q-…]`
        // first, and the row names it by that id.
        let origin_id = followup_origin_id(&origin_row_text(&once, "leaf work")).unwrap();
        assert!(origin_id.starts_with("q-"), "{once}");
        assert_eq!(
            first.inserted,
            vec![format!("[#{origin_id}] follow-up: wire the gutter stamp")]
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
        assert_eq!(extra.inserted.len(), 1, "{extra:?}");
        assert!(
            extra.inserted[0].starts_with("[#q-")
                && extra.inserted[0].ends_with("] follow-up: a different limit"),
            "{extra:?}"
        );
        assert_eq!(added.matches("follow up on").count(), 1, "{added}");
        assert_eq!(added.matches("follow-up:").count(), 1, "{added}");
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
            vec!["[#decisionidstamp] follow-up: an ordinary limit".to_string()]
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
            "agent-doc queue follow-up tasks/session.md --id main --after 'do [#decisionidstamp]' --item '[#decisionidstamp] follow-up: an ordinary limit'"
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
            &["[#alpha] follow-up: it's `done`".to_string()],
        );
        assert_eq!(
            quoted,
            "agent-doc queue follow-up 'tasks/my session.md' --id main --after 'do [#alpha]' --item '[#alpha] follow-up: it'\\''s `done`'"
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
        let from = "[#decisionidstamp] follow-up:";
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

    /// `#followupshorttitle`: a follow-up of a follow-up names only its
    /// immediate origin's own id. Titles never nest, whether the origin is a
    /// current-form row or a legacy quoted one.
    #[test]
    fn a_followup_of_a_followup_names_only_its_immediate_origin() {
        let handback = "**Summary:** done\n**Action items:**\n- agent: rerun CI\n";
        for origin in [
            "[#checkadragainst] follow-up: agent: fix the check [#q-0123456789ab]",
            "follow up on an action item from queue item “follow up on an action item from `checkadragainst`: agent: fix…”: agent: fix the check [#q-0123456789ab]",
        ] {
            let content = doc(&format!(
                "- ~~do [#checkadragainst]~~\n- 🚧 [#subagent:x#1] {origin}\n- later\n"
            ));
            let (updated, outcome) = plan_handback_followups(&content, origin, handback);
            assert_eq!(
                outcome.inserted,
                vec!["[#q-0123456789ab] follow-up: agent: rerun CI".to_string()],
                "{origin}"
            );
            assert!(
                updated.contains(&format!(
                    "- 🚧 [#subagent:x#1] {origin}\n- [#q-0123456789ab] follow-up: agent: rerun CI\n- later\n"
                )),
                "{updated}"
            );
            // The new row is free text: it aliases neither its origin nor the
            // origin's own origin.
            let row = &outcome.inserted[0];
            assert!(!agent_doc_element_queue::queue_head_identity(row).is_id_backed());
            assert_eq!(row.matches("follow").count(), 1, "{row}");
            let (again, repeat) = plan_handback_followups(&updated, origin, handback);
            assert_eq!(again, updated);
            assert_eq!(repeat.already_present, 1);
        }
    }

    /// `#followupshorttitle`: an id-less origin gets its `[#q-…]` in the same
    /// write, so the row names it by id rather than quoting its text.
    #[test]
    fn an_idless_origin_is_given_an_id_before_its_rows_name_it() {
        let content = doc("- ☑️ [#subagent:x#1] fix the flaky sync test\n- later\n");
        let (updated, outcome) =
            plan_handback_followups(&content, "fix the flaky sync test", HANDBACK);
        let origin = origin_row_text(&updated, "fix the flaky sync test");
        let id = followup_origin_id(&origin).expect("origin admitted an id");
        assert!(id.starts_with("q-"), "{updated}");
        assert!(
            origin.ends_with(&format!("fix the flaky sync test [#{id}]")),
            "{origin}"
        );
        assert_eq!(
            outcome.inserted,
            vec![
                format!("[#{id}] follow-up: >512-commit documents can miss answers"),
                format!("[#{id}] follow-up: the gutter does not show the stamp"),
            ]
        );
        assert!(!updated.contains("queue item “"), "{updated}");
        // A repeat report by the claim's original text finds the same origin
        // and adds nothing.
        let (again, repeat) =
            plan_handback_followups(&updated, "fix the flaky sync test", HANDBACK);
        assert_eq!(again, updated);
        assert_eq!(repeat.already_present, 2);

        // A refused placement leaves the document, and the origin, unchanged:
        // the printed rows quote the origin as it still reads.
        let ambiguous = doc("- fix it\n- fix it\n");
        let (same, refused) = plan_handback_followups(&ambiguous, "fix it", HANDBACK);
        assert_eq!(same, ambiguous);
        assert!(refused.refused.is_some(), "{refused:?}");
        assert!(
            refused
                .refused_items
                .iter()
                .all(|row| row.starts_with(FOLLOWUP_LEGACY_PREFIX)),
            "{refused:?}"
        );
    }

    /// Rows in the legacy title form still parse, and still hold their item:
    /// a repeated report after the upgrade never re-adds it in the new form.
    #[test]
    fn legacy_titles_still_hold_their_items() {
        let handback = "**Summary:** done\n**Action items:**\n- operator: decide the ZIP layout\n";
        for legacy in [
            "- ### 🙋 follow up on an action item from `decisionidstamp`: operator: decide the ZIP layout [operator-verify] [#q-0123456789ab]\n",
            "- ### ~~follow up on an action item from `decisionidstamp`: operator: decide the ZIP layout~~\n",
            "- ### 🙋 [from:recovery key=q-0123456789ab-restore] follow up on an action item from `decisionidstamp`: operator: decide the ZIP layout [operator-verify] [#q-0123456789ab]\n",
        ] {
            let content = doc(&format!("- do [#decisionidstamp]\n{legacy}"));
            let (same, outcome) =
                plan_handback_followups(&content, "do [#decisionidstamp]", handback);
            assert_eq!(same, content, "{legacy}");
            assert_eq!(outcome.already_present, 1, "{legacy}");
            let row = legacy.trim_start_matches("- ### ").trim_end();
            let work = agent_doc_element_queue::strip_attempt_projection_prefix(row);
            assert!(
                crate::followup_owner::followup_row_action_item(work.trim_start_matches("~~"))
                    .is_some_and(|item| item.starts_with("operator: decide the ZIP layout")),
                "{legacy}"
            );
        }
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

#[cfg(test)]
mod ordered_step_tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue id=main -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    /// `#orderedsteps`: a numbered step's action items become follow-up rows
    /// right after that step, and the step keeps its number.
    #[test]
    fn a_numbered_step_gets_its_followups_after_it() {
        for marker in ["-", "1."] {
            let two = if marker == "-" { "-" } else { "2." };
            let content = doc(&format!(
                "- release notes\n  - steps\n    {marker} ☑️ [#subagent:d#1] draft the release notes\n    {two} review the release notes\n"
            ));
            let (updated, outcome) = plan_handback_followups(
                &content,
                "release notes: steps: draft the release notes",
                "**Summary:** drafted.\n**Action items:**\n- agent: spell-check the draft\n",
            );
            assert_eq!(outcome.inserted.len(), 1, "{marker}: {outcome:?}");
            let expected = if marker == "-" {
                "    - [#q-"
            } else {
                // Nested under the step, at its content column; the step
                // keeps its number and step 2 stays step 2.
                "    1. ☑️ [#subagent:d#1] draft the release notes [#q-"
            };
            assert!(updated.contains(expected), "{updated}");
            if marker == "1." {
                assert!(
                    updated.contains("]\n       - [#q-") && updated.contains("\n    2. review"),
                    "{updated}"
                );
            }
        }
    }
}
