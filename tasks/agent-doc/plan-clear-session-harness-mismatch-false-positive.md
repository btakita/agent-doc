# Plan: Clear Session Context harness mismatch false positive

## Problem

JB `Clear Session Context` falsely refused with "drafted prompt input" when the
document's frontmatter says `agent: codex` but the pane actually runs a different
harness (e.g. OpenCode). The Codex-specific draft detection in
`protected_prompt_input_reason` was applied to non-Codex pane output, producing a
false positive.

## Root Cause

`protected_clear_input_reason` in `session_actor_cmd.rs` always created the
`HarnessConfig` from `ctx.harness` (the document's configured agent). When the pane
runs a different agent, the wrong harness's prompt heuristics are applied:

1. `live_pane_prompt_ready` doesn't recognize the other harness's idle state → `AliveBusy`
2. `protected_prompt_input_reason` applies Codex-specific draft detection to
   non-Codex TUI output → false positive "drafted prompt input"

## Fix

Added `HarnessConfig::from_pane_command(cmd)` that infers the harness from the pane's
`pane_current_command` when the command is an unambiguous binary (`codex`, `opencode`,
`claude`, `bun`). Returns `None` for ambiguous commands (`node`, `agent-doc`, shell).

Updated `protected_clear_input_reason` to prefer the inferred harness over the
document's configured harness, falling back to `from_agent_name(&ctx.harness)` when
the command is ambiguous.

## Files Changed

- `src/agent-doc/src/harness.rs` — added `from_pane_command()` + tests
- `src/agent-doc/src/session_actor_cmd.rs` — updated `protected_clear_input_reason`
- `src/agent-doc/specs/07-session-tmux-commands.md` — spec updated
