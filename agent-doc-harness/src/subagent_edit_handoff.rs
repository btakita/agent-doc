//! Deliver an operator edit to the running subagent (`#harnessagenthandoff`).
//!
//! [`render_forward_to_owner`] is the harness-neutral forward-to-owner text.
//! Tool names come from the Grok Build user guide or from agent-doc's
//! orchestration spec. A harness with no such tool, or one that addresses a
//! worker and has no worker id, says the handoff did not happen.
//!
//! `fix/grok-send-subagent-message` owns `midturn_steering.rs`. Wire it there
//! with one call, after adding this dependency to
//! `agent-doc-document-realtime/Cargo.toml`:
//!
//! `agent-doc-harness = { path = "../agent-doc-harness", version = "0.35.570" }`
//!
//! ```ignore
//! agent_doc_harness::subagent_edit_handoff::render_forward_to_owner(
//!     &agent_doc_harness::subagent_edit_handoff::OperatorEditDelivery {
//!         harness,
//!         worker_id: item.worker_id.as_deref(),
//!         previous: item.previous.as_deref(),
//!         verbatim: &item.verbatim,
//!         change: match item.change {
//!             SteeringChange::Added => EditHandoffChange::Added,
//!             _ => EditHandoffChange::Edited,
//!         },
//!         requeue_command: &requeue,
//!     },
//! )
//! ```
//!
//! Replace the whole `forward_to_owner_instruction` body with that call. Pass
//! the harness explicitly. Do not call `detect_harness`.

/// Phrase every failed delivery uses. The handoff did not happen.
pub const HANDOFF_DID_NOT_HAPPEN: &str = "the handoff did NOT happen";

/// Whether the operator edited the claimed line or added a note on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditHandoffChange {
    Edited,
    Added,
}

/// How the harness names the running worker. No invented parameter names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerAddress {
    /// Grok `send_subagent_message` parameter `subagent_id`. Never `parent`.
    GrokSubagentId,
    /// Claude Code `SendMessage` to the running subagent. No documented parameter.
    ClaudeRunningSubagent,
    /// Codex `followup_task` resumes the worker. No documented parameter name.
    CodexFollowupTask,
    /// No documented mid-run edit tool.
    Unsupported,
}

/// One harness's ability to deliver an operator edit to a running subagent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubagentEditCapability {
    pub harness: &'static str,
    pub tool: Option<&'static str>,
    /// `Some("steer")` only when that harness documents the mode.
    pub delivery: Option<&'static str>,
    pub worker_address: WorkerAddress,
    /// Why a call cannot be made. The rendered text then says the handoff did not happen.
    pub unavailable: &'static str,
}

/// Inputs for [`render_forward_to_owner`]. The caller passes the harness.
pub struct OperatorEditDelivery<'a> {
    pub harness: &'a str,
    pub worker_id: Option<&'a str>,
    pub previous: Option<&'a str>,
    pub verbatim: &'a str,
    pub change: EditHandoffChange,
    /// Generation-fenced `agent-doc queue release ... --generation N --requeue`.
    pub requeue_command: &'a str,
}

/// Capability for `harness`. Unknown names, including `default`, are unsupported.
pub fn subagent_edit_capability(harness: &str) -> SubagentEditCapability {
    match canonical_harness(harness) {
        "grok" => SubagentEditCapability {
            harness: "grok",
            tool: Some("send_subagent_message"),
            delivery: Some("steer"),
            worker_address: WorkerAddress::GrokSubagentId,
            unavailable: "`send_subagent_message` is not in the tool list when \
                 `active_agent_messages` is off (`GROK_ACTIVE_AGENT_MESSAGES` or \
                 `[features] active_agent_messages`), or the send is rejected, or the \
                 exact worker is unavailable or has exited",
        },
        "claude" => SubagentEditCapability {
            harness: "claude",
            tool: Some("SendMessage"),
            delivery: None,
            worker_address: WorkerAddress::ClaudeRunningSubagent,
            unavailable: "the exact worker is unavailable or has exited, or `SendMessage` \
                 is not available",
        },
        "codex" => SubagentEditCapability {
            harness: "codex",
            tool: Some("followup_task"),
            delivery: None,
            worker_address: WorkerAddress::CodexFollowupTask,
            unavailable: "`followup_task` is unavailable or the exact worker is unavailable \
                 or has exited",
        },
        "opencode" => SubagentEditCapability {
            harness: "opencode",
            tool: None,
            delivery: None,
            worker_address: WorkerAddress::Unsupported,
            unavailable: "OpenCode documents no mid-run tool that delivers an edit to a \
                 running child. Continuing the same child session ID after the child returns \
                 is not in-flight delivery",
        },
        "cursor" => SubagentEditCapability {
            harness: "cursor",
            tool: None,
            delivery: None,
            worker_address: WorkerAddress::Unsupported,
            unavailable: "Cursor has no documented tool that delivers an operator edit to a \
                 running subagent",
        },
        _ => SubagentEditCapability {
            harness: "default",
            tool: None,
            delivery: None,
            worker_address: WorkerAddress::Unsupported,
            unavailable: "this harness has no documented tool that delivers an operator edit \
                 to a running subagent",
        },
    }
}

