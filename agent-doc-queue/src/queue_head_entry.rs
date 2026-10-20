//! `#qheadresiduescope`: where a live free-text queue head last (re-)entered
//! the queue, as pure content policy.
//!
//! The committed exchange keeps every earlier answer. An operator who types an
//! already-answered report again (a repeated bug report), or un-strikes an
//! answered head, has asked for it again, so an echo written before that
//! re-entry does not answer it. Every site that judges a live free-text head
//! against the whole committed exchange — the preflight catch-up residue strike
//! and the `#syntheticstrike` continuation-head skip — scopes the exchange to
//! the text written after the head's entry point.
//!
//! The entry point comes from the document's git history, the one record of
//! when a line appeared that survives closed editor epochs, restarts and
//! snapshot rewrites. This module owns the walk's decisions;
//! `agent_doc_git_io::revision::free_text_head_entry_anchor_exchange` owns the
//! git reads and feeds each committed version into [`FreeTextHeadEntryWalk`].

use agent_doc_document::queue_projection::strip_priority_markers;

use crate::document_queue::{self, QueueEntry};

/// How many committed versions of a document the walk reads before it stops
/// looking for a free-text head's queue entry.
pub const FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT: usize = 128;

/// The identity the entry walk matches a head by: priority markers stripped,
/// whitespace collapsed, lowercase. The same identity the preflight catch-up's
/// in-flight-edit gate uses.
pub fn free_text_head_entry_identity(text: &str) -> String {
    strip_priority_markers(text)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `agent:exchange` component's text in `content` (empty when absent).
pub fn exchange_component_text(content: &str) -> String {
    agent_doc_element::element::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_default()
}

/// The entries of queue `queue_id` in `content`, falling back to the first
/// `agent:queue` component when that id is absent (a single-queue document, or
/// a version from before the queue gained an id).
fn queue_entries_for_entry_walk(content: &str, queue_id: &str) -> Vec<QueueEntry> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let by_id = crate::queue_set::component_for_id(content, &components, queue_id)
        .ok()
        .flatten()
        .and_then(|queue| document_queue::parse(queue.content(content)).ok())
        .unwrap_or_default();
    if !by_id.is_empty() {
        return by_id;
    }
    components
        .iter()
        .find(|component| component.name == "queue")
        .and_then(|queue| document_queue::parse(queue.content(content)).ok())
        .unwrap_or_default()
}

/// Whether a live `Prompt` row of queue `queue_id` in `content` has the entry
/// identity `head_identity` ([`free_text_head_entry_identity`]).
pub fn free_text_head_live_in_queue(content: &str, queue_id: &str, head_identity: &str) -> bool {
    queue_entries_for_entry_walk(content, queue_id)
        .iter()
        .any(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                free_text_head_entry_identity(&prompt.text) == head_identity
            }
            _ => false,
        })
}

/// The decisions of the newest-first walk over a document's committed
/// versions that finds where a free-text head last entered the queue.
///
/// Feed each committed version to [`Self::visit`] (newest first, `None` for a
/// commit where the path is absent) until it returns `false`, then read the
/// anchor with [`Self::finish`]. The walk continues while the head is live and
/// stops at the first version in which it is not (struck, deleted, or not yet
/// typed); that version's exchange is the anchor, and only exchange text
/// written after it can answer the head.
#[derive(Debug, Clone)]
pub struct FreeTextHeadEntryWalk {
    queue_id: String,
    head_identity: String,
    anchor: Option<String>,
    oldest_live: Option<String>,
    visited: usize,
}

impl FreeTextHeadEntryWalk {
    pub fn new(queue_id: &str, head_text: &str) -> Self {
        Self {
            queue_id: queue_id.to_string(),
            head_identity: free_text_head_entry_identity(head_text),
            anchor: None,
            oldest_live: None,
            visited: 0,
        }
    }

