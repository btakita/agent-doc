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
//! A worker's report on such a head carries the same text a second time,
//! outside the queue: `agent-doc queue report-complete` appends a
//! binary-authored completion cell to `agent:exchange` whose
//! `### Re: Subagent completed: …` heading names the worker and quotes the
//! queue item verbatim. That cell is agent-doc's own bookkeeping,
//! never an operator request, so its lines leave the scope too. Without this,
//! the 2026-10-20 `agent-doc-bugs.md` dispatch-only coordinator cycle
//! (`cycle-1792527508436`) was blocked by `pending_capture_required`: a
//! reported head's report heading quoted "… haiven-dev tasks/fpe.md …", and
//! the bare word `tasks` is a backlog signal.
//!
//! The scope is fail-closed. A line is out of scope only when it is positively
//! a queue entry this cycle does not drain, or a line of a structurally
//! binary-attested completion cell
//! ([`agent_doc_document::subagent_completion::completion_cell_line_ranges`]),
//! AND its text appears nowhere else: not as a drained live head, and not as
//! any other line outside the queue components (an operator may type the same
//! text as a live exchange prompt). A completion-shaped heading without the
//! fixed body and keyed marker is ordinary text and stays in scope. Anything
//! ambiguous stays in scope and keeps the gate armed.

use std::collections::HashSet;
use std::ops::Range;

use agent_doc_document::subagent_completion;
use agent_doc_element::element;

use crate::document_queue::{self, QueueEntry};
use crate::queue_claim::ClaimedQueueItems;

/// Normalized keys of the source lines that carry no request for this cycle:
/// the queue components' completed (struck) entries, closed by the cycle that
/// answered them, the live heads `not_drained` holds (a worker claim, a
/// subagents-attribute hold, or this cycle's subagent dispatch set), and every
/// line of a binary-attested subagent completion cell outside the queue.
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
    let completion_cells = completion_cell_byte_ranges(content);

    let mut cursor = 0usize;
    for queue in queues {
        if queue.open_end < cursor {
            // Overlapping queue components: no reliable inside/outside split,
            // so nothing in this view may leave the scope.
            in_scope.extend(line_keys(content));
            return;
        }
        classify_outside_queue(
            content,
            cursor..queue.open_end,
            &completion_cells,
            undrained,
            in_scope,
        );
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
    classify_outside_queue(
        content,
        cursor..content.len(),
        &completion_cells,
        undrained,
        in_scope,
    );
}

/// Byte ranges of `content` covered by binary-attested completion cells.
fn completion_cell_byte_ranges(content: &str) -> Vec<Range<usize>> {
    let ranges = subagent_completion::completion_cell_line_ranges(content);
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut line_starts = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        line_starts.push(offset);
        offset += line.len();
    }
    line_starts.push(offset);
    ranges
        .into_iter()
        .filter_map(|lines| Some(*line_starts.get(lines.start)?..*line_starts.get(lines.end)?))
        .collect()
}

/// Classify the lines of `content[segment]`, which lie outside every queue
/// component: a completion-cell line is binary bookkeeping, anything else is
/// in scope.
fn classify_outside_queue(
    content: &str,
    segment: Range<usize>,
    completion_cells: &[Range<usize>],
    undrained: &mut HashSet<String>,
    in_scope: &mut HashSet<String>,
) {
    let mut offset = segment.start;
    for line in content[segment].split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let key = document_queue::normalized_queue_line_for_match(line);
        if key.is_empty() {
            continue;
        }
        if completion_cells.iter().any(|cell| cell.contains(&start)) {
            undrained.insert(key);
        } else {
            in_scope.insert(key);
        }
    }
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

    /// The 2026-10-20 `agent-doc-bugs.md` shape (`cycle-1792527508436`): a
    /// reported head's binary completion cell in the exchange quoted the item
    /// verbatim, and the item mentioned `tasks/fpe.md`. The cell is agent-doc's
    /// bookkeeping, so none of its lines stays in the capture scope.
    #[test]
    fn a_completion_cell_for_a_reported_head_leaves_the_capture_scope() {
        let item =
            "a session clear queued behind a live Codex turn (haiven-dev tasks/fpe.md) never fired";
        let cell = subagent_completion::cell("subagent:queuedclearstall", item, "k1");
        let content = doc(
            &format!("\n{cell}\n"),
            &format!("- 📥 {item}\n- do [#drainedhere]\n"),
        );
        let undrained = undrained_queue_line_keys(&[&content], &claimed(&[item]));

        let mut added = cell.lines().map(str::to_string).collect::<Vec<_>>();
        added.push(format!("- 📥 {item}"));
        added.push("- do [#drainedhere]".to_string());
        assert_eq!(
            without_undrained_queue_lines(&added, &undrained),
            strings(&["- do [#drainedhere]"]),
        );
        // The diff hunk that carries the whole cell as one text keeps nothing.
        assert!(without_undrained_queue_lines(&[cell], &undrained).is_empty());
    }

    /// Fail-closed: a completion-shaped heading without the binary body and
    /// keyed marker is ordinary exchange text, and the same heading typed as
    /// its own line elsewhere keeps a real cell's heading in scope too.
    #[test]
    fn completion_shaped_prose_stays_in_the_capture_scope() {
        let item = "add tasks to the backlog";
        let heading = subagent_completion::heading("subagent:x", item);
        let content = doc(&format!("\n{heading}\n\nPlease do this.\n"), "");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        assert!(undrained.is_empty(), "{undrained:?}");

        let cell = subagent_completion::cell("subagent:x", item, "k1");
        let content = doc(&format!("\n{cell}\n\n{heading}\n"), "");
        let undrained = undrained_queue_line_keys(&[&content], &ClaimedQueueItems::none());
        assert_eq!(
            without_undrained_queue_lines(std::slice::from_ref(&heading), &undrained),
            vec![heading],
            "the typed copy of the heading is a live line: {undrained:?}"
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
