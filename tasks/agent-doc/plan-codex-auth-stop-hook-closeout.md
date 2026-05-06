# Plan — Codex auth/tool-only stop interrupts `agent-doc` closeout

**Pending:** `#257p`
**Status:** Draft

## Problem

In Codex-managed `agent-doc` turns, an MCP authentication step can end the turn
before the assistant emits the final closeout body and before the skill reaches
`agent-doc finalize`.

Concrete observed shape:

1. User enters a normal `agent-doc <FILE>` turn.
2. The agent reaches an MCP tool that requires interactive auth (for example a
   browser OAuth flow / `authenticate` tool call).
3. Codex stops the turn after the tool step, with the document cycle still
   open and no replayable `last_assistant_message`.
4. The session document remains unresolved unless the operator notices the
   failed closeout and repairs it manually.

The existing Stop hook already blocks transcript-shaped replay payloads and can
auto-close replayable assistant responses, but the empty-message auth/tool-only
shape still needs better detection, diagnostics, and instruction-level guidance.

## Goal

Make auth-interrupted `agent-doc` turns fail closed with explicit recovery
guidance instead of looking like an unexplained missed finalize:

- treat MCP auth / OAuth steps as sub-steps of the same managed turn
- keep the shared instruction surface explicit that the turn still must end with
  `agent-doc finalize` / `write --commit` and `session-check`
- when Codex Stop fires with an open cycle and no `last_assistant_message`,
  capture a durable diagnostic record that points to the tracked prompt and
  explains the likely tool-only/auth interruption

## Implementation Steps

- [ ] Update `codex_hook.rs` so empty-message open-cycle Stop events save a
      blocked-stop diagnostic record instead of only emitting a generic
      "nothing to capture" note.
- [ ] Include the tracked prompt in that diagnostic so the interrupted
      `agent-doc <FILE>` request can be reconstructed easily.
- [ ] Make the Stop-hook block text mention the common tool-only /
      authentication-step shape explicitly, including MCP OAuth / authenticate
      flows.
- [ ] Add regression coverage for the empty-message Stop-hook path.
- [ ] Update the bundled skill/runbook/spec text so MCP auth is described as a
      sub-step that still must finish with `finalize` / `session-check`.

## Files Likely To Change

- `src/agent-doc/src/codex_hook.rs`
- `src/agent-doc/tests/codex_hook_integration.rs` and/or `src/agent-doc/src/codex_hook.rs` tests
- `src/agent-doc/SKILL.md`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/specs/09-git-integration.md`
- `src/agent-doc/SPEC.md`

## Exit Criteria

- An open-cycle Codex Stop with no `last_assistant_message` leaves a durable
  diagnostic artifact and an explicit recovery explanation instead of only a
  generic block.
- The shared instruction/spec surface says that MCP auth / OAuth is not a turn
  boundary and the same `agent-doc` cycle must still close through
  `finalize` / `write --commit` plus `session-check`.
- Regression coverage proves the empty-message Stop-hook path stays
  fail-closed.
