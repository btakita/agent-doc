//! Why a preflight found nothing to do (`#noopnamefault`).
//!
//! A `no_changes` preflight used to emit a bare `no_changes: true` with
//! `diff: null`. When the operator re-ran the document several times, the
//! agent had nothing from the binary that said *what* was compared or *why*
//! nothing ran, so it guessed. On `tasks/software/lazily.md` (2026-10-03,
//! cycle-1791003516299 already committed) it told the operator to "save the
//! file in the editor", even though:
//!
//! * preflight had read the live JetBrains buffer through the CRDT relay
//!   (`realtime_doc_resolve authority=editor_buffer reason=crdt_relay_current`),
//!   and the plugin's own published buffer hash equalled the committed HEAD
//!   (`215f3ffe…`, 21532 bytes), so there was no unsaved edit to save; and
//! * the one thing the operator was waiting on, `do [#lzwiremodel]`, sat in an
//!   `agent:queue` that was stopped (a legacy `queue: stop`, since retired by
//!   `#queuestopremove`; the one hold is now `queue: pause`), so
//!   preflight reported `queue_active: false` and
//!   `queue_drainable_head_count: 0` without naming the waiting item.
//!
//! This module owns the pure policy that turns those facts into an explanation
//! the agent relays instead of inventing one: which source preflight read, which
//! queue items are waiting on a go-ahead, and the guidance text.

use agent_doc_element_queue::strip_priority_markers;
use serde::{Deserialize, Serialize};

/// Where the preflight read the current document from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoChangesReadSource {
    /// A live agent-doc editor buffer (CRDT relay or an in-sync editor). Unsaved
    /// typing in that buffer is already visible to preflight.
    LiveEditor,
    /// Disk, with no agent-doc editor buffer attached. An edit in an editor
    /// without the agent-doc plugin is invisible until it is saved.
    Disk,
}

impl NoChangesReadSource {
    /// Classify a resolved read. `authority_is_editor_buffer` is the read
    /// authority; `reason` is its stable reason code. An `in_sync` read resolves
    /// to disk authority but still proves an editor buffer was observed and
    /// matched disk, so it counts as a live-editor read.
    pub fn from_read(authority_is_editor_buffer: bool, reason: &str) -> Self {
        if authority_is_editor_buffer || reason == "in_sync" {
            Self::LiveEditor
        } else {
            Self::Disk
        }
    }
}

/// Facts the preflight already holds when it reports `no_changes`.
#[derive(Debug, Clone, Copy)]
pub struct NoChangesFacts<'a> {
    pub read_source: NoChangesReadSource,
    /// The document content preflight compared against the baseline.
    pub content: &'a str,
    /// Whether the queue will run on its own: active, or deferred to a start
    /// time. Waiting items are only reported when it will not.
    pub queue_runs: bool,
}

/// Binary-authored explanation of a `no_changes` preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoChangesExplanation {
    pub read_source: NoChangesReadSource,
    /// Queue items present in a stopped `agent:queue`, verbatim (markers
    /// stripped). They are why an operator may expect work that never starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_queue_items: Vec<String>,
    /// What to tell the operator. Relay this instead of improvising a cause.
    pub guidance: String,
}

/// Items in the document's `agent:queue`, in order, priority/lifecycle markers
/// stripped. Struck (`~~…~~`) lines are skipped: they are done.
pub fn queue_item_texts(content: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let Ok(entries) = crate::document_queue::parse(queue.content(content)) else {
        return Vec::new();
    };
    crate::document_queue::prompts(&entries)
        .into_iter()
        .filter_map(|prompt| {
            let text = strip_priority_markers(&prompt.text);
            let text = text.trim();
            (!text.is_empty() && !text.starts_with("~~")).then(|| text.to_string())
        })
        .collect()
}

