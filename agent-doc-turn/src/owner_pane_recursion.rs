//! Pure owner-pane self-invocation diagnostics.
//!
//! Orchestration detects tmux/session ownership and performs recovery effects.
//! This module owns the operator-facing turn guidance when a managed owner pane
//! tries to recursively invoke `agent-doc` for the document it already owns.

use serde::{Deserialize, Serialize};

/// Consecutive same-head self-invocation guard fires that prove a dead-loop.
/// Two transient self-invokes are tolerated; the third halts.
pub const OWNER_PANE_WEDGE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerPaneWedgeRecord {
    /// The queue head (or self-invocation detail) the count is keyed on. A new
    /// head means the loop advanced, so the counter resets.
    pub head: String,
    /// Consecutive self-invocation guard fires for `head`.
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerPaneQueueHead<'a> {
    pub prompt: &'a str,
    pub id: Option<&'a str>,
}

fn first_nonempty_excerpt(text: &str, max: usize) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .chars()
        .take(max)
        .collect()
}

fn queue_head_excerpt(head: OwnerPaneQueueHead<'_>) -> String {
    first_nonempty_excerpt(head.prompt, 200)
}

fn queue_head_id_note(head: OwnerPaneQueueHead<'_>) -> String {
    head.id.map(|id| format!(" (id #{id})")).unwrap_or_default()
}

pub fn record_owner_pane_wedge_fire(
    prior: Option<&OwnerPaneWedgeRecord>,
    head: &str,
) -> OwnerPaneWedgeRecord {
    let count = match prior {
        Some(record) if record.head == head => record.count.saturating_add(1),
        _ => 1,
    };
    OwnerPaneWedgeRecord {
        head: head.to_string(),
        count,
    }
}

pub fn owner_pane_wedge_threshold_reached(count: u32) -> bool {
    count >= OWNER_PANE_WEDGE_THRESHOLD
}

pub fn recursive_direct_invocation_message(document: &str, detail: &str) -> String {
    format!(
        "recursive direct invocation would deadlock: `agent-doc {document}` is running inside the Codex pane that already owns this document ({detail}). The empty preflight cycle has been abandoned (terminal - `session-check` accepts it, no manual `agent-doc cancel` needed). If the pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {document}` (or `agent-doc session clear {document}`) - idle pane evidence repairs a stale busy actor back to ready; otherwise retry from outside the managed pane or restart the owner with `agent-doc start {document}`."
    )
}

pub fn recursive_start_invocation_message(document: &str, detail: &str) -> String {
    format!(
        "recursive self-owned-pane start would deadlock: `agent-doc start {document}` was run inside the Codex pane that already owns this document ({detail}). Spawning a replacement owner here would loop re-injecting `agent-doc {document}` into this same pane. Recover from a DIFFERENT pane: first reconcile a possibly stale-busy actor without killing the pane via `agent-doc session status {document}`, then if the pane really is wedged run `agent-doc session interrupt-clear {document}` to interrupt the owner and clear the session; if that cannot settle, run `agent-doc session interrupt-clear {document} --force` to kill the owner pane/supervisor and clear the registry in one command. Do NOT re-run `agent-doc start {document}` from this pane - it only re-trips this guard."
    )
}

pub fn prompt_miss_message(document: &str, detail: &str, unresolved: &str) -> String {
    let excerpt = first_nonempty_excerpt(unresolved, 200);
    format!(
        "owned-pane self-invocation with unresolved exchange prompt: `agent-doc {document}` was run inside the Codex pane that already owns this document ({detail}), but a user prompt is still unanswered: \"{excerpt}\". The recursive same-pane guard refuses to dispatch a nested child here, so this request would be a no-op for that prompt. No pre-commit, snapshot, or queue mutation was made - the prompt stays executable. Recovery: answer the prompt in THIS owner pane's current turn, then persist with `agent-doc finalize {document}` (or `agent-doc write --commit {document}`). Do NOT re-run `agent-doc {document}` from this same pane; that only re-triggers this guard."
    )
}

