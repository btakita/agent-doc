//! Pure owner-pane self-invocation contract construction.
//!
//! Orchestration gathers tmux/session evidence. This module owns the
//! side-effect-free JSON contract emitted to preflight consumers when a Codex
//! owner pane re-invokes agent-doc while unresolved work is still live.

use serde::{Deserialize, Serialize};

const WORK_EXCERPT_CHARS: usize = 200;

/// Structured owner-pane self-invocation contract.
///
/// Emitted when a Codex owner-pane re-invocation has unresolved exchange work
/// that must be handled in the current owner turn instead of dispatched to a
/// nested child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedPaneSelfInvocation {
    pub file: String,
    pub current_pane: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_state: Option<String>,
    /// `"unresolved_prompt"` or `"active_queue_head"`.
    pub kind: String,
    /// First non-empty line of the unresolved work, truncated.
    pub work_excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<String>,
    /// The exact persistence command to run after composing the in-pane response.
    pub persistence_command: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnedPaneSelfInvocationOptions {
    pub suppress_active_queue_head: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedPaneSelfInvocationKind {
    UnresolvedPrompt,
    ActiveQueueHead,
}

impl OwnedPaneSelfInvocationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnresolvedPrompt => "unresolved_prompt",
            Self::ActiveQueueHead => "active_queue_head",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OwnedPaneSelfInvocationInput<'a> {
    pub file: &'a str,
    pub current_pane: &'a str,
    pub session_id: &'a str,
    pub actor_generation: Option<u64>,
    pub actor_state: Option<&'a str>,
    pub kind: OwnedPaneSelfInvocationKind,
    pub work: &'a str,
    pub head_id: Option<&'a str>,
}

pub fn build_owned_pane_self_invocation(
    input: OwnedPaneSelfInvocationInput<'_>,
) -> OwnedPaneSelfInvocation {
    OwnedPaneSelfInvocation {
        file: input.file.to_string(),
        current_pane: input.current_pane.to_string(),
        session_id: input.session_id.to_string(),
        actor_generation: input.actor_generation,
        actor_state: input.actor_state.map(str::to_string),
        kind: input.kind.as_str().to_string(),
        work_excerpt: first_nonempty_excerpt(input.work, WORK_EXCERPT_CHARS),
        head_id: input.head_id.map(str::to_string),
        persistence_command: persistence_command(input.file),
    }
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

fn persistence_command(file: &str) -> String {
    format!("agent-doc finalize {file} (or agent-doc write --commit {file})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_pane_contract_uses_first_nonempty_excerpt_and_persistence_command() {
        let contract = build_owned_pane_self_invocation(OwnedPaneSelfInvocationInput {
            file: "tasks/x.md",
            current_pane: "%9",
            session_id: "sess-123",
            actor_generation: Some(7),
            actor_state: Some("alive-busy"),
            kind: OwnedPaneSelfInvocationKind::UnresolvedPrompt,
            work: "\n\n  Please handle #abc\nmore detail",
            head_id: None,
        });

        assert_eq!(contract.file, "tasks/x.md");
        assert_eq!(contract.current_pane, "%9");
        assert_eq!(contract.session_id, "sess-123");
        assert_eq!(contract.actor_generation, Some(7));
        assert_eq!(contract.actor_state.as_deref(), Some("alive-busy"));
        assert_eq!(contract.kind, "unresolved_prompt");
        assert_eq!(contract.work_excerpt, "Please handle #abc");
        assert_eq!(contract.head_id, None);
        assert_eq!(
            contract.persistence_command,
            "agent-doc finalize tasks/x.md (or agent-doc write --commit tasks/x.md)"
        );
    }

    #[test]
    fn owner_pane_contract_truncates_queue_head_excerpt() {
        let long_head = "x".repeat(250);
        let contract = build_owned_pane_self_invocation(OwnedPaneSelfInvocationInput {
            file: "tasks/x.md",
            current_pane: "%9",
            session_id: "sess-123",
            actor_generation: None,
            actor_state: None,
            kind: OwnedPaneSelfInvocationKind::ActiveQueueHead,
            work: &long_head,
            head_id: Some("abc"),
        });

        assert_eq!(contract.kind, "active_queue_head");
        assert_eq!(contract.work_excerpt.len(), WORK_EXCERPT_CHARS);
        assert_eq!(contract.head_id.as_deref(), Some("abc"));
    }
}
