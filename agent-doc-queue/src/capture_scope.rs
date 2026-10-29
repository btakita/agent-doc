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
//! attested response (or recognized legacy body) and keyed marker is ordinary text and stays in scope. Anything
//! ambiguous stays in scope and keeps the gate armed.
//!
//! The requirement is fixed at preflight, but the set of heads a cycle drains
//! is not: the coordinator may claim a head preflight left drainable and start
//! a worker for it mid-cycle. [`CaptureScope::heads_behind_requirement`]
//! names the drainable heads the armed requirement rests on, so the closeout
//! gates can release it once every one of them belongs to a worker. It names
//! none (the requirement can never be released that way) when any in-scope
//! text that is not a drainable queue head also arms it.

use std::collections::{HashMap, HashSet};
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
    capture_scope(views, not_drained).undrained
}

/// A live queue head this cycle may drain, seen only inside queue components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainableHead {
    /// The head's queue item text, as a claim names it.
    pub text: String,
    /// Normalized keys of every source line the head spans, in any view.
    pub line_keys: HashSet<String>,
}

/// The capture scope of a cycle's views: what leaves it, and which drainable
/// heads stay in it.
#[derive(Debug, Clone, Default)]
pub struct CaptureScope {
    /// See [`undrained_queue_line_keys`].
    pub undrained: HashSet<String>,
    /// Live heads `not_drained` does not hold, whose lines appear in no view
    /// outside a queue component and as no other kind of queue line. A head
    /// whose text also appears elsewhere is never listed: that other copy may
    /// be an operator request, so claiming the head cannot release it.
    pub drainable_heads: Vec<DrainableHead>,
}

/// [`undrained_queue_line_keys`] plus the drainable heads that stay in scope.
pub fn capture_scope(views: &[&str], not_drained: &ClaimedQueueItems) -> CaptureScope {
    let mut sets = ViewSets::default();
    for content in views {
        classify_view(content, not_drained, &mut sets);
    }
    let ViewSets {
        mut undrained,
        heads,
        other,
    } = sets;
    // The same text as a drained head, or anywhere outside the queue, keeps it
    // in scope.
    undrained.retain(|key| {
        !other.contains(key) && !heads.values().any(|head| head.line_keys.contains(key))
    });
    let mut drainable_heads = heads
        .into_values()
        .filter(|head| head.line_keys.iter().all(|key| !other.contains(key)))
        .collect::<Vec<_>>();
    drainable_heads.sort_by(|a, b| a.text.cmp(&b.text));
    CaptureScope {
        undrained,
        drainable_heads,
    }
}

impl CaptureScope {
    /// Keep `key` in scope: a line the operator typed in the harness prompt is
    /// a live request whatever queue entry shares its text, so it neither
    /// leaves the scope nor lets a claim on such a head release the gate.
    pub fn keep_in_scope(&mut self, key: &str) {
        self.undrained.remove(key);
        self.drainable_heads
            .retain(|head| !head.line_keys.contains(key));
    }