pub fn queue_handoff_message(document: &str, detail: &str, head: OwnerPaneQueueHead<'_>) -> String {
    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(head.prompt) {
        return format!(
            "owned-pane self-invocation with active auto-queue slash command: `agent-doc {document}` was run inside the Codex pane that already owns this document ({detail}), and the ready queue head is the literal slash command {command:?}. The recursive same-pane guard refuses to answer slash commands as agent-doc work. No pre-commit, snapshot, exchange, or queue mutation was made - the command stays live. Recovery: let the current turn stop; the managed owner-pane supervisor will submit {command:?} at the next idle prompt and consume the queue head. Do NOT answer this queue head in the exchange, and do NOT re-run `agent-doc {document}` from this same pane."
        );
    }
    let head_excerpt = queue_head_excerpt(head);
    let id_note = queue_head_id_note(head);
    format!(
        "owned-pane self-invocation with active auto-queue head: `agent-doc {document}` was run inside the Codex pane that already owns this document ({detail}), and a ready queue head is still live: \"{head_excerpt}\"{id_note}. The recursive same-pane guard refuses to dispatch a nested child here, so this request would baseline queue/boundary drift and leave the head unprocessed. No pre-commit, snapshot, or queue mutation was made - the head stays live. Recovery: run the queue head in THIS owner pane's current turn, then persist with `agent-doc finalize {document}` (or `agent-doc write --commit {document}`) so the head is consumed and the next queue prompt is exposed. Do NOT re-run `agent-doc {document}` from this same pane; that only re-triggers the recursive guard."
    )
}

