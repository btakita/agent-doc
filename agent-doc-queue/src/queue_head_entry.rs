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
}