    /// Visit the next-older committed version; `false` stops the walk.
    pub fn visit(&mut self, content: Option<&str>) -> bool {
        self.visited += 1;
        match content {
            Some(content)
                if free_text_head_live_in_queue(content, &self.queue_id, &self.head_identity) =>
            {
                self.oldest_live = Some(exchange_component_text(content));
                true
            }
            Some(content) => {
                self.anchor = Some(exchange_component_text(content));
                false
            }
            None => {
                self.anchor = Some(String::new());
                false
            }
        }
    }

    /// The exchange as it stood just before the head last (re-)entered the
    /// queue, or `None` when every echo counts.
    ///
    /// `None` (whole exchange, the behavior before `#qheadresiduescope`) when
    /// the document has no commit history or the head was live in every commit
    /// back to the document's first: every echo then postdates the head. When
    /// the walk reached `limit` with the head still live, the oldest version
    /// read is the anchor. That can only hide an echo written more than the
    /// limit's worth of commits ago, which an earlier pass would already have
    /// acted on.
    pub fn finish(self, limit: usize) -> Option<String> {
        if self.anchor.is_some() {
            return self.anchor;
        }
        if self.visited >= limit {
            return self.oldest_live;
        }
        None
    }
}

/// `exchange`, scoped to the text written after `anchor` when there is one
/// ([`crate::queue_response::exchange_text_newer_than`]).
pub fn exchange_after_head_entry(exchange: &str, anchor: Option<&str>) -> String {
    match anchor {
        Some(anchor) => crate::queue_response::exchange_text_newer_than(exchange, anchor),
        None => exchange.to_string(),
    }
}

/// Why a [`LoopPassEchoWalk`] anchored where it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopPassEchoAnchorKind {
    /// The newest committed version that already held an echo written about
    /// another queue's copy of the same head: that write retired the head in
    /// the other queue and left this queue's copy live.
    OtherQueueEcho,
    /// The newest version committed at or before the current pass opened.
    PassOpened,
    /// The head (re-)entered this queue after that version
    /// ([`FreeTextHeadEntryWalk`]'s anchor).
    HeadEntry,
    /// The walk reached its history limit with the head still live.
    HistoryLimit,
}

impl LoopPassEchoAnchorKind {
    /// Stable ops-log label.
    pub fn label(self) -> &'static str {
        match self {
            Self::OtherQueueEcho => "other_queue_echo",
            Self::PassOpened => "pass_opened",
            Self::HeadEntry => "head_entry",
            Self::HistoryLimit => "history_limit",
        }
    }
}

/// The exchange text an answering echo must be newer than
/// ([`crate::queue_response::exchange_text_newer_than`]), and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopPassEchoAnchor {
    pub exchange: String,
    pub kind: LoopPassEchoAnchorKind,
}

/// GH #327 pass 4: which echoes may answer a live head
/// of a `loop` queue whose pass is open.
///
/// A `loop` head is a standing program: every pass restores it, so the
/// committed exchange always holds the echoes of earlier passes, and a sibling
/// queue may hold the same text. Only an echo written for THIS pass's run of
/// THIS queue's head answers it. The walk keeps the
/// [`FreeTextHeadEntryWalk`] entry scope and narrows it twice:
///
/// - **Pass opening.** An echo committed at or before the pass opened belongs
///   to an earlier run (an earlier pass, or another queue's cycle); the newest
///   version committed at or before `opened_secs` is an anchor.
/// - **Another queue's head.** A write that added an echo of the head while it
///   struck that head in another queue, or consumed it from another queue,
///   answered that queue's copy. The written version is an anchor, so the echo
///   and everything before it stay history for this queue.
///
/// Feed each committed version newest first with its commit time to
/// [`Self::visit`] (`None` content for a commit where the path is absent)
/// until it returns `false`, then read the anchor with [`Self::finish`]. The
/// walk starts from `current_content` so an uncommitted echo is attributed
/// too. The newest anchor found wins, because each check runs at each step in
/// newest-first order.
#[derive(Debug, Clone)]
pub struct LoopPassEchoWalk {
    queue_id: String,
    head_text: String,
    head_identity: String,
    opened_secs: u64,
    newer: String,
    anchor: Option<LoopPassEchoAnchor>,
    oldest: Option<String>,
    visited: usize,
}

