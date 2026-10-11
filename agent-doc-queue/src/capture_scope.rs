//! Backlog-capture scope of a cycle's queue diff (`#capturegatedispatched`).
//!
//! Preflight records `requires_backlog_capture` when the cycle's prompt texts
//! (document diff lines plus the preset bodies they reference) ask for backlog
//! work. Queue lines ride in that diff even when the cycle does not drain
//! them: a coordinator cycle that dispatches `#actionable-review` to a
//! subagent, or a cycle whose queue maintenance strikes a head an earlier
//! cycle already answered, still carries the head's text, and the
//! `#actionable-review` preset body ("Add actionable review items into
//! backlog + queue") then makes the finalize pending-capture gate demand a
//! backlog mutation from a cycle that owns none of that work. The capture
//! belongs to the cycle that closes the head.
//!
//! This module computes which queue source lines a cycle does NOT drain, so
//! preflight can scope the capture requirement to the heads it does. It is
//! pure: the caller supplies the claim / hold / dispatch set it observed.
//!
//! The scope is fail-closed. A line is out of scope only when it is positively
//! a queue entry this cycle does not drain, AND its text appears nowhere else:
//! not as a drained live head, and not outside the queue components
//! (an operator may type the same text as a live exchange prompt). Anything
//! ambiguous stays in scope and keeps the gate armed.

use std::collections::HashSet;

use agent_doc_element::element;

use crate::document_queue::{self, QueueEntry};
use crate::queue_claim::ClaimedQueueItems;

/// Normalized keys of the queue components' source lines that this cycle
/// does not drain: completed (struck) entries, closed by the cycle that
/// answered them, and live heads `not_drained` holds (a worker claim, a
/// subagents-attribute hold, or this cycle's subagent dispatch set).
///
/// `views` are the document cuts the cycle's prompt texts were drawn from —
/// preflight passes the admitted diff cut and the post-maintenance queue
/// projection, because maintenance may reap a struck line the diff still
/// carries. A key survives only when no view shows it as a drained head or
/// outside a queue component.
///
/// Keys are [`document_queue::normalized_queue_line_for_match`] of each line,
/// so a diff line matches whether or not it carries the bullet, strike, pin,
/// or in-progress marker the settled queue shows. A view without a parseable
/// queue contributes nothing, which keeps its lines in scope.
pub fn undrained_queue_line_keys(
    views: &[&str],
    not_drained: &ClaimedQueueItems,
) -> HashSet<String> {
    let mut undrained = HashSet::new();
    let mut in_scope = HashSet::new();
    for content in views {
        classify_view(content, not_drained, &mut undrained, &mut in_scope);
    }
    // The same text as a drained head, or anywhere outside the queue, keeps it
    // in scope.
    undrained.retain(|key| !in_scope.contains(key));
    undrained
}

fn classify_view(
    content: &str,
    not_drained: &ClaimedQueueItems,
    undrained: &mut HashSet<String>,
    in_scope: &mut HashSet<String>,
) {
    let Ok(components) = element::parse(content) else {
        // No component structure: every line is outside any queue.
        in_scope.extend(line_keys(content));
        return;
    };
    let mut queues = components
        .iter()
        .filter(|component| component.name == "queue")
        .collect::<Vec<_>>();
    queues.sort_by_key(|queue| queue.open_end);

    let mut cursor = 0usize;
    for queue in queues {
        if queue.open_end < cursor {
            // Overlapping queue components: no reliable inside/outside split,
            // so nothing in this view may leave the scope.
            in_scope.extend(line_keys(content));
            return;
        }
        in_scope.extend(line_keys(&content[cursor..queue.open_end]));
        cursor = queue.close_start;
        let body = &content[queue.open_end..queue.close_start];
        let Ok(entries) = document_queue::parse_spans(body) else {
            // An unparseable queue body cannot prove any line undrained.
            in_scope.extend(line_keys(body));
            continue;
        };
        for (entry, span) in &entries {
            let target = match entry {
                QueueEntry::Completed(_) => &mut *undrained,
                QueueEntry::Prompt(prompt) if not_drained.claims(&prompt.text) => &mut *undrained,
                // A live head this cycle may drain, and any line queue parsing
                // does not classify as a head: in scope.
                _ => &mut *in_scope,
            };
            target.extend(line_keys(&body[span.clone()]));
        }
    }
    in_scope.extend(line_keys(&content[cursor..]));
}

