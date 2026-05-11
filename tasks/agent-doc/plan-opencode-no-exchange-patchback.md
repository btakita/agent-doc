# Plan: OpenCode Response Not Written to Exchange (#noexchopencode2)

## Bug

OpenCode agent response output appears in CLI but is never patched back into the session document `agent:exchange` block. Reproduction: `boost-client/tasks/monsterrodholders.md` session a9421282, GLM-5.1 agent — sidebar revert assessment response appeared in CLI output but was never patched back. User had to manually request "Write the response into the document."

Same class as #nopatchbackopencode, #rspcmt4, #rspcmt5.

## Root Cause

The root cause spans two layers:

### Layer 1: Instruction Surface (Partially Fixed in #rspcmt4)

The #rspcmt4 fix added `session-check` requirements to the OpenCode section of `harness-invocation.md` and made error messages harness-agnostic. However, the instruction surface does not explicitly call out the **anti-pattern** of outputting a response to CLI without piping it through `agent-doc finalize`. The OpenCode section describes the correct flow but does not name the specific failure mode (response-in-CLI-only).

### Layer 2: No Programmatic Backstop

Unlike Codex (which has `codex_hook.rs` Stop handler for auto-recovery), OpenCode has:
- No hook backstop (`opencode_hook.rs` does not exist)
- No active session tracking (`.agent-doc/codex-hooks/sessions/` is Codex-only)
- `detect_active_session_post_commit_drift()` returns `None` for OpenCode
- The OpenCode agent backend is non-streaming only (`opencode.rs` — no real-time write-back)

The `check_prompt_only_exchange_tail_guard()` in `session_check.rs` would catch this *after the fact*, but only if the agent or a downstream consumer runs `session-check`. When the agent skips finalize entirely, session-check is never invoked.

## Fix Approach

### 1. Instruction Surface: Name the Anti-Pattern

Add explicit anti-pattern warnings to the OpenCode section of `harness-invocation.md`:
- "Do **not** output the response to the CLI without piping it through `agent-doc finalize`"
- "Response text visible in console but absent from the session document is the same closeout violation as skipping finalize"

### 2. Regression Test

Add a `session_check.rs` test proving the OpenCode scenario is caught:
- `prompt_only_exchange_tail_catches_opencode_no_patchback()` — simulates an OpenCode agent that completed work (committed cycle) but left a prompt-only exchange tail, with the `OPENCODE` env var set to prove harness-agnostic detection

### 3. Skill Content Alignment

Add a stronger mandate in the harness-agnostic "Hot Path Digest" section of `SKILL.md`:
- Reinforce that "outputting a response to CLI without finalize is the same as not responding at all"

## Files Changed

| File | Change |
|------|--------|
| `runbooks/harness-invocation.md` | Add anti-pattern warnings to OpenCode section |
| `SKILL.md` | Strengthen Hot Path Digest with no-CLI-only-output mandate |
| `session_check.rs` | Add regression test for OpenCode no-patchback scenario |
| `skill.rs` | Add test proving OpenCode runbook section names the anti-pattern |
| `VERSIONS.md` | Add #noexchopencode2 entry |

## Out of Scope

- OpenCode hook backstop (requires OpenCode to expose lifecycle events)
- OpenCode active session tracking (requires env vars or state files)
- OpenCode streaming backend (requires OpenCode CLI streaming support)