impl LoopPassEchoWalk {
    pub fn new(queue_id: &str, head_text: &str, opened_secs: u64, current_content: &str) -> Self {
        Self {
            queue_id: queue_id.to_string(),
            head_text: head_text.to_string(),
            head_identity: free_text_head_entry_identity(head_text),
            opened_secs,
            newer: current_content.to_string(),
            anchor: None,
            oldest: None,
            visited: 0,
        }
    }

    /// Visit the next-older committed version, committed at `committed_secs`;
    /// `false` stops the walk.
    pub fn visit(&mut self, committed_secs: u64, content: Option<&str>) -> bool {
        self.visited += 1;
        let Some(content) = content else {
            self.anchor = Some(LoopPassEchoAnchor {
                exchange: String::new(),
                kind: LoopPassEchoAnchorKind::HeadEntry,
            });
            return false;
        };
        if self.newer_echo_answers_another_queue(content) {
            self.anchor = Some(LoopPassEchoAnchor {
                exchange: exchange_component_text(&self.newer),
                kind: LoopPassEchoAnchorKind::OtherQueueEcho,
            });
            return false;
        }
        if committed_secs <= self.opened_secs {
            self.anchor = Some(LoopPassEchoAnchor {
                exchange: exchange_component_text(content),
                kind: LoopPassEchoAnchorKind::PassOpened,
            });
            return false;
        }
        if !free_text_head_live_in_queue(content, &self.queue_id, &self.head_identity) {
            self.anchor = Some(LoopPassEchoAnchor {
                exchange: exchange_component_text(content),
                kind: LoopPassEchoAnchorKind::HeadEntry,
            });
            return false;
        }
        self.oldest = Some(exchange_component_text(content));
        self.newer = content.to_string();
        true
    }

    /// Whether the step from `older` to the version visited before it added an
    /// echo of the head while retiring the same head in another queue.
    fn newer_echo_answers_another_queue(&self, older: &str) -> bool {
        let added = crate::queue_response::exchange_text_newer_than(
            &exchange_component_text(&self.newer),
            &exchange_component_text(older),
        );
        if !crate::queue_response::free_text_head_answered_by_response(&added, &self.head_text) {
            return false;
        }
        let before = head_rows_in_other_queues(older, &self.queue_id, &self.head_identity);
        let after = head_rows_in_other_queues(&self.newer, &self.queue_id, &self.head_identity);
        let ids: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        ids.into_iter().any(|id| {
            let was = before.get(id).copied().unwrap_or_default();
            let now = after.get(id).copied().unwrap_or_default();
            (now.completed && !was.completed) || (was.live && !now.live)
        })
    }