/// `texts` with every line whose normalized key is in `undrained` removed;
/// texts left blank are dropped. Lines that do not match stay verbatim, so a
/// multi-line prompt keeps its in-scope content.
pub fn without_undrained_queue_lines(texts: &[String], undrained: &HashSet<String>) -> Vec<String> {
    if undrained.is_empty() {
        return texts.to_vec();
    }
    texts
        .iter()
        .filter_map(|text| {
            let kept = text
                .lines()
                .filter(|line| {
                    let key = document_queue::normalized_queue_line_for_match(line);
                    key.is_empty() || !undrained.contains(&key)
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!kept.trim().is_empty()).then_some(kept)
        })
        .collect()
}

fn line_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .map(document_queue::normalized_queue_line_for_match)
        .filter(|key| !key.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::claim_identity;

    fn doc(exchange: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n\n## Exchange\n\n\
             <!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n\
             ## Queue\n\n<!-- agent:queue subagents -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    fn claimed(items: &[&str]) -> ClaimedQueueItems {
        ClaimedQueueItems::from_identities(items.iter().map(|item| claim_identity(item)))
    }

    fn strings(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| text.to_string()).collect()
    }

    /// The 2026-10-10 agent-doc-bugs.md shape: the cycle's diff carried a
    /// struck `#actionable-review` head (answered by the previous cycle) and two
    /// heads this cycle dispatches to subagents. None of them is drained here,
    /// so none of their lines stays in the capture scope.
    #[test]
    fn struck_claimed_and_dispatched_heads_leave_the_capture_scope() {
        let content = doc(
            "",
            "- 🚧 do [#hookstalemsgfile]\n- do [#relayhubtestisolation]\n\
             - do [#drainedhere]\n- ~~#actionable-review~~\n",
        );
        let not_drained = claimed(&["do [#hookstalemsgfile]", "do [#relayhubtestisolation]"]);
        let undrained = undrained_queue_line_keys(&[&content], &not_drained);

        let added = strings(&[
            "- 🚧 do [#hookstalemsgfile]",
            "- do [#relayhubtestisolation]",
            "- ~~#actionable-review~~",
            "- do [#drainedhere]",
        ]);
        assert_eq!(
            without_undrained_queue_lines(&added, &undrained),
            strings(&["- do [#drainedhere]"]),
        );
        // The pre-maintenance spelling of the struck head matches too.
        assert!(
            without_undrained_queue_lines(&strings(&["- #actionable-review"]), &undrained)
                .is_empty()
        );
    }

    /// Fail-closed: a live head the cycle may drain keeps its line, even when
    /// another (struck or claimed) entry shares its text.
    #[test]
    fn a_drained_head_sharing_the_text_keeps_it_in_scope() {
        let content = doc("", "- ~~#actionable-review~~\n- #actionable-review\n");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        assert!(undrained.is_empty(), "{undrained:?}");

        let content = doc("", "- #actionable-review\n");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        assert!(
            undrained.is_empty(),
            "an unclaimed live head is drained: {undrained:?}"
        );
    }

    /// Fail-closed: the same text typed outside the queue is a live request.
    #[test]
    fn the_same_text_outside_the_queue_keeps_it_in_scope() {
        let content = doc("#actionable-review\n", "- ~~#actionable-review~~\n");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        assert!(undrained.is_empty(), "{undrained:?}");
    }

    /// Queue maintenance may reap the struck line from the settled projection
    /// while the admitted diff cut still carries it.
    #[test]
    fn a_struck_line_reaped_from_one_view_stays_out_of_scope() {
        let admitted = doc("", "- ~~#actionable-review~~\n");
        let settled = doc("", "");
        let undrained =
            undrained_queue_line_keys(&[&admitted, &settled], &ClaimedQueueItems::none());
        assert!(
            without_undrained_queue_lines(&strings(&["- ~~#actionable-review~~"]), &undrained)
                .is_empty()
        );
        // A view that shows the text as a drainable head keeps it in scope.
        let revived = doc("", "- #actionable-review\n");
        let undrained =
            undrained_queue_line_keys(&[&admitted, &revived], &ClaimedQueueItems::none());
        assert!(undrained.is_empty(), "{undrained:?}");
    }

    #[test]
    fn multi_line_texts_keep_their_in_scope_lines() {
        let content = doc("", "- ~~#actionable-review~~\n");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        let texts = strings(&["what's next?\n- ~~#actionable-review~~"]);
        assert_eq!(
            without_undrained_queue_lines(&texts, &undrained),
            strings(&["what's next?"])
        );
    }

    #[test]
    fn no_queue_means_nothing_leaves_the_scope() {
        let content = "---\nagent_doc_format: template\n---\n\n#actionable-review\n";
        assert!(
            undrained_queue_line_keys(&[content], &claimed(&["#actionable-review"])).is_empty()
        );
    }
}
