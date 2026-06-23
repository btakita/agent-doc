# Plan: direct-chat owned-pane response loss recurrence

## Problem

On 2026-06-23, a Codex turn answered a prompt from
`src/boost-client/tasks/monsterrodholders.md` in chat, but the answer did not
land in the session document.

The live prompt was a free-text queue head:

> I'm getting lint rejected for too long. Is 2300 words too long? Why

The assistant correctly inspected
`src/monsterrodholders-dev/scripts/waterways-augment-bot.mjs` and answered that
the `too long (2278)` lint is counting characters, because `lintProse()` rejects
`text.length > 1200`.

The failure is the closeout boundary: the answer was delivered to chat while
`tasks/monsterrodholders.md` still contained the queue prompt and no matching
`### Re:` response in `agent:exchange`.

## Evidence

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Session: `b04ec6ec-0a5c-4afb-b92e-c5448bce5969`
- `agent-doc <file>` from the owning Codex pane failed with the recursive direct
  invocation guard:
  `recursive direct invocation would deadlock`.
- The current worktree still shows the prompt as an uncommitted queue edit under
  `agent:queue`.
- The document has a `<!-- no-free-text-queue-head-guard -->` marker and a bare
  `###` after the queue close marker, but no clean response body for the lint
  question.
- The runbook already states that direct-chat session responses must be written
  with `agent-doc write --commit <FILE>` before success, but the final chat answer
  still escaped without that writeback.

## Related Work

- `tasks/agent-doc/plan-direct-chat-response-patchback-required.md`
- `tasks/agent-doc/plan-codex-final-response-not-written.md`
- `tasks/agent-doc/plan-finalize-owned-pane-response-patchback.md`

This recurrence is not a new root-cause family. It is evidence that the current
instruction/runbook invariant is insufficient without an enforced Codex final
gate or Stop-hook block for owner-pane direct-chat turns.

## Fix Plan

- [ ] Add a deterministic regression for an owned Codex pane that:
  - receives a session-document prompt,
  - trips the recursive direct invocation guard,
  - answers manually in chat,
  - and attempts to complete without `agent-doc finalize` or
    `agent-doc write --commit`.
- [ ] Make the Codex final gate fail closed when a known active session document
  has an unresolved prompt or queue head and the final assistant message has not
  crossed the `agent-doc` write boundary.
- [ ] Ensure the failure diagnostic names the exact recovery command:
  `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`, followed by
  `agent-doc session-check <FILE>`.
- [ ] Treat malformed partial writeback markers such as
  `no-free-text-queue-head-guard` plus a bare `###` as interrupted closeout
  evidence, not as a successful response body.
- [ ] Keep recovery idempotent: if the operator asks for manual repair after a
  partial write attempt, the response should be appended once, the free-text
  queue head should be struck once, and repeated recovery must not duplicate the
  response.

## Acceptance Criteria

- A direct-chat response to an active `agent-doc` prompt cannot reach final
  success while the session document still lacks a matching `### Re:` response.
- The final gate distinguishes chat-only answers from binary-owned closeout and
  prints a single actionable recovery command.
- `agent-doc session-check` catches the `no-free-text-queue-head-guard` / bare
  heading residue when no response body exists.
- Re-running recovery after a successful write is a no-op.