/// Forward-to-owner instruction a harness-neutral caller can render.
pub fn render_forward_to_owner(edit: &OperatorEditDelivery<'_>) -> String {
    let capability = subagent_edit_capability(edit.harness);
    let worker = bound_worker(edit.worker_id);
    match capability.worker_address {
        WorkerAddress::Unsupported => failed_handoff(edit, capability.unavailable),
        WorkerAddress::GrokSubagentId => match worker {
            Some(worker) => grok_call(edit, worker),
            None => failed_handoff(
                edit,
                "no worker id is bound, so `send_subagent_message` was not called \
                 (`subagent_id` is never `parent`)",
            ),
        },
        WorkerAddress::CodexFollowupTask => match worker {
            Some(worker) => codex_call(edit, worker),
            None => failed_handoff(
                edit,
                "Codex resumes a worker with `followup_task` and no worker id is bound. \
                 Codex documents no parameter name and no delivery mode for that resume",
            ),
        },
        WorkerAddress::ClaudeRunningSubagent => claude_text(edit, worker),
    }
}

fn canonical_harness(harness: &str) -> &'static str {
    match harness
        .trim()
        .to_ascii_lowercase()
        .replace(['_', ' '], "-")
        .as_str()
    {
        "grok" | "grok-build" => "grok",
        "claude" | "claude-code" => "claude",
        "codex" => "codex",
        "opencode" | "open-code" => "opencode",
        "cursor" | "cursor-agent" => "cursor",
        _ => "default",
    }
}

/// Empty and the literal `parent` cannot address a worker.
fn bound_worker(worker_id: Option<&str>) -> Option<&str> {
    worker_id
        .map(str::trim)
        .filter(|worker| !worker.is_empty() && *worker != "parent")
}

const NESTED_AUTH: &str = "if `verbatim` adds or keeps a nested line that says `push` or \
     `publish`, that line is operator authorization to perform that action now \
     (`#nestedoutwardauth`): it is not a question, not a chat reply, and not blocked by a \
     preset or skill that says to wait until asked. A trailing `⟦effect:N⟧` marker does not \
     remove it. `publish` on a review posts the review as a plain comment unless the line says \
     approve.";

const CLAUDE_EDITED: &str = "the operator EDITED a queue item a worker already claimed (`owner` \
     above; the claim followed the edit). FORWARD the change (`previous` -> `verbatim`) to that \
     owner now (for example SendMessage to the running subagent) so the work in flight picks it \
     up. If `verbatim` adds or keeps a nested line that says `push` or `publish`, tell the owner \
     that line is operator authorization to perform that action now (`#nestedoutwardauth`): it is \
     not a question, not a chat reply, and not blocked by a preset or skill that says to wait \
     until asked. A trailing `⟦effect:N⟧` marker does not remove it. `publish` on a review posts \
     the review as a plain comment unless the line says approve. Do NOT dispatch a new subagent \
     and do NOT re-claim the item.";

const CLAUDE_ADDED: &str = "already claimed by the worker named in `owner` (in flight there). \
     Nothing to dispatch; do NOT start a second subagent for it.";

