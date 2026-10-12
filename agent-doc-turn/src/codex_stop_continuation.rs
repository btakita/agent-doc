//! Pure Codex Stop-hook continuation instruction rendering.
//!
//! Codex renders every Stop continuation `reason` verbatim in the operator's
//! pane under its `Blocked by hook` label, so these strings are operator-visible.
//! They carry only what the agent must do next (`#codexstopterse`): answer in
//! this turn, persist through the binary, never re-invoke the owner pane, and
//! hold the final answer. Diagnostics the agent cannot act on — such as a
//! suppressed background context reset and its accretion/ctx% reason — belong
//! in `.agent-doc/logs/ops.log` (`codex_background_context_clear_suppressed`),
//! never in this payload. When queue auto-clear hands the item boundary to the
//! supervisor (`#bgclearpolicy`), the hook allows the stop instead of rendering
//! any continuation (`codex_stop_queue_continuation_yield` in ops.log).

/// Upper bound for the fixed (path- and prompt-independent) part of a prompt
/// continuation instruction. A regression past this budget means operator-pane
/// noise crept back into the Stop payload (`#codexstopterse`).
pub const PROMPT_CONTINUATION_INSTRUCTION_BUDGET: usize = 320;

pub fn render_prompt_continuation_instruction(display_path: &str, mcp_configured: bool) -> String {
    if mcp_configured {
        format!(
            "Continue THIS turn in-pane via the configured `agent-doc` MCP server (`agent_doc_admit` → `agent_doc_plan` → `agent_doc_finalize` → `agent_doc_session_check`), or `agent-doc finalize {display_path}` if the MCP tools are unavailable. Do NOT run `agent-doc {display_path}` (recursive self-invocation) and do not send the final answer yet."
        )
    } else {
        format!(
            "Continue THIS turn in-pane: answer it, then persist with `agent-doc finalize {display_path}`. Do NOT run `agent-doc {display_path}` (recursive self-invocation) and do not send the final answer yet."
        )
    }
}

pub fn render_slash_command_continuation_instruction(display_path: &str, command: &str) -> String {
    format!(
        "Do NOT answer the queued slash command {command:?} as an agent-doc prompt. Let the current turn close so the managed owner-pane supervisor can submit {command:?} at the next idle prompt, mark that queue head complete, and continue the remaining queue. Do not send the final answer yet. If no managed supervisor is available, submit {command:?} in the owner pane, then run `agent-doc queue consume {display_path}` and `agent-doc commit {display_path}` before continuing."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "docs/session.md";

    #[test]
    fn prompt_instruction_without_mcp_matches_codex_stop_guidance() {
        assert_eq!(
            render_prompt_continuation_instruction(DOC, false),
            "Continue THIS turn in-pane: answer it, then persist with `agent-doc finalize docs/session.md`. Do NOT run `agent-doc docs/session.md` (recursive self-invocation) and do not send the final answer yet."
        );
    }

    #[test]
    fn prompt_instruction_prefers_configured_mcp_tools() {
        assert_eq!(
            render_prompt_continuation_instruction(DOC, true),
            "Continue THIS turn in-pane via the configured `agent-doc` MCP server (`agent_doc_admit` → `agent_doc_plan` → `agent_doc_finalize` → `agent_doc_session_check`), or `agent-doc finalize docs/session.md` if the MCP tools are unavailable. Do NOT run `agent-doc docs/session.md` (recursive self-invocation) and do not send the final answer yet."
        );
    }

    /// `#codexstopterse`: the operator sees this payload in the Codex pane, so
    /// the path-independent instruction text must stay within a fixed budget.
    #[test]
    fn prompt_instruction_stays_within_operator_pane_budget() {
        for mcp in [false, true] {
            let rendered = render_prompt_continuation_instruction("", mcp);
            assert!(
                rendered.chars().count() <= PROMPT_CONTINUATION_INSTRUCTION_BUDGET,
                "mcp={mcp}: {} chars > {PROMPT_CONTINUATION_INSTRUCTION_BUDGET}: {rendered}",
                rendered.chars().count()
            );
            assert!(!rendered.contains("context"), "{rendered}");
        }
    }

    #[test]
    fn slash_command_instruction_keeps_queue_head_out_of_agent_doc_prompt_flow() {
        assert_eq!(
            render_slash_command_continuation_instruction(DOC, "/clear"),
            "Do NOT answer the queued slash command \"/clear\" as an agent-doc prompt. Let the current turn close so the managed owner-pane supervisor can submit \"/clear\" at the next idle prompt, mark that queue head complete, and continue the remaining queue. Do not send the final answer yet. If no managed supervisor is available, submit \"/clear\" in the owner pane, then run `agent-doc queue consume docs/session.md` and `agent-doc commit docs/session.md` before continuing."
        );
    }
}