    /// The drainable heads an armed backlog-capture requirement rests on, by
    /// queue item text. `requests` is the requirement predicate over
    /// `(prompt_targets, added_lines)`, already known to hold for the texts
    /// without [`Self::undrained`] lines.
    ///
    /// Empty — the requirement never releases on worker claims — when the
    /// texts still arm it with every drainable head's lines removed too: in
    /// scope text that is not a drainable queue head (an exchange prompt, a
    /// harness prompt line, a head whose text also appears outside the queue)
    /// asks for the capture itself. Otherwise the heads whose own lines arm it,
    /// when removing just those disarms it, else every drainable head present
    /// in the texts.
    pub fn heads_behind_requirement(
        &self,
        prompt_targets: &[String],
        added_lines: &[String],
        requests: impl Fn(&[String], &[String]) -> bool,
    ) -> Vec<String> {
        let arms_without = |removed: &HashSet<String>| {
            requests(
                &without_undrained_queue_lines(prompt_targets, removed),
                &without_undrained_queue_lines(added_lines, removed),
            )
        };
        let present_keys = prompt_targets
            .iter()
            .chain(added_lines)
            .flat_map(|text| line_keys(text).collect::<Vec<_>>())
            .collect::<HashSet<_>>();
        let present = self
            .drainable_heads
            .iter()
            .filter(|head| head.line_keys.iter().any(|key| present_keys.contains(key)))
            .collect::<Vec<_>>();
        let removing = |heads: &[&DrainableHead]| {
            let mut removed = self.undrained.clone();
            for head in heads {
                removed.extend(head.line_keys.iter().cloned());
            }
            removed
        };
        if present.is_empty() || arms_without(&removing(&present)) {
            return Vec::new();
        }
        let arming = present
            .iter()
            .filter(|head| {
                let others = present
                    .iter()
                    .filter(|other| other.text != head.text)
                    .copied()
                    .collect::<Vec<_>>();
                arms_without(&removing(&others))
            })
            .copied()
            .collect::<Vec<_>>();
        let behind = if !arming.is_empty() && !arms_without(&removing(&arming)) {
            arming
        } else {
            present
        };
        behind.into_iter().map(|head| head.text.clone()).collect()
    }
}

/// `heads` named as live heads after free-text promotion: a head queue
/// maintenance promoted (exact `(source_text, id)` receipts, `#freetextqueue`)
/// is now the `do [#id]` head a claim names, the form claims carry across the
/// same promotion. Order is kept; duplicates collapse.
pub fn promoted_head_texts(heads: &[String], promotions: &[(String, String)]) -> Vec<String> {
    let mut named = Vec::<String>::new();
    for head in heads {
        let identity = crate::queue_claim::claim_identity(head);
        let text = promotions
            .iter()
            .find(|(source, _)| crate::queue_claim::claim_identity(source) == identity)
            .map(|(_, id)| {
                format!(
                    "do [#{}]",
                    id.trim().trim_start_matches('#').to_ascii_lowercase()
                )
            })
            .unwrap_or_else(|| head.clone());
        if !named.contains(&text) {
            named.push(text);
        }
    }
    named
}

#[derive(Default)]
struct ViewSets {
    /// Keys of lines that carry no request for this cycle.
    undrained: HashSet<String>,
    /// Drainable live heads, by claim identity.
    heads: HashMap<agent_doc_element_queue::QueueItemIdentity, DrainableHead>,
    /// Keys of every other in-scope line: outside the queue components, a
    /// queue line that is not a head, or a view that cannot be classified.
    other: HashSet<String>,
}

