//! Pure Codex Stop-hook continuation instruction rendering.

pub fn render_prompt_continuation_instruction(
    display_path: &str,
    mcp_configured: bool,
    context_reset_reason: Option<&str>,
) -> String {
    if let Some(reason) = context_reset_reason {
        return format!(
            "Continue THIS turn in-pane: answer that prompt in {display_path} and persist with `agent-doc finalize {display_path}` (or `agent-doc write --commit {display_path}`). Do NOT run `agent-doc {display_path}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet. A background context reset was requested because {reason}, but automatic context clearing is disabled; only an explicit operator clear may reset the session."
        );
    }

    if mcp_configured {
        format!(
            "Continue THIS turn in-pane via the configured `agent-doc` MCP server: call `agent_doc_admit` for {display_path}, use `agent_doc_plan` / `agent_doc_read` as needed, answer that prompt in the response passed to `agent_doc_finalize`, and verify the result with `agent_doc_session_check`. If the MCP tools are unavailable in this Codex run, answer that prompt in {display_path} and persist with `agent-doc finalize {display_path}` (or `agent-doc write --commit {display_path}`). Do NOT run `agent-doc {display_path}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet."
        )
    } else {
        format!(
            "Continue THIS turn in-pane: answer that prompt in {display_path} and persist with `agent-doc finalize {display_path}` (or `agent-doc write --commit {display_path}`). Do NOT run `agent-doc {display_path}` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet."
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
            render_prompt_continuation_instruction(DOC, false, None),
            "Continue THIS turn in-pane: answer that prompt in docs/session.md and persist with `agent-doc finalize docs/session.md` (or `agent-doc write --commit docs/session.md`). Do NOT run `agent-doc docs/session.md` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet."
        );
    }

    #[test]
    fn prompt_instruction_prefers_configured_mcp_tools() {
        assert_eq!(
            render_prompt_continuation_instruction(DOC, true, None),
            "Continue THIS turn in-pane via the configured `agent-doc` MCP server: call `agent_doc_admit` for docs/session.md, use `agent_doc_plan` / `agent_doc_read` as needed, answer that prompt in the response passed to `agent_doc_finalize`, and verify the result with `agent_doc_session_check`. If the MCP tools are unavailable in this Codex run, answer that prompt in docs/session.md and persist with `agent-doc finalize docs/session.md` (or `agent-doc write --commit docs/session.md`). Do NOT run `agent-doc docs/session.md` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet."
        );
    }

    #[test]
    fn context_reset_reason_overrides_mcp_guidance() {
        assert_eq!(
            render_prompt_continuation_instruction(
                DOC,
                true,
                Some("transcript context 91.0% >= clear threshold 90% (#clearcodex)")
            ),
            "Continue THIS turn in-pane: answer that prompt in docs/session.md and persist with `agent-doc finalize docs/session.md` (or `agent-doc write --commit docs/session.md`). Do NOT run `agent-doc docs/session.md` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and do not send the final answer yet. A background context reset was requested because transcript context 91.0% >= clear threshold 90% (#clearcodex), but automatic context clearing is disabled; only an explicit operator clear may reset the session."
        );
    }

    #[test]
    fn slash_command_instruction_keeps_queue_head_out_of_agent_doc_prompt_flow() {
        assert_eq!(
            render_slash_command_continuation_instruction(DOC, "/clear"),
            "Do NOT answer the queued slash command \"/clear\" as an agent-doc prompt. Let the current turn close so the managed owner-pane supervisor can submit \"/clear\" at the next idle prompt, mark that queue head complete, and continue the remaining queue. Do not send the final answer yet. If no managed supervisor is available, submit \"/clear\" in the owner pane, then run `agent-doc queue consume docs/session.md` and `agent-doc commit docs/session.md` before continuing."
        );
    }
}
