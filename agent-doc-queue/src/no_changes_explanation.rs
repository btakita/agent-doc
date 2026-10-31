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

use crate::queue_claim::QueueClaimState;
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

/// The queue lifecycle state that explains why an unchanged document did not
/// open a turn (`#nochangepausephantom`).
///
/// This is deliberately distinct from drainability. An active `go` queue can
/// have no fresh drainable head while workers are running or lifecycle rows
/// await coordinator reconciliation; that state is not an explicit pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoChangesQueueLifecycle {
    Absent,
    ExplicitlyPaused,
    ControllerPaused,
    Deferred,
    Halted,
    Inactive,
    ActiveNoDrainableHead,
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

/// A live queue head an unexpired worker claim holds (`#explicitrunqueue`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedQueueItemFact {
    /// The queue head text, lifecycle markers stripped.
    pub item: String,
    /// The claim owner, e.g. `subagent:runctrlclaude`.
    pub owner: String,
    /// Exact fenced attempt generation shown in the queue annotation.
    pub generation: u64,
    /// Durable attempt state; unlike the old generic "in flight" wording this
    /// distinguishes dispatch reservation, worker binding, and reported debt.
    pub state: QueueClaimState,
    /// Concrete harness worker positively bound by `queue start`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// When the worker binding was acknowledged, ISO-8601 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_started_at: Option<String>,
    /// `live`, `unrecorded`, or `ended`. This states only coordinator-session
    /// liveness; it is not a worker heartbeat or external-job observation.
    pub owner_session_state: String,
    /// When the claim expires, ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
    pub expires_at: String,
    /// `#claimownerliveness`: set when the coordinator session that made the
    /// claim is known to have ended (`/clear`, restart, exit). The claim still
    /// holds the head, but nothing proves its work is in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session_ended: Option<StaleClaimFact>,
}

/// Why a claimed head is a **stale claim** and how the current coordinator
/// resolves it (`#claimownerliveness`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleClaimFact {
    /// Stable reason token (`owner_process_exited`, `session_replaced`).
    pub reason: String,
    /// Operator-facing reason.
    pub detail: String,
    /// Re-bind the claim to the current session, for a subagent that is
    /// still working and reporting back.
    pub adopt_command: String,
    /// Release the claim so the head is dispatched again.
    pub release_command: String,
}

/// Facts the preflight already holds when it reports `no_changes`.
#[derive(Debug, Clone, Copy)]
pub struct NoChangesFacts<'a> {
    pub read_source: NoChangesReadSource,
    /// The document content preflight compared against the baseline.
    pub content: &'a str,
    /// The settled queue activation projection. `Some(false)` is not by itself
    /// proof of an explicit pause; the visible control is classified below.
    pub queue_active: Option<bool>,
    pub queue_deferred: bool,
    pub queue_halted: Option<&'a str>,
    /// Controller `admin queue pause`, which only pauses unattended dispatch.
    pub queue_controller_paused: bool,
    /// Live heads a durable claim holds. They are reserved from this session,
    /// but the claim alone is not a worker heartbeat or external-job status.
    pub claimed_queue_items: &'a [ClaimedQueueItemFact],
}

/// Binary-authored explanation of a `no_changes` preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoChangesExplanation {
    pub read_source: NoChangesReadSource,
    /// The actual queue lifecycle state. This prevents callers from translating
    /// every `drainable_head_count == 0` outcome into a fictitious pause.
    pub queue_lifecycle: NoChangesQueueLifecycle,
    /// Queue items present in a stopped `agent:queue`, verbatim (markers
    /// stripped). They are why an operator may expect work that never starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_queue_items: Vec<String>,
    /// Queue heads held by a durable worker claim (`#explicitrunqueue`): why an
    /// explicit Run Agent Doc on a queue whose heads are all claimed opens no
    /// cycle. Relay the exact attempt state and its observation limits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claimed_queue_items: Vec<ClaimedQueueItemFact>,
    /// What to tell the operator. Relay this instead of improvising a cause.
    pub guidance: String,
}

fn has_queue_component(content: &str) -> bool {
    agent_doc_element::element::parse(content)
        .ok()
        .is_some_and(|components| components.iter().any(|component| component.name == "queue"))
}