fn classify_view(content: &str, not_drained: &ClaimedQueueItems, sets: &mut ViewSets) {
    let Ok(components) = element::parse(content) else {
        // No component structure: every line is outside any queue.
        sets.other.extend(line_keys(content));
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
            sets.other.extend(line_keys(content));
            return;
        }
        classify_outside_queue(content, cursor..queue.open_end, &completion_cells, sets);
        cursor = queue.close_start;
        let body = &content[queue.open_end..queue.close_start];
        let Ok(spans) = document_queue::parse_spans(body) else {
            // An unparseable queue body cannot prove any line undrained.
            sets.other.extend(line_keys(body));
            continue;
        };
        // Nest identity needs the whole body: dropping one nested `#rebase`
        // must not look like dropping every copy (`#nestlistlineage`).
        let entries: Vec<QueueEntry> = spans.iter().map(|(entry, _)| entry.clone()).collect();
        for (index, (_, span)) in spans.iter().enumerate() {
            let keys = line_keys(&body[span.clone()]);
            match &entries[index] {
                QueueEntry::Completed(_) => sets.undrained.extend(keys),
                QueueEntry::Prompt(_) if not_drained.claims_at(&entries, index) => {
                    sets.undrained.extend(keys)
                }
                // A live head this cycle may drain: in scope.
                QueueEntry::Prompt(prompt) => {
                    let identity = crate::queue_claim::claim_identity_at(&entries, index)
                        .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text));
                    sets.heads
                        .entry(identity)
                        .or_insert_with(|| DrainableHead {
                            text: prompt.text.clone(),
                            line_keys: HashSet::new(),
                        })
                        .line_keys
                        .extend(keys);
                }
                // Any line queue parsing does not classify as a head: in scope.
                _ => sets.other.extend(keys),
            }
        }
    }
    classify_outside_queue(content, cursor..content.len(), &completion_cells, sets);
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
    sets: &mut ViewSets,
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
            sets.undrained.insert(key);
        } else {
            sets.other.insert(key);
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

    /// Fail-closed: a completion-shaped heading without an attested response and
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

    fn mentions_backlog(targets: &[String], lines: &[String]) -> bool {
        targets
            .iter()
            .chain(lines)
            .any(|text| text.contains("backlog"))
    }

    /// Only the head whose own line arms the requirement is behind it; a
    /// drainable head without a signal is not.
    #[test]
    fn the_heads_behind_a_requirement_are_the_ones_that_arm_it() {
        let content = doc(
            "",
            "- put the cleanup in the backlog\n- fix the flaky test\n",
        );
        let scope = capture_scope(&[&content], &ClaimedQueueItems::none());
        assert_eq!(scope.drainable_heads.len(), 2, "{scope:?}");
        let added = strings(&["- put the cleanup in the backlog", "- fix the flaky test"]);
        assert_eq!(
            scope.heads_behind_requirement(&[], &added, mentions_backlog),
            strings(&["put the cleanup in the backlog"]),
        );
    }

    /// Fail-closed: in-scope text that is not a drainable head arms the
    /// requirement by itself, so no head is behind it.
    #[test]
    fn operator_text_outside_the_queue_leaves_no_heads_behind_the_requirement() {
        let content = doc(
            "Also note the retry cleanup in the backlog.\n",
            "- put the cleanup in the backlog\n",
        );
        let scope = capture_scope(&[&content], &ClaimedQueueItems::none());
        let added = strings(&[
            "Also note the retry cleanup in the backlog.",
            "- put the cleanup in the backlog",
        ]);
        assert!(
            scope
                .heads_behind_requirement(&[], &added, mentions_backlog)
                .is_empty()
        );
    }

    /// Fail-closed: a head whose text also appears outside the queue, or that
    /// the operator typed in the harness prompt, is not a drainable head a
    /// claim can release.
    #[test]
    fn a_head_sharing_text_with_a_live_line_is_never_behind_the_requirement() {
        let content = doc(
            "put the cleanup in the backlog\n",
            "- put the cleanup in the backlog\n",
        );
        let scope = capture_scope(&[&content], &ClaimedQueueItems::none());
        assert!(scope.drainable_heads.is_empty(), "{scope:?}");

        let content = doc("", "- put the cleanup in the backlog\n");
        let mut scope = capture_scope(&[&content], &ClaimedQueueItems::none());
        scope.keep_in_scope(&document_queue::normalized_queue_line_for_match(
            "put the cleanup in the backlog",
        ));
        assert!(
            scope
                .heads_behind_requirement(
                    &[],
                    &strings(&["- put the cleanup in the backlog"]),
                    mentions_backlog
                )
                .is_empty()
        );
    }

    #[test]
    fn promoted_heads_are_named_by_their_minted_id_head() {
        let heads = strings(&["#actionable-review", "add the cleanup to the backlog"]);
        let promotions = vec![(
            "add the cleanup to the backlog".to_string(),
            "CleanupBacklog".to_string(),
        )];
        assert_eq!(
            promoted_head_texts(&heads, &promotions),
            strings(&["#actionable-review", "do [#cleanupbacklog]"])
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