/// Build the explanation for a `no_changes` preflight.
pub fn explain_no_changes(facts: NoChangesFacts<'_>) -> NoChangesExplanation {
    let mut guidance = match facts.read_source {
        NoChangesReadSource::LiveEditor => {
            "Nothing new: preflight read the live editor buffer and it matches the last \
             committed cycle. Unsaved typing is already visible to agent-doc, so do NOT tell \
             the operator to save the file. If they say they edited, that edit is not in this \
             document's editor buffer (a different file or tab, or it was undone); say so."
                .to_string()
        }
        NoChangesReadSource::Disk => {
            "Nothing new: preflight read the document from disk (no agent-doc editor buffer is \
             attached) and it matches the last committed cycle. An edit made in an editor \
             without the agent-doc plugin is not visible until it is saved to disk."
                .to_string()
        }
    };
    let waiting_queue_items = if facts.queue_runs {
        Vec::new()
    } else {
        queue_item_texts(facts.content)
    };
    if !waiting_queue_items.is_empty() {
        let listed = waiting_queue_items
            .iter()
            .map(|item| format!("`{item}`"))
            .collect::<Vec<_>>()
            .join(", ");
        guidance.push_str(&format!(
            " The `agent:queue` holds {} item(s) that will not run because the queue is \
             held (`queue: pause`): {listed}. Tell the operator these are waiting on a go-ahead; \
             to run them they delete the `queue: pause` line (or write `go` on the \
             `agent:queue` marker). Do not start them without that.",
            waiting_queue_items.len()
        ));
    }
    NoChangesExplanation {
        read_source: facts.read_source,
        waiting_queue_items,
        guidance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lazily.md shape from 2026-10-03: a stopped queue holding one
    /// recommended item, read through a live editor.
    const STOPPED_QUEUE_DOC: &str = concat!(
        "---\n",
        "queue: pause\n",
        "---\n\n",
        "## Queue\n\n",
        "<!-- agent:queue preset=\"#spec-test-commit-push\" priority -->\n",
        "- do [#lzwiremodel]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn live_editor_no_changes_names_the_stopped_queue_item_and_forbids_save_advice() {
        let explanation = explain_no_changes(NoChangesFacts {
            read_source: NoChangesReadSource::LiveEditor,
            content: STOPPED_QUEUE_DOC,
            queue_runs: false,
        });
        assert_eq!(explanation.read_source, NoChangesReadSource::LiveEditor);
        assert_eq!(explanation.waiting_queue_items, vec!["do [#lzwiremodel]"]);
        assert!(
            explanation
                .guidance
                .contains("do NOT tell the operator to save"),
            "{}",
            explanation.guidance
        );
        assert!(
            explanation.guidance.contains("`do [#lzwiremodel]`")
                && explanation.guidance.contains("`go`"),
            "{}",
            explanation.guidance
        );
    }

    #[test]
    fn disk_read_explains_that_a_plugin_less_editor_must_save() {
        let explanation = explain_no_changes(NoChangesFacts {
            read_source: NoChangesReadSource::Disk,
            content: "no queue here\n",
            queue_runs: false,
        });
        assert!(explanation.waiting_queue_items.is_empty());
        assert!(explanation.guidance.contains("saved to disk"));
        assert!(!explanation.guidance.contains("agent:queue"));
    }

    #[test]
    fn a_running_queue_reports_no_waiting_items() {
        let explanation = explain_no_changes(NoChangesFacts {
            read_source: NoChangesReadSource::LiveEditor,
            content: STOPPED_QUEUE_DOC,
            queue_runs: true,
        });
        assert!(explanation.waiting_queue_items.is_empty());
        assert!(!explanation.guidance.contains("agent:queue"));
    }

    #[test]
    fn struck_and_marked_items_are_reported_clean() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#done]~~\n",
            "- 🚧 do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(queue_item_texts(doc), vec!["do [#next]"]);
    }

    #[test]
    fn in_sync_editor_read_counts_as_live_editor() {
        assert_eq!(
            NoChangesReadSource::from_read(false, "in_sync"),
            NoChangesReadSource::LiveEditor
        );
        assert_eq!(
            NoChangesReadSource::from_read(true, "crdt_relay_current"),
            NoChangesReadSource::LiveEditor
        );
        assert_eq!(
            NoChangesReadSource::from_read(false, "editor_absent"),
            NoChangesReadSource::Disk
        );
    }
}
