//! Strike a live hourglass row once the operator action it tracks is complete
//! or moot (`#hourglasslifetime`).
//!
//! `⏳` is a projection. The row stays dispatchable until something records
//! that the operator action ended. `#todosync` only strikes a row whose done
//! id is the checked todo, and a free-text follow-up's first `#` token is
//! often a different id. This pass reads the reserved request annotation, the
//! row's own stable id when a todo owns it, a checked todo source locator,
//! exact follow-up action text, and an already-struck copy of the same
//! follow-up.
//!
//! A plain `[operator-verify]` row with none of that evidence stays. Two
//! still-open copies stay. An open todo or an open backlog item keeps the
//! row. A live claim
//! (`🟡`/`🚧`/`☑️`/`⚠️`/`⛔`) stays. The second call is a no-op.
//!
//! [`crate::queue_sync_payload::HOURGLASS_RESOLVE_GUIDANCE`] still tells the
//! admitted session to resolve leftover rows. This module is the lifetime
//! strike, not a replacement for that guidance.

use std::collections::HashSet;

use anyhow::Result;

use agent_doc_document::queue_projection::{
    QueueLifecycleMarker, has_awaiting_operator_marker, is_reserved_operator_action_id,
    lifecycle_marker, operator_answer_row_request_id, operator_request_annotation_ids,
};
use agent_doc_document::tracked_work_projection::open_backlog_ids;
use agent_doc_element::todo::{CheckedTodo, checked_todos, open_todo_actions, open_todo_ids};
use agent_doc_element_queue::{queue_head_identity, queue_prompt_stable_id, struck_work_text};

use crate::document_queue::{self, QueueEntry};
use crate::followup_owner;
use crate::queue_continuation::head_awaits_operator_inline;

/// Document after a lifetime strike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourglassLifetimeStrike {
    /// Successor document.
    pub content: String,
    /// Live rows converted to struck rows, including request-packet children.
    pub struck: usize,
}