fn failed_handoff(edit: &OperatorEditDelivery<'_>, reason: &str) -> String {
    format!(
        "{reason}. {HANDOFF_DID_NOT_HAPPEN}: do not drop this operator answer/edit, claim it \
         was forwarded, or keep routing only to the unreachable owner. Preserve `verbatim` \
         ({verbatim}) in the queue and run `{requeue}`. That generation-fenced transition \
         abandons only the stale attempt and makes the edited row redispatchable; let the \
         normal queue dispatcher start its replacement under a fresh generation.",
        verbatim = edit.verbatim,
        requeue = edit.requeue_command,
    )
}

fn grok_call(edit: &OperatorEditDelivery<'_>, worker: &str) -> String {
    let lead = match edit.change {
        EditHandoffChange::Added => {
            "the operator added a note on a queue item a worker already claimed (`owner` above)"
        }
        EditHandoffChange::Edited => {
            "the operator EDITED a queue item a worker already claimed (`owner` above; the claim \
             followed the edit)"
        }
    };
    let previous = edit.previous.unwrap_or("");
    let verbatim = edit.verbatim;
    let requeue = edit.requeue_command;
    format!(
        "{lead}. Call `send_subagent_message` now with `delivery` `steer`, `subagent_id` \
         `{worker}` (never `parent`), and `message` containing `previous` and `verbatim` so \
         the worker sees the edit. previous: {previous} verbatim: {verbatim}. That `message` \
         must also say: {NESTED_AUTH} Do NOT dispatch a new subagent and do NOT re-claim the \
         item. If `send_subagent_message` is not in the tool list when `active_agent_messages` \
         is off (`GROK_ACTIVE_AGENT_MESSAGES` or `[features] active_agent_messages`), or the \
         send is rejected, or the harness reports that the exact worker is unavailable or has \
         exited, {HANDOFF_DID_NOT_HAPPEN}: do not drop this operator answer/edit, claim it was \
         forwarded, or keep routing only to the unreachable owner. Preserve `verbatim` in the \
         queue and run `{requeue}`. That generation-fenced transition abandons only the stale \
         attempt and makes the edited row redispatchable; let the normal queue dispatcher start \
         its replacement under a fresh generation."
    )
}

fn codex_call(edit: &OperatorEditDelivery<'_>, worker: &str) -> String {
    let lead = match edit.change {
        EditHandoffChange::Added => {
            "the operator added a note on a queue item a worker already claimed (`owner` above)"
        }
        EditHandoffChange::Edited => {
            "the operator EDITED a queue item a worker already claimed (`owner` above; the claim \
             followed the edit). FORWARD the change (`previous` -> `verbatim`) to that owner"
        }
    };
    let previous = edit.previous.unwrap_or("");
    let verbatim = edit.verbatim;
    let requeue = edit.requeue_command;
    format!(
        "{lead}. Call `followup_task` for the steering worker `{worker}`. Codex documents no \
         parameter name for that worker and no delivery mode; pass the worker id the way \
         `followup_task` addresses a worker, with text containing previous: {previous} \
         verbatim: {verbatim}. `send_message` delivers a worker packet to the parent and is \
         not this edit. That text must also say: {NESTED_AUTH} Do NOT dispatch a new subagent \
         and do NOT re-claim the item. If `followup_task` is unavailable or the harness reports \
         that the exact worker is unavailable or has exited, {HANDOFF_DID_NOT_HAPPEN}: do not \
         drop this operator answer/edit, claim it was forwarded, or keep routing only to the \
         unreachable owner. Preserve `verbatim` in the queue and run `{requeue}`. That \
         generation-fenced transition abandons only the stale attempt and makes the edited row \
         redispatchable; let the normal queue dispatcher start its replacement under a fresh \
         generation."
    )
}