    /// The exchange an answering echo must be newer than, or `None` when every
    /// echo counts (no history, or every committed version postdates the pass
    /// opening with the head live and no other queue's echo).
    pub fn finish(self, limit: usize) -> Option<LoopPassEchoAnchor> {
        if self.anchor.is_some() {
            return self.anchor;
        }
        if self.visited >= limit {
            return self.oldest.map(|exchange| LoopPassEchoAnchor {
                exchange,
                kind: LoopPassEchoAnchorKind::HistoryLimit,
            });
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct HeadRows {
    live: bool,
    completed: bool,
}

/// Per queue id other than `queue_id`: whether that queue holds a live or a
/// struck row with entry identity `head_identity`.
fn head_rows_in_other_queues(
    content: &str,
    queue_id: &str,
    head_identity: &str,
) -> std::collections::BTreeMap<String, HeadRows> {
    let mut rows = std::collections::BTreeMap::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return rows;
    };
    let Ok(graph) = crate::queue_graph::QueueGraph::parse(content, &components) else {
        return rows;
    };
    for block in graph.blocks() {
        if block.id == queue_id {
            continue;
        }
        let mut state = HeadRows::default();
        for entry in &block.entries {
            match entry {
                QueueEntry::Prompt(prompt)
                    if free_text_head_entry_identity(&prompt.text) == head_identity =>
                {
                    state.live = true;
                }
                QueueEntry::Completed(prompt)
                    if free_text_head_entry_identity(&prompt.text) == head_identity =>
                {
                    state.completed = true;
                }
                _ => {}
            }
        }
        rows.insert(block.id.clone(), state);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(exchange: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    const REPORT: &str = "the session list flickers when a second pane opens";

    #[test]
    fn entry_identity_ignores_priority_markers_case_and_spacing() {
        assert_eq!(
            free_text_head_entry_identity(":pushpin: The  Session list\nflickers"),
            free_text_head_entry_identity("the session list flickers"),
        );
    }

    #[test]
    fn walk_anchors_at_the_newest_version_where_the_head_was_not_live() {
        let answer = format!("> **Queue prompt:** {REPORT}\n\nFixed.\n");
        let newest = doc(&format!("{answer}Other.\n"), &format!("- {REPORT}\n"));
        let struck = doc(&answer, &format!("- ~~{REPORT}~~\n"));
        let mut walk = FreeTextHeadEntryWalk::new("", REPORT);
        assert!(walk.visit(Some(&newest)));
        assert!(!walk.visit(Some(&struck)));
        assert_eq!(
            walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT),
            Some(answer)
        );
    }

    #[test]
    fn walk_without_an_entry_point_counts_every_echo() {
        let live = doc("", &format!("- {REPORT}\n"));
        let mut walk = FreeTextHeadEntryWalk::new("", REPORT);
        assert!(walk.visit(Some(&live)));
        assert_eq!(
            walk.clone().finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT),
            None
        );
        // At the limit with the head still live, the oldest version read anchors.
        assert_eq!(walk.finish(1), Some(String::new()));
        // No history at all: every echo counts.
        assert_eq!(
            FreeTextHeadEntryWalk::new("", REPORT).finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT),
            None
        );
    }

    #[test]
    fn walk_anchors_on_an_absent_path_with_an_empty_exchange() {
        let mut walk = FreeTextHeadEntryWalk::new("", REPORT);
        assert!(!walk.visit(None));
        assert_eq!(
            walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT),
            Some(String::new())
        );
    }

    #[test]
    fn exchange_after_head_entry_scopes_only_with_an_anchor() {
        let old = "### Re: a\n\nOld.\n";
        let full = format!("{old}### Re: b\n\nNew.\n");
        assert_eq!(exchange_after_head_entry(&full, None), full);
        let scoped = exchange_after_head_entry(&full, Some(old));
        assert!(
            scoped.contains("New.") && !scoped.contains("Old."),
            "{scoped}"
        );
    }