/// Strike completed and moot hourglass rows in every queue block.
///
/// Returns `None` when the document is unchanged, so a second call is a no-op.
pub fn strike_completed_hourglass_rows(content: &str) -> Result<Option<HourglassLifetimeStrike>> {
    let components = agent_doc_element::element::parse(content)?;
    let queues = crate::queue_set::QueueSet::parse(content, &components)?;
    let evidence = Evidence::from_content(content);
    let mut struck_keys = HashSet::new();
    for block in queues.blocks() {
        collect_completed_keys(&block.entries, &mut struck_keys);
    }
    let mut plans: Vec<Vec<bool>> = queues
        .blocks()
        .iter()
        .map(|block| vec![false; block.entries.len()])
        .collect();
    for (block, marks) in queues.blocks().iter().zip(plans.iter_mut()) {
        mark_matches(&block.entries, &evidence, &struck_keys, false, marks);
    }
    for (block, marks) in queues.blocks().iter().zip(plans.iter()) {
        collect_marked_keys(&block.entries, marks, &mut struck_keys);
    }
    for (block, marks) in queues.blocks().iter().zip(plans.iter_mut()) {
        mark_matches(&block.entries, &evidence, &struck_keys, true, marks);
    }
    for (block, marks) in queues.blocks().iter().zip(plans.iter_mut()) {
        expand_packet_children(&block.entries, marks);
    }

    let mut replacements = Vec::new();
    let mut struck = 0usize;
    for (block, marks) in queues.blocks().iter().zip(plans.iter()) {
        let converted = marks.iter().filter(|mark| **mark).count();
        if converted == 0 {
            continue;
        }
        let entries = apply_marks(&block.entries, marks);
        let body = document_queue::render(&entries);
        if body == block.component.content(content) {
            continue;
        }
        struck += converted;
        replacements.push((block.component.open_end, block.component.close_start, body));
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut projected = content.to_string();
    for (start, end, body) in replacements {
        projected.replace_range(start..end, &body);
    }
    Ok(Some(HourglassLifetimeStrike {
        content: projected,
        struck,
    }))
}

struct Evidence {
    open: HashSet<String>,
    checked: HashSet<String>,
    open_actions: HashSet<String>,
    checked_actions: HashSet<String>,
    open_sources: HashSet<String>,
    checked_sources: HashSet<String>,
}

impl Evidence {
    fn from_content(content: &str) -> Self {
        let mut open = open_todo_ids(content);
        // A `do [#subagent-operator-request-…]` head often names a backlog
        // item, not a todo. That id is still live work.
        for id in open_backlog_ids(content) {
            open.insert(id.to_ascii_lowercase());
        }
        let checked_items = checked_todos(content);
        let checked = checked_items
            .iter()
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        let mut open_actions = HashSet::new();
        let mut open_sources = HashSet::new();
        for action in open_todo_actions(content) {
            let norm = normalize_action(&action);
            if !norm.is_empty() {
                open_actions.insert(norm);
            }
            open_sources.extend(queue_source_ids(&action));
        }
        let mut checked_actions = HashSet::new();
        let mut checked_sources = HashSet::new();
        for CheckedTodo { action, .. } in &checked_items {
            let norm = normalize_action(action);
            if !norm.is_empty() {
                checked_actions.insert(norm);
            }
            checked_sources.extend(queue_source_ids(action));
        }
        Self {
            open,
            checked,
            open_actions,
            checked_actions,
            open_sources,
            checked_sources,
        }
    }
}

fn mark_matches(
    entries: &[QueueEntry],
    evidence: &Evidence,
    struck_keys: &HashSet<String>,
    siblings_only: bool,
    marks: &mut [bool],
) {
    for (index, entry) in entries.iter().enumerate() {
        if marks[index] {
            continue;
        }
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if blocked_by_live_descendant(entries, index) {
            continue;
        }
        let strike = if siblings_only {
            is_hourglass_row(&prompt.text)
                && followup_lifetime_key(&prompt.text).is_some_and(|key| struck_keys.contains(&key))
        } else {
            completion_or_moot(&prompt.text, evidence)
        };
        if strike {
            marks[index] = true;
        }
    }
}

fn completion_or_moot(text: &str, evidence: &Evidence) -> bool {
    if !is_hourglass_row(text) {
        return false;
    }
    let tracked = tracked_ids(text, evidence);
    if tracked.iter().any(|id| evidence.open.contains(id)) {
        return false;
    }
    if let Some(stable) = stable_id(text)
        && (evidence.open.contains(&stable) || evidence.open_sources.contains(&stable))
    {
        return false;
    }
    if tracked.iter().any(|id| evidence.checked.contains(id)) {
        return true;
    }
    if let Some(stable) = stable_id(text)
        && evidence.checked_sources.contains(&stable)
    {
        return true;
    }
    if let Some(action) = followup_action(text) {
        let norm = normalize_action(&action);
        if !norm.is_empty()
            && evidence.checked_actions.contains(&norm)
            && !evidence.open_actions.contains(&norm)
        {
            return true;
        }
    }
    let reserved = operator_request_annotation_ids(text);
    !reserved.is_empty()
        && reserved
            .iter()
            .all(|id| !evidence.open.contains(id) && !evidence.checked.contains(id))
        && !head_awaits_operator_inline(text)
}

fn is_hourglass_row(text: &str) -> bool {
    if operator_answer_row_request_id(text).is_some() || row_in_flight(text) {
        return false;
    }
    has_awaiting_operator_marker(text)
        || head_awaits_operator_inline(text)
        || !operator_request_annotation_ids(text).is_empty()
}

fn row_in_flight(text: &str) -> bool {
    matches!(
        lifecycle_marker(text),
        Some(
            QueueLifecycleMarker::Claimed
                | QueueLifecycleMarker::Running
                | QueueLifecycleMarker::Reported
                | QueueLifecycleMarker::Lost
                | QueueLifecycleMarker::LostClaimed
                | QueueLifecycleMarker::LostRunning
                | QueueLifecycleMarker::LostReported
                | QueueLifecycleMarker::Blocked
                | QueueLifecycleMarker::Reconciling
                | QueueLifecycleMarker::Unknown
        )
    )
}

fn tracked_ids(text: &str, evidence: &Evidence) -> Vec<String> {
    let mut ids = operator_request_annotation_ids(text);
    if let Some(stable) = stable_id(text)
        && (evidence.open.contains(&stable) || evidence.checked.contains(&stable))
        && !ids.iter().any(|id| id == &stable)
    {
        ids.push(stable);
    }
    ids
}

fn stable_id(text: &str) -> Option<String> {
    queue_prompt_stable_id(text).map(|id| id.to_ascii_lowercase())
}

fn followup_action(text: &str) -> Option<String> {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    let work = work.trim_matches('~').trim();
    followup_owner::followup_row_action_item(work).map(str::to_string)
}

fn normalize_action(raw: &str) -> String {
    let text = strip_source_locators(raw);
    let text = strip_operator_verify(&text);
    let text = strip_correlation_ids(&text);
    let text = strip_owner_label(text.trim());
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn strip_owner_label(item: &str) -> &str {
    let emphasis = |c: char| c == '*' || c == '_';
    let rest = item.trim_start().trim_start_matches(emphasis);
    let word_end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    if !matches!(
        rest[..word_end].to_ascii_lowercase().as_str(),
        "agent" | "operator" | "human"
    ) {
        return item.trim();
    }
    let mut tail = rest[word_end..].trim_start_matches(emphasis).trim_start();
    if let Some(inner) = tail.strip_prefix('(') {
        let Some(close) = inner.find(')') else {
            return item.trim();
        };
        tail = inner[close + 1..].trim_start_matches(emphasis).trim_start();
    }
    tail.strip_prefix(':')
        .map(str::trim_start)
        .unwrap_or(item.trim())
}

fn strip_source_locators(action: &str) -> String {
    let mut out = String::new();
    let mut rest = action;
    while let Some(start) = rest.find("(source:") {
        out.push_str(rest[..start].trim_end());
        let after = &rest[start..];
        let Some(end) = after.find(')') else {
            out.push_str(after);
            return out;
        };
        rest = after[end + 1..].trim_start();
        if !out.is_empty() && !rest.is_empty() {
            out.push(' ');
        }
    }
    out.push_str(rest);
    out
}

fn strip_operator_verify(text: &str) -> String {
    let tag = followup_owner::OPERATOR_OWNED_TAG;
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = lower[search..].find(tag) {
        let start = search + found;
        out.push_str(&text[copied..start]);
        copied = start + tag.len();
        search = copied;
    }
    out.push_str(&text[copied..]);
    out
}

fn strip_correlation_ids(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else {
            out.push_str(rest);
            return out;
        };
        let id = &after[..end];
        let drop = id.starts_with("q-") || is_reserved_operator_action_id(id);
        if drop {
            out.push_str(&rest[..start]);
            rest = &after[end + 1..];
        } else {
            let keep_end = start + 2 + end + 1;
            out.push_str(&rest[..keep_end]);
            rest = &rest[keep_end..];
        }
    }
    out.push_str(rest);
    out
}

/// Ids named by `(source: agent:queue [#id])` locators.
fn queue_source_ids(action: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = action;
    while let Some(start) = rest.find("(source:") {
        let after = &rest[start + "(source:".len()..];
        let Some(close) = after.find(')') else {
            break;
        };
        let locator = &after[..close];
        if locator.trim_start().starts_with("agent:queue")
            && let Some(id_start) = locator.find("[#")
        {
            let id_rest = &locator[id_start + 2..];
            if let Some(id_end) = id_rest.find(']') {
                let id = id_rest[..id_end].trim().to_ascii_lowercase();
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        rest = &after[close + 1..];
    }
    ids
}

fn followup_lifetime_key(text: &str) -> Option<String> {
    let bare = agent_doc_element_queue::strip_priority_markers(text);
    let bare = strip_operator_verify(&bare);
    let bare = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(&bare);
    let bare = strip_correlation_ids(&bare);
    let bare = bare.split_whitespace().collect::<Vec<_>>().join(" ");
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(&bare);
    followup_owner::followup_row_action_item(work)?;
    let key = bare.to_ascii_lowercase();
    (!key.is_empty()).then_some(key)
}

fn collect_completed_keys(entries: &[QueueEntry], keys: &mut HashSet<String>) {
    for entry in entries {
        if let QueueEntry::Completed(prompt) = entry
            && let Some(key) = followup_lifetime_key(&prompt.text)
        {
            keys.insert(key);
        }
    }
}

fn collect_marked_keys(entries: &[QueueEntry], marks: &[bool], keys: &mut HashSet<String>) {
    for (entry, mark) in entries.iter().zip(marks.iter()) {
        if *mark
            && let QueueEntry::Prompt(prompt) = entry
            && let Some(key) = followup_lifetime_key(&prompt.text)
        {
            keys.insert(key);
        }
    }
}

fn blocked_by_live_descendant(entries: &[QueueEntry], index: usize) -> bool {
    let QueueEntry::Prompt(parent) = &entries[index] else {
        return false;
    };
    for later in entries.iter().skip(index + 1) {
        if entry_depth(later) <= parent.indent {
            break;
        }
        if let QueueEntry::Prompt(prompt) = later
            && row_in_flight(&prompt.text)
        {
            return true;
        }
    }
    false
}

fn entry_depth(entry: &QueueEntry) -> usize {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => prompt.indent,
        QueueEntry::Freeform(line) => {
            let first = line.lines().next().unwrap_or(line);
            first.len() - first.trim_start().len()
        }
        _ => 0,
    }
}

fn expand_packet_children(entries: &[QueueEntry], marks: &mut [bool]) {
    let independent = marks.to_vec();
    for index in 0..entries.len() {
        if !independent[index] {
            continue;
        }
        let QueueEntry::Prompt(parent) = &entries[index] else {
            continue;
        };
        if operator_request_annotation_ids(&parent.text).is_empty() {
            continue;
        }
        for later_index in index + 1..entries.len() {
            if entry_depth(&entries[later_index]) <= parent.indent {
                break;
            }
            let QueueEntry::Prompt(prompt) = &entries[later_index] else {
                continue;
            };
            if strike_packet_descendant(&prompt.text, independent[later_index]) {
                marks[later_index] = true;
            }
        }
    }
}

fn strike_packet_descendant(text: &str, independent: bool) -> bool {
    if row_in_flight(text) {
        return false;
    }
    if independent {
        return true;
    }
    if is_hourglass_row(text) {
        return false;
    }
    !queue_head_identity(text).is_id_backed()
}

fn apply_marks(entries: &[QueueEntry], marks: &[bool]) -> Vec<QueueEntry> {
    entries
        .iter()
        .zip(marks.iter())
        .map(|(entry, mark)| {
            if *mark && let QueueEntry::Prompt(prompt) = entry {
                let mut completed = prompt.clone();
                completed.text = struck_work_text(&completed.text).to_string();
                return QueueEntry::Completed(completed);
            }
            entry.clone()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(queue: &str, todo: &str) -> String {
        format!(
            "<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n<!-- agent:todo -->\n{todo}<!-- /agent:todo -->\n"
        )
    }

    fn states(content: &str) -> Vec<(bool, String)> {
        let components = agent_doc_element::element::parse(content).unwrap();
        let queues = crate::queue_set::QueueSet::parse(content, &components).unwrap();
        let mut out = Vec::new();
        for block in queues.blocks() {
            for entry in &block.entries {
                match entry {
                    QueueEntry::Prompt(prompt) => out.push((true, prompt.text.clone())),
                    QueueEntry::Completed(prompt) => out.push((false, prompt.text.clone())),
                    _ => {}
                }
            }
        }
        out
    }

    fn assert_live(content: &str, needle: &str) {
        let found = states(content);
        assert!(
            found
                .iter()
                .any(|(live, text)| *live && text.contains(needle)),
            "expected live {needle} in {found:?}"
        );
    }

    fn assert_struck(content: &str, needle: &str) {
        let found = states(content);
        assert!(
            found
                .iter()
                .any(|(live, text)| !*live && text.contains(needle)),
            "expected struck {needle} in {found:?}"
        );
    }

    #[test]
    fn checked_request_strikes_followup_whose_first_hash_is_not_the_request() {
        let doc = document(
            "- ### ⏳ follow up on an action item from `rebase #rebase onto main`: operator: land the rebase [#subagent-operator-request-x]\n- ### 📥 do [#keep]\n",
            "- [x] [#subagent-operator-request-x] land the rebase\n",
        );
        let struck = strike_completed_hourglass_rows(&doc)
            .unwrap()
            .expect("checked request strikes the hourglass row");
        assert_struck(&struck.content, "subagent-operator-request-x");
        assert_live(&struck.content, "do [#keep]");
        assert!(
            strike_completed_hourglass_rows(&struck.content)
                .unwrap()
                .is_none(),
            "second pass is a no-op"
        );
    }

    #[test]
    fn open_request_todo_keeps_the_hourglass_row() {
        let doc = document(
            "- ### 🙋⏳ follow up on an action item from `origin`: review the pull request [#subagent-operator-request-cli-pr18-review]\n",
            "- [ ] [#subagent-operator-request-cli-pr18-review] review the pull request\n",
        );
        assert!(strike_completed_hourglass_rows(&doc).unwrap().is_none());
    }

    #[test]
    fn plain_operator_verify_without_evidence_stays() {
        let doc = document(
            "- ### ⏳ follow up on an action item from `origin`: operator: approve haiven-sdk pull request 244 [operator-verify] [#q-abc]\n",
            "",
        );
        assert!(strike_completed_hourglass_rows(&doc).unwrap().is_none());
    }

    #[test]
    fn struck_followup_sibling_strikes_and_two_live_copies_stay() {
        let row = "follow up on an action item from `origin`: operator: approve the release";
        let duplicated = document(
            &format!(
                "- ### ⏳ {row} [operator-verify] [#q-a]\n- ### ⏳ {row} [operator-verify] [#q-b]\n"
            ),
            "",
        );
        assert!(
            strike_completed_hourglass_rows(&duplicated)
                .unwrap()
                .is_none(),
            "two live copies are not a completed lifetime"
        );
        let leftover = document(
            &format!(
                "- ### ~~{row} [operator-verify] [#q-done]~~\n- ### ⏳ {row} [operator-verify] [#q-live]\n"
            ),
            "",
        );
        let struck = strike_completed_hourglass_rows(&leftover)
            .unwrap()
            .expect("a live copy of a struck follow-up is leftover");
        assert_eq!(
            states(&struck.content)
                .iter()
                .filter(|(live, _)| *live)
                .count(),
            0
        );
    }

    #[test]
    fn removed_request_todo_strikes_only_a_non_verify_row() {
        let moot = document(
            "- ### 🙋⏳ follow up on an action item from `origin`: land the rebase [#subagent-operator-request-gone]\n",
            "",
        );
        let struck = strike_completed_hourglass_rows(&moot)
            .unwrap()
            .expect("a reserved request with no todo is moot");
        assert_struck(&struck.content, "subagent-operator-request-gone");

        let waiting = document(
            "- ### 🙋⏳ follow up on an action item from `origin`: operator: land the rebase [operator-verify] [#subagent-operator-request-gone]\n",
            "",
        );
        assert!(
            strike_completed_hourglass_rows(&waiting).unwrap().is_none(),
            "an operator-verify wait is not moot merely because its request todo is gone"
        );
    }

    #[test]
    fn checked_action_text_strikes_operator_verify_and_an_open_match_does_not() {
        let row = "- ### ⏳ follow up on an action item from `origin`: operator: approve the release [operator-verify] [#q-1]\n";
        let checked = document(row, "- [x] [#release-ok] approve the release\n");
        let struck = strike_completed_hourglass_rows(&checked)
            .unwrap()
            .expect("exact checked action text completes the wait");
        assert_struck(&struck.content, "approve the release");

        let open = document(row, "- [ ] [#release-ok] approve the release\n");
        assert!(strike_completed_hourglass_rows(&open).unwrap().is_none());
    }

    #[test]
    fn checked_source_locator_strikes_when_the_action_text_differs() {
        let doc = document(
            "- ### ⏳ follow up on an action item from `origin`: operator: approve the release [operator-verify] [#q-abc]\n",
            "- [x] [#unrelated] something else entirely (source: agent:queue [#q-abc])\n",
        );
        let struck = strike_completed_hourglass_rows(&doc)
            .unwrap()
            .expect("a checked source locator names this row");
        assert_struck(&struck.content, "q-abc");

        let open = document(
            "- ### ⏳ follow up on an action item from `origin`: operator: approve the release [operator-verify] [#q-abc]\n",
            "- [ ] [#unrelated] something else entirely (source: agent:queue [#q-abc])\n",
        );
        assert!(strike_completed_hourglass_rows(&open).unwrap().is_none());
    }

    #[test]
    fn non_hourglass_row_and_live_claim_stay() {
        let doc = document(
            "- ### 📥 do [#other]\n- ### 🚧 decide the request [#subagent-operator-request-x]\n- ### ⛔ hold the request [#subagent-operator-request-y]\n",
            "- [x] [#subagent-operator-request-x] decide the request\n- [x] [#subagent-operator-request-y] hold the request\n- [x] [#other] do the other thing\n",
        );
        assert!(strike_completed_hourglass_rows(&doc).unwrap().is_none());
    }

    #[test]
    fn second_queue_block_and_packet_children_are_struck() {
        let doc = "<!-- agent:queue id=main -->\n- ### 📥 do [#keep]\n<!-- /agent:queue -->\n<!-- agent:queue id=side -->\n- ### 🙋⏳ decide the release [#subagent-operator-request-x]\n  - packet detail\n<!-- /agent:queue -->\n<!-- agent:todo -->\n- [x] [#subagent-operator-request-x] decide the release\n<!-- /agent:todo -->\n";
        let struck = strike_completed_hourglass_rows(doc)
            .unwrap()
            .expect("the side queue is in scope");
        assert_live(&struck.content, "do [#keep]");
        assert_struck(&struck.content, "subagent-operator-request-x");
        assert_struck(&struck.content, "packet detail");
        assert!(
            strike_completed_hourglass_rows(&struck.content)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn inflight_child_keeps_the_parent() {
        let doc = document(
            "- ### 🙋⏳ decide the release [#subagent-operator-request-x]\n  - 🚧 still running\n",
            "- [x] [#subagent-operator-request-x] decide the release\n",
        );
        assert!(strike_completed_hourglass_rows(&doc).unwrap().is_none());
    }
}