fn claude_text(edit: &OperatorEditDelivery<'_>, worker: Option<&str>) -> String {
    let body = match edit.change {
        EditHandoffChange::Added => CLAUDE_ADDED,
        EditHandoffChange::Edited => CLAUDE_EDITED,
    };
    let bound = worker
        .map(|worker| format!(" The bound worker id is `{worker}`."))
        .unwrap_or_default();
    let requeue = edit.requeue_command;
    format!(
        "{body}{bound} If the harness reports that the exact worker is unavailable or has \
         exited, {HANDOFF_DID_NOT_HAPPEN}: do not drop this \
         operator answer/edit, claim it was forwarded, or keep routing only to the unreachable \
         owner. Preserve `verbatim` in the queue and run `{requeue}`. That generation-fenced \
         transition abandons only the stale attempt and makes the edited row redispatchable; \
         let the normal queue dispatcher start its replacement under a fresh generation."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit<'a>(
        harness: &'a str,
        change: EditHandoffChange,
        worker_id: Option<&'a str>,
        previous: Option<&'a str>,
        verbatim: &'a str,
    ) -> OperatorEditDelivery<'a> {
        OperatorEditDelivery {
            harness,
            worker_id,
            previous,
            verbatim,
            change,
            requeue_command: "agent-doc queue release 'tasks/bugs.md' --item 'line' --generation 4 --requeue",
        }
    }

    fn assert_requeue(text: &str) {
        assert!(
            text.contains(
                "agent-doc queue release 'tasks/bugs.md' --item 'line' --generation 4 --requeue"
            ),
            "{text}"
        );
        assert!(text.contains(HANDOFF_DID_NOT_HAPPEN), "{text}");
    }

    #[test]
    fn grok_with_worker_calls_send_subagent_message_steer() {
        let previous = "previous wording";
        let verbatim = "verbatim wording with a nested publish";
        let worker = "worker-grok-42";
        for harness in ["grok", "grok-build", "Grok"] {
            for change in [EditHandoffChange::Edited, EditHandoffChange::Added] {
                let text = render_forward_to_owner(&edit(
                    harness,
                    change,
                    Some(worker),
                    Some(previous),
                    verbatim,
                ));
                assert!(text.contains("send_subagent_message"), "{harness}: {text}");
                assert!(
                    text.contains(&format!("`subagent_id` `{worker}`")),
                    "{harness}: {text}"
                );
                assert!(text.contains("`delivery` `steer`"), "{harness}: {text}");
                let tool_at = text.find("send_subagent_message").unwrap();
                let steer_at = text.find("`delivery` `steer`").unwrap();
                assert!(
                    steer_at > tool_at && steer_at - tool_at < 160,
                    "{harness}: {text}"
                );
                assert!(text.contains(previous), "{text}");
                assert!(text.contains(verbatim), "{text}");
                assert!(text.contains("#nestedoutwardauth"), "{text}");
                assert!(text.contains("not a chat reply"), "{text}");
                assert!(
                    text.contains(
                        "`send_subagent_message` is not in the tool list when `active_agent_messages` is off"
                    ),
                    "{text}"
                );
                assert!(text.contains("GROK_ACTIVE_AGENT_MESSAGES"), "{text}");
                assert!(text.contains("Do NOT dispatch a new subagent"), "{text}");
                assert!(text.contains("do NOT re-claim"), "{text}");
                assert!(!text.contains("`subagent_id` `parent`"), "{text}");
                assert!(!text.contains("SendMessage"), "{text}");
                assert!(!text.contains("followup_task"), "{text}");
                assert_requeue(&text);
                assert_eq!(subagent_edit_capability(harness).delivery, Some("steer"));
            }
        }
    }

    #[test]
    fn grok_without_worker_does_not_hand_off() {
        for worker in [None, Some(""), Some("   "), Some("parent")] {
            let text = render_forward_to_owner(&edit(
                "grok",
                EditHandoffChange::Edited,
                worker,
                Some("previous wording"),
                "verbatim wording",
            ));
            assert!(text.contains(HANDOFF_DID_NOT_HAPPEN), "{text}");
            assert!(!text.contains("SendMessage"), "{text}");
            assert!(!text.contains("`subagent_id` `parent`"), "{text}");
            assert!(!text.contains("Call `send_subagent_message`"), "{text}");
            assert!(text.contains("verbatim wording"), "{text}");
            assert_requeue(&text);
        }
    }

    #[test]
    fn claude_edited_keeps_send_message() {
        let text = render_forward_to_owner(&edit(
            "claude-code",
            EditHandoffChange::Edited,
            Some("worker-claude"),
            Some("old"),
            "new line",
        ));
        assert!(text.contains("FORWARD the change"), "{text}");
        assert!(text.contains("SendMessage"), "{text}");
        assert!(text.contains("#nestedoutwardauth"), "{text}");
        assert!(text.contains("not a chat reply"), "{text}");
        assert!(text.contains("`worker-claude`"), "{text}");
        assert!(!text.contains("send_subagent_message"), "{text}");
        assert!(!text.contains("`delivery` `steer`"), "{text}");
        assert!(!text.contains("followup_task"), "{text}");
        assert_requeue(&text);
        assert_eq!(
            subagent_edit_capability("Claude Code").tool,
            Some("SendMessage")
        );
        assert_eq!(subagent_edit_capability("claude").delivery, None);
    }

    #[test]
    fn claude_added_does_not_dispatch() {
        let text = render_forward_to_owner(&edit(
            "claude",
            EditHandoffChange::Added,
            Some("worker-claude"),
            None,
            "added note",
        ));
        assert!(text.contains("Nothing to dispatch"), "{text}");
        assert!(!text.contains("SendMessage"), "{text}");
        assert!(!text.contains("send_subagent_message"), "{text}");
        assert_requeue(&text);
    }

    #[test]
    fn codex_uses_followup_task() {
        let text = render_forward_to_owner(&edit(
            "codex",
            EditHandoffChange::Edited,
            Some("worker-codex"),
            Some("previous wording"),
            "verbatim wording",
        ));
        assert!(text.contains("followup_task"), "{text}");
        assert!(text.contains("`worker-codex`"), "{text}");
        assert!(text.contains("FORWARD the change"), "{text}");
        assert!(text.contains("previous wording"), "{text}");
        assert!(text.contains("verbatim wording"), "{text}");
        assert!(text.contains("#nestedoutwardauth"), "{text}");
        assert!(text.contains("send_message"), "{text}");
        assert!(!text.contains("SendMessage"), "{text}");
        assert!(!text.contains("send_subagent_message"), "{text}");
        assert!(!text.contains("`delivery` `steer`"), "{text}");
        assert_eq!(subagent_edit_capability("codex").delivery, None);
        assert_requeue(&text);

        let added = render_forward_to_owner(&edit(
            "codex",
            EditHandoffChange::Added,
            Some("worker-codex"),
            None,
            "added note",
        ));
        assert!(added.contains("followup_task"), "{added}");
        assert!(added.contains("added note"), "{added}");
        assert!(!added.contains("SendMessage"), "{added}");
    }

    #[test]
    fn codex_without_worker_does_not_hand_off() {
        let text = render_forward_to_owner(&edit(
            "codex",
            EditHandoffChange::Edited,
            None,
            Some("previous wording"),
            "verbatim wording",
        ));
        assert!(text.contains(HANDOFF_DID_NOT_HAPPEN), "{text}");
        assert!(!text.contains("Call `followup_task`"), "{text}");
        assert!(!text.contains("SendMessage"), "{text}");
        assert!(!text.contains("send_subagent_message"), "{text}");
        assert_requeue(&text);
    }

    #[test]
    fn opencode_and_unknown_are_unsupported() {
        for harness in ["opencode", "open-code", "default", "cursor", ""] {
            let text = render_forward_to_owner(&edit(
                harness,
                EditHandoffChange::Edited,
                Some("worker-1"),
                Some("previous wording"),
                "verbatim wording",
            ));
            assert!(text.contains(HANDOFF_DID_NOT_HAPPEN), "{harness}: {text}");
            assert!(text.contains("verbatim wording"), "{harness}: {text}");
            assert!(!text.contains("SendMessage"), "{harness}: {text}");
            assert!(!text.contains("send_subagent_message"), "{harness}: {text}");
            assert!(!text.contains("followup_task"), "{harness}: {text}");
            assert!(!text.contains("`delivery` `steer`"), "{harness}: {text}");
            assert_eq!(subagent_edit_capability(harness).tool, None, "{harness}");
            assert_requeue(&text);
        }
        assert!(
            render_forward_to_owner(&edit(
                "opencode",
                EditHandoffChange::Edited,
                Some("worker-1"),
                None,
                "verbatim wording",
            ))
            .contains("same child session ID")
        );
    }
}