    /// GH #327 pass 4 fixture: a priority `go` queue and a `loop` queue, both
    /// in one document.
    fn two_queue_doc(exchange: &str, main: &str, looped: &str) -> String {
        format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue priority id=queue-1 go -->\n{main}<!-- /agent:queue -->\n\n<!-- agent:queue loop interval=\"1h\" id=queue-2 -->\n{looped}<!-- /agent:queue -->\n"
        )
    }

    const LOOP_HEAD: &str = "#upgrade";
    const OPENED: u64 = 1_800_000_000;

    fn upgrade_echo() -> String {
        format!("### Re: upgrade\n\n> **Queue prompt:** {LOOP_HEAD}\n\nUpgraded.\n\n")
    }

    /// The measured pass 4: the echo was committed 94 s before the pass
    /// opened. The newest version committed at or before the opening anchors,
    /// so the echo is history for this pass.
    #[test]
    fn loop_walk_anchors_at_the_version_committed_before_the_pass_opened() {
        let echo = upgrade_echo();
        let live = format!("- {LOOP_HEAD}\n");
        let answered = two_queue_doc(&echo, &format!("- ~~{LOOP_HEAD}~~\n"), &live);
        let mut walk = LoopPassEchoWalk::new("queue-2", LOOP_HEAD, OPENED, &answered);
        assert!(!walk.visit(OPENED - 94, Some(&answered)));
        let anchor = walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT).unwrap();
        assert_eq!(anchor.kind, LoopPassEchoAnchorKind::PassOpened);
        let newer = crate::queue_response::exchange_text_newer_than(
            &exchange_component_text(&answered),
            &anchor.exchange,
        );
        assert!(
            !crate::queue_response::free_text_head_answered_by_response(&newer, LOOP_HEAD),
            "an echo committed before the pass opened must not answer the loop head: {newer:?}"
        );
    }

    /// An echo written after the pass opened, by a write that struck the same
    /// head in another queue and left the loop queue's copy live, answered the
    /// other queue's head.
    #[test]
    fn loop_walk_anchors_after_an_echo_that_struck_another_queues_head() {
        let echo = upgrade_echo();
        let live = format!("- {LOOP_HEAD}\n");
        for retired in [
            format!("- ~~{LOOP_HEAD}~~ — auto-struck: answered this cycle (#ftstrike)\n"),
            String::new(),
        ] {
            let before = two_queue_doc("", &live, &live);
            let answered = two_queue_doc(&echo, &retired, &live);
            let mut walk = LoopPassEchoWalk::new("queue-2", LOOP_HEAD, OPENED, &answered);
            assert!(walk.visit(OPENED + 30, Some(&answered)));
            assert!(!walk.visit(OPENED + 10, Some(&before)));
            let anchor = walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT).unwrap();
            assert_eq!(
                anchor.kind,
                LoopPassEchoAnchorKind::OtherQueueEcho,
                "{retired:?}"
            );
            assert_eq!(anchor.exchange, echo, "{retired:?}");
        }
    }

    /// Positive control: the loop pass's own cycle answered the loop head
    /// after the pass opened, and no other queue held it. Every committed
    /// version postdates the opening, so the echo counts.
    #[test]
    fn loop_walk_counts_the_passes_own_echo() {
        let echo = upgrade_echo();
        let live = format!("- {LOOP_HEAD}\n");
        let before = two_queue_doc("", "", &live);
        let answered = two_queue_doc(&echo, "", &live);
        let mut walk = LoopPassEchoWalk::new("queue-2", LOOP_HEAD, OPENED, &answered);
        assert!(walk.visit(OPENED + 30, Some(&answered)));
        assert!(!walk.visit(OPENED - 600, Some(&before)));
        let anchor = walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT).unwrap();
        assert_eq!(anchor.kind, LoopPassEchoAnchorKind::PassOpened);
        let newer = crate::queue_response::exchange_text_newer_than(
            &exchange_component_text(&answered),
            &anchor.exchange,
        );
        assert!(crate::queue_response::free_text_head_answered_by_response(
            &newer, LOOP_HEAD
        ));
    }

    /// An uncommitted write is attributed too: the walk starts from the
    /// current content.
    #[test]
    fn loop_walk_attributes_an_uncommitted_other_queue_echo() {
        let echo = upgrade_echo();
        let live = format!("- {LOOP_HEAD}\n");
        let committed = two_queue_doc("", &live, &live);
        let current = two_queue_doc(&echo, &format!("- ~~{LOOP_HEAD}~~\n"), &live);
        let mut walk = LoopPassEchoWalk::new("queue-2", LOOP_HEAD, OPENED, &current);
        assert!(!walk.visit(OPENED + 10, Some(&committed)));
        let anchor = walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT).unwrap();
        assert_eq!(anchor.kind, LoopPassEchoAnchorKind::OtherQueueEcho);
        assert_eq!(anchor.exchange, echo);
    }

    /// An old struck copy in another queue that the write did not touch does
    /// not claim a new echo.
    #[test]
    fn loop_walk_ignores_another_queues_old_struck_row() {
        let echo = upgrade_echo();
        let live = format!("- {LOOP_HEAD}\n");
        let struck = format!("- ~~{LOOP_HEAD}~~\n");
        let before = two_queue_doc("", &struck, &live);
        let answered = two_queue_doc(&echo, &struck, &live);
        let mut walk = LoopPassEchoWalk::new("queue-2", LOOP_HEAD, OPENED, &answered);
        assert!(walk.visit(OPENED + 30, Some(&answered)));
        assert!(walk.visit(OPENED + 10, Some(&before)));
        assert_eq!(walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT), None);
    }
}