pub fn queue_wedge_halt_message(
    document: &str,
    detail: &str,
    head: OwnerPaneQueueHead<'_>,
    count: u32,
) -> String {
    let head_excerpt = queue_head_excerpt(head);
    let id_note = queue_head_id_note(head);
    format!(
        "owned-pane self-invocation WEDGE: `agent-doc {document}` has re-entered the Codex pane that already owns this document ({detail}) {count} times in a row for the same live queue head \"{head_excerpt}\"{id_note} without it advancing - a self-driving `agent:queue auto` dead-loop. The auto-queue has been HALTED (`queue: stop`) so it stops re-firing. The head was NOT lost (it stays live) and no snapshot/queue drift was committed. Recovery: either (a) answer this head in the current owner turn and persist with `agent-doc finalize {document}`, then re-enable with `queue: go`; or (b) re-establish a clean owner with `agent-doc start {document}` and trigger the queue from OUTSIDE this pane. Do NOT re-run `agent-doc {document}` from this same pane - that is exactly the re-entry that wedged the loop."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETAIL: &str =
        "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9";

    #[test]
    fn wedge_counter_counts_consecutive_same_head_and_reaches_threshold() {
        let first = record_owner_pane_wedge_fire(None, "do [#alpha]");
        assert_eq!(first.count, 1);
        assert!(!owner_pane_wedge_threshold_reached(first.count));

        let second = record_owner_pane_wedge_fire(Some(&first), "do [#alpha]");
        assert_eq!(second.count, 2);
        assert!(!owner_pane_wedge_threshold_reached(second.count));

        let third = record_owner_pane_wedge_fire(Some(&second), "do [#alpha]");
        assert_eq!(third.count, OWNER_PANE_WEDGE_THRESHOLD);
        assert!(owner_pane_wedge_threshold_reached(third.count));
    }

    #[test]
    fn wedge_counter_resets_when_head_advances() {
        let first = record_owner_pane_wedge_fire(None, "do [#alpha]");
        let second = record_owner_pane_wedge_fire(Some(&first), "do [#alpha]");
        assert_eq!(second.count, 2);

        let advanced = record_owner_pane_wedge_fire(Some(&second), "do [#beta]");
        assert_eq!(advanced.head, "do [#beta]");
        assert_eq!(advanced.count, 1);
        assert!(!owner_pane_wedge_threshold_reached(advanced.count));
    }

    #[test]
    fn recursive_direct_message_names_idle_reconcile_recovery() {
        let msg = recursive_direct_invocation_message("tasks/x.md", DETAIL);
        assert!(msg.contains("recursive direct invocation would deadlock"));
        assert!(msg.contains("agent-doc tasks/x.md"));
        assert!(msg.contains("empty preflight cycle has been abandoned"));
        assert!(msg.contains("agent-doc session status tasks/x.md"));
        assert!(msg.contains("agent-doc session clear tasks/x.md"));
        assert!(msg.contains("without killing the pane"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
    }

    #[test]
    fn recursive_start_message_refuses_and_names_out_of_pane_recovery() {
        let msg = recursive_start_invocation_message("tasks/x.md", DETAIL);
        assert!(msg.contains("recursive self-owned-pane start would deadlock"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("loop re-injecting `agent-doc tasks/x.md`"));
        assert!(msg.contains("DIFFERENT pane"));
        assert!(msg.contains("agent-doc session status tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md --force"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn prompt_miss_message_names_prompt_and_in_owner_recovery() {
        let msg = prompt_miss_message("tasks/x.md", DETAIL, "\n\nPlease handle #abc\nmore");
        assert!(msg.contains("unresolved exchange prompt"));
        assert!(msg.contains("Please handle #abc"));
        assert!(msg.contains("prompt stays executable"));
        assert!(msg.contains("THIS owner pane"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("agent-doc write --commit tasks/x.md"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn queue_handoff_message_names_head_and_recovery() {
        let head = OwnerPaneQueueHead {
            prompt: "do [#codex-owned-pane-auto-queue-stuck]",
            id: Some("codex-owned-pane-auto-queue-stuck"),
        };
        let msg = queue_handoff_message("tasks/x.md", DETAIL, head);
        assert!(msg.contains("active auto-queue head"));
        assert!(msg.contains("do [#codex-owned-pane-auto-queue-stuck]"));
        assert!(msg.contains("(id #codex-owned-pane-auto-queue-stuck)"));
        assert!(msg.contains("THIS owner pane"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("Do NOT re-run"));
        assert!(msg.contains("No pre-commit, snapshot, or queue mutation was made"));
    }

    #[test]
    fn queue_handoff_message_uses_supervisor_for_slash_command() {
        let head = OwnerPaneQueueHead {
            prompt: "  /clear  ",
            id: None,
        };
        let msg = queue_handoff_message("tasks/x.md", DETAIL, head);
        assert!(msg.contains("slash command"));
        assert!(msg.contains("\"/clear\""));
        assert!(msg.contains("managed owner-pane supervisor will submit"));
        assert!(msg.contains("No pre-commit, snapshot, exchange, or queue mutation was made"));
        assert!(msg.contains("Do NOT answer this queue head in the exchange"));
        assert!(
            !msg.contains("agent-doc finalize"),
            "slash-command handoff must not instruct an assistant closeout: {msg}"
        );
    }

    #[test]
    fn queue_wedge_halt_message_names_halt_and_both_recoveries() {
        let head = OwnerPaneQueueHead {
            prompt: "do [#recguard-wedge-escape]",
            id: Some("recguard-wedge-escape"),
        };
        let msg = queue_wedge_halt_message("tasks/x.md", DETAIL, head, 3);
        assert!(msg.contains("WEDGE"));
        assert!(msg.contains("HALTED (`queue: stop`)"));
        assert!(msg.contains("do [#recguard-wedge-escape]"));
        assert!(msg.contains("(id #recguard-wedge-escape)"));
        assert!(msg.contains("stays live"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("queue: go"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("OUTSIDE this pane"));
        assert!(msg.contains("Do NOT re-run"));
    }
}