fn explicitly_paused(content: &str) -> bool {
    let Ok((frontmatter, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return false;
    };
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return false;
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    crate::control_binding::explicit_queue_pause_mode(
        &queue.attrs,
        crate::control_binding::frontmatter_queue_control(&frontmatter),
    )
}

fn queue_lifecycle(facts: NoChangesFacts<'_>) -> NoChangesQueueLifecycle {
    if !has_queue_component(facts.content) {
        NoChangesQueueLifecycle::Absent
    } else if explicitly_paused(facts.content) {
        NoChangesQueueLifecycle::ExplicitlyPaused
    } else if facts.queue_halted.is_some() {
        NoChangesQueueLifecycle::Halted
    } else if facts.queue_deferred {
        NoChangesQueueLifecycle::Deferred
    } else if facts.queue_controller_paused {
        NoChangesQueueLifecycle::ControllerPaused
    } else if facts.queue_active == Some(true) {
        NoChangesQueueLifecycle::ActiveNoDrainableHead
    } else {
        NoChangesQueueLifecycle::Inactive
    }
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

/// `#explicitrunqueue` / `#claimfreshnessdiagnostics`: the non-error outcome
/// for queue heads a durable claim holds. It names the exact attempt state,
/// worker binding, owner-session evidence, and expiry without converting those
/// facts into an unobservable worker-heartbeat or external-progress claim.
///
/// `#claimownerliveness`: a claim whose owner session ended is a **stale
/// claim**. It is never described as in flight; the guidance names why it is
/// unverifiable and gives the two exits (adopt or release) instead of "do NOT
/// release", which kept a post-`/clear` coordinator reporting a subagent it had
/// no record of as running.
pub fn claimed_queue_guidance(claimed: &[ClaimedQueueItemFact]) -> String {
    let (stale, live): (Vec<_>, Vec<_>) = claimed
        .iter()
        .partition(|fact| fact.owner_session_ended.is_some());
    let mut parts = Vec::new();
    if !live.is_empty() {
        let listed = live
            .iter()
            .map(|fact| {
                let owner_state = fact.owner_session_state.as_str();
                match (fact.state, fact.worker_id.as_deref()) {
                    (QueueClaimState::Claimed, _) => format!(
                        "`{}` has durable state `claimed` (dispatching, no worker bound) for {} generation {}; coordinator session {}; expires {}",
                        fact.item, fact.owner, fact.generation, owner_state, fact.expires_at
                    ),
                    (QueueClaimState::Running, Some(worker)) => format!(
                        "`{}` has durable state `running`: worker `{worker}` is bound to {} generation {}; coordinator session {}; worker started {}; expires {}",
                        fact.item,
                        fact.owner,
                        fact.generation,
                        owner_state,
                        fact.worker_started_at.as_deref().unwrap_or("at an unknown time"),
                        fact.expires_at
                    ),
                    (QueueClaimState::Running, None) => format!(
                        "`{}` has inconsistent durable state `running` without a worker binding for {} generation {}; coordinator session {}; expires {}",
                        fact.item, fact.owner, fact.generation, owner_state, fact.expires_at
                    ),
                    (QueueClaimState::Reported, _) => format!(
                        "`{}` has durable state `reported` for {} generation {}; coordinator integration and closeout remain; expires {}",
                        fact.item, fact.owner, fact.generation, fact.expires_at
                    ),
                    (QueueClaimState::Lost, _) => format!(
                        "`{}` has durable state `lost` for {} generation {}; recovery is required",
                        fact.item, fact.owner, fact.generation
                    ),
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        parts.push(format!(
            "Queue claim state: {listed}. These are durable dispatch facts, not a worker heartbeat \
             or proof of progress in an external job. Nothing to dispatch in this session; do NOT \
             respond to, re-dispatch, re-claim, or release a claim solely because its original \
             claim timestamp is old. Inspect the named worker/external wait when progress matters; \
             the item returns to the queue when its owner finishes or the claim expires."
        ));
    }
    for fact in stale {
        let Some(ended) = fact.owner_session_ended.as_ref() else {
            continue;
        };
        parts.push(format!(
            "Stale claim: `{}` is claimed by {} (expires {}), but {}, so nothing proves this work \
             is in flight; do NOT tell the operator it is running. If one of YOUR live subagents \
             is working on it, adopt the claim: `{}`. Otherwise release it so it is dispatched \
             again: `{}`.",
            fact.item,
            fact.owner,
            fact.expires_at,
            ended.detail,
            ended.adopt_command,
            ended.release_command
        ));
    }
    parts.join(" ")
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
    let queue_lifecycle = queue_lifecycle(facts);
    let queue_items = queue_item_texts(facts.content);
    let waiting_queue_items = if queue_lifecycle == NoChangesQueueLifecycle::ExplicitlyPaused {
        queue_items.clone()
    } else {
        Vec::new()
    };
    if !waiting_queue_items.is_empty() {
        let listed = waiting_queue_items
            .iter()
            .map(|item| format!("`{item}`"))
            .collect::<Vec<_>>()
            .join(", ");
        guidance.push_str(&format!(
            " The `agent:queue` is explicitly paused by visible queue control and holds {} \
             item(s): {listed}. Tell the operator these are waiting on that explicit go-ahead; \
             to run them they delete the `queue: pause` line (or write `go` on the \
             `agent:queue` marker). Do not start them without that.",
            waiting_queue_items.len()
        ));
    } else if !queue_items.is_empty() {
        let listed = queue_items
            .iter()
            .map(|item| format!("`{item}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let lifecycle_guidance = match queue_lifecycle {
            NoChangesQueueLifecycle::ControllerPaused => {
                "The queue is in `go` mode with an `admin queue pause` controlling unattended \
                 supervisor dispatch; it is not paused by a `queue: pause` line. Name the \
                 controller pause and use `agent-doc admin queue resume` only if the operator \
                 wants unattended dispatch resumed"
            }
            NoChangesQueueLifecycle::Deferred => {
                "The queue is deferred by its current time gate; it is not explicitly paused"
            }
            NoChangesQueueLifecycle::Halted => {
                "The queue is halted at a stop fence; it is not explicitly paused by frontmatter"
            }
            NoChangesQueueLifecycle::Inactive => {
                "The queue is inactive without an explicit pause; add an accepted `go` control \
                 to start it rather than telling the operator to remove a nonexistent pause"
            }
            NoChangesQueueLifecycle::ActiveNoDrainableHead => {
                "The queue is active in `go` mode but currently has no selected drainable head. \
                 This is a lifecycle wait (for example running work or non-drainable rows), not \
                 an explicit pause; do NOT tell the operator to remove a `queue: pause` line"
            }
            NoChangesQueueLifecycle::Absent | NoChangesQueueLifecycle::ExplicitlyPaused => "",
        };
        if !lifecycle_guidance.is_empty() {
            guidance.push_str(&format!(
                " Queue lifecycle `{}`: {lifecycle_guidance}. Live queue item(s): {listed}.",
                match queue_lifecycle {
                    NoChangesQueueLifecycle::Absent => "absent",
                    NoChangesQueueLifecycle::ExplicitlyPaused => "explicitly_paused",
                    NoChangesQueueLifecycle::ControllerPaused => "controller_paused",
                    NoChangesQueueLifecycle::Deferred => "deferred",
                    NoChangesQueueLifecycle::Halted => "halted",
                    NoChangesQueueLifecycle::Inactive => "inactive",
                    NoChangesQueueLifecycle::ActiveNoDrainableHead => {
                        "active_no_drainable_head"
                    }
                }
            ));
        }
    }
    let claimed_queue_items = facts.claimed_queue_items.to_vec();
    if !claimed_queue_items.is_empty() {
        guidance.push_str(&format!(
            " {}",
            claimed_queue_guidance(&claimed_queue_items)
        ));
    }
    NoChangesExplanation {
        read_source: facts.read_source,
        queue_lifecycle,
        waiting_queue_items,
        claimed_queue_items,
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
            queue_active: Some(false),
            queue_deferred: false,
            queue_halted: None,
            queue_controller_paused: false,
            claimed_queue_items: &[],
        });
        assert_eq!(explanation.read_source, NoChangesReadSource::LiveEditor);
        assert_eq!(
            explanation.queue_lifecycle,
            NoChangesQueueLifecycle::ExplicitlyPaused
        );
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
            queue_active: None,
            queue_deferred: false,
            queue_halted: None,
            queue_controller_paused: false,
            claimed_queue_items: &[],
        });
        assert_eq!(explanation.queue_lifecycle, NoChangesQueueLifecycle::Absent);
        assert!(explanation.waiting_queue_items.is_empty());
        assert!(explanation.guidance.contains("saved to disk"));
        assert!(!explanation.guidance.contains("agent:queue"));
    }

    #[test]
    fn visible_pause_control_wins_over_a_stale_active_projection() {
        let explanation = explain_no_changes(NoChangesFacts {
            read_source: NoChangesReadSource::LiveEditor,
            content: STOPPED_QUEUE_DOC,
            queue_active: Some(true),
            queue_deferred: false,
            queue_halted: None,
            queue_controller_paused: false,
            claimed_queue_items: &[],
        });
        // Visible pause control is authoritative even if a stale caller says
        // active; lifecycle classification never invents a running queue.
        assert_eq!(
            explanation.queue_lifecycle,
            NoChangesQueueLifecycle::ExplicitlyPaused
        );
        assert!(!explanation.waiting_queue_items.is_empty());
    }

    /// `#explicitrunqueue`: an explicit Run Agent Doc whose only live head is
    /// claimed by a subagent names the owner and expiry as a non-error outcome
    /// and tells the agent to leave the claim alone.
    #[test]
    fn claimed_heads_are_named_with_owner_and_expiry() {
        let claimed = [ClaimedQueueItemFact {
            item: "do [#runctrlclaude]".to_string(),
            owner: "subagent:runctrlclaude".to_string(),
            generation: 1,
            state: QueueClaimState::Claimed,
            worker_id: None,
            worker_started_at: None,
            owner_session_state: "live".to_string(),
            expires_at: "2026-10-10T19:32:32Z".to_string(),
            owner_session_ended: None,
        }];
        let explanation = explain_no_changes(NoChangesFacts {
            read_source: NoChangesReadSource::LiveEditor,
            content: "<!-- agent:queue -->\n- do [#runctrlclaude]\n<!-- /agent:queue -->\n",
            queue_active: Some(true),
            queue_deferred: false,
            queue_halted: None,
            queue_controller_paused: false,
            claimed_queue_items: &claimed,
        });
        assert_eq!(explanation.claimed_queue_items, claimed.to_vec());
        assert!(explanation.waiting_queue_items.is_empty());
        assert_eq!(
            explanation.queue_lifecycle,
            NoChangesQueueLifecycle::ActiveNoDrainableHead
        );
        assert!(
            explanation
                .guidance
                .contains("not an explicit pause; do NOT tell the operator to remove")
        );
        assert!(
            !explanation
                .guidance
                .contains("delete the `queue: pause` line")
        );
        assert!(
            explanation.guidance.contains(
                "`do [#runctrlclaude]` has durable state `claimed` (dispatching, no worker bound) for subagent:runctrlclaude generation 1; coordinator session live; expires 2026-10-10T19:32:32Z"
            ),
            "{}",
            explanation.guidance
        );
        assert!(explanation.guidance.contains("Nothing to dispatch"));
        assert!(
            explanation
                .guidance
                .contains("durable dispatch facts, not a worker heartbeat")
        );
        let json = serde_json::to_value(&explanation).unwrap();
        assert_eq!(
            json["claimed_queue_items"][0]["owner"],
            "subagent:runctrlclaude"
        );
    }

    /// `#claimownerliveness` (haiven-dev tasks/sdk.md, 2026-10-11): a claim
    /// whose coordinator session was `/clear`ed is reported as a stale claim
    /// with adopt/release commands, never as in-flight work with "do NOT
    /// release"; a live claim beside it keeps the in-flight wording.
    #[test]
    fn stale_claims_are_named_unverifiable_with_adopt_and_release_exits() {
        let claimed = [
            ClaimedQueueItemFact {
                item: "do [#cligenport]".to_string(),
                owner: "subagent:cligoshell-then-cligenport".to_string(),
                generation: 4,
                state: QueueClaimState::Running,
                worker_id: Some("worker:cligenport".to_string()),
                worker_started_at: Some("2026-10-11T07:00:00Z".to_string()),
                owner_session_state: "ended".to_string(),
                expires_at: "2026-10-11T08:30:00Z".to_string(),
                owner_session_ended: Some(StaleClaimFact {
                    reason: "session_replaced".to_string(),
                    detail: "the claiming coordinator session was replaced in its pane".to_string(),
                    adopt_command: "agent-doc queue claim tasks/sdk.md --item '#cligenport' \
                                    --owner subagent:cligoshell-then-cligenport --refresh"
                        .to_string(),
                    release_command: "agent-doc queue release tasks/sdk.md --item '#cligenport'"
                        .to_string(),
                }),
            },
            ClaimedQueueItemFact {
                item: "do [#live]".to_string(),
                owner: "subagent:live".to_string(),
                generation: 2,
                state: QueueClaimState::Running,
                worker_id: Some("worker:live".to_string()),
                worker_started_at: Some("2026-10-11T07:30:00Z".to_string()),
                owner_session_state: "live".to_string(),
                expires_at: "2026-10-11T09:00:00Z".to_string(),
                owner_session_ended: None,
            },
        ];
        let guidance = claimed_queue_guidance(&claimed);
        let (live_part, stale_part) = guidance.split_once("Stale claim:").unwrap();
        assert!(
            live_part.contains(
                "`do [#live]` has durable state `running`: worker `worker:live` is bound to subagent:live generation 2"
            ),
            "{guidance}"
        );
        assert!(
            live_part.contains("not a worker heartbeat or proof of progress in an external job"),
            "{guidance}"
        );
        assert!(!live_part.contains("cligenport"), "{guidance}");
        assert!(
            stale_part.contains("nothing proves this work is in flight"),
            "{guidance}"
        );
        assert!(stale_part.contains("--refresh"), "{guidance}");
        assert!(
            stale_part.contains("agent-doc queue release tasks/sdk.md --item '#cligenport'"),
            "{guidance}"
        );
        assert!(!stale_part.contains("do NOT respond"), "{guidance}");
        let json = serde_json::to_value(&claimed[0]).unwrap();
        assert_eq!(json["owner_session_ended"]["reason"], "session_replaced");
        assert!(
            serde_json::to_value(&claimed[1])
                .unwrap()
                .get("owner_session_ended")
                .is_none()
        );
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
