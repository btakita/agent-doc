# Plan: direct-chat owned-pane response loss recurrence

## Problem

On 2026-06-23, a Codex turn answered a prompt from
`src/boost-client/tasks/monsterrodholders.md` in chat, but the answer did not
land in the session document.

The failure recurred on a later Big Bear Lake species prompt from the same
document. Codex updated and pushed the species data, ran focused validation and
the waterways app test/build suite, and answered the GBIF primary-source question
in chat, but the operator reported:

> Response was not added to the document.

The same document then hit the same failure again on the follow-up prompt:

> Remove the max character count cap.

Codex removed the upper-bound prose lint, verified the focused self-test, and
again the operator reported that the response was not added to the document.

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
- Later repro prompt: Big Bear Lake species task for
  `https://monsterrodholders.com/reservoirs/big-bear-lake-ca-269807/`.
- Later repro side effects completed outside the document writeback boundary:
  submodule commit `1ea4b054e` and parent commit `762b01e6` were pushed after
  the species data update, but no matching response was appended to
  `agent:exchange`.
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
- A second same-day direct-chat response in the same owned pane also escaped the
  document writeback path after the max-character-cap follow-up, so the
  regression must cover consecutive prompts in one live session, not just the
  first missed closeout.

## Related Work

- `tasks/agent-doc/plan-direct-chat-response-patchback-required.md`
- `tasks/agent-doc/plan-codex-final-response-not-written.md`
- `tasks/agent-doc/plan-finalize-owned-pane-response-patchback.md`

This recurrence is not a new root-cause family. It is evidence that the current
instruction/runbook invariant is insufficient without an enforced Codex final
gate or Stop-hook block for owner-pane direct-chat turns.

## Status

**COMPLETE (verified 2026-08-23).** All fix-plan items landed and are covered by green
regression tests; see Verification below.

## Fix Plan

- [x] Add a deterministic regression for an owned Codex pane that:
  - receives a session-document prompt,
  - trips the recursive direct invocation guard,
  - answers manually in chat,
  - and attempts to complete without `agent-doc finalize` or
    `agent-doc write --commit`.
- [x] Extend the regression to cover two consecutive direct-chat answers from
  the same owned pane, where the second answer also must not reach final success
  unless it crosses the `agent-doc` write boundary.
- [x] Make the Codex final gate fail closed when a known active session document
  has an unresolved prompt or queue head and the final assistant message has not
  crossed the `agent-doc` write boundary.
- [x] Ensure the failure diagnostic names the exact recovery command:
  `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`, followed by
  `agent-doc session-check <FILE>`.
- [x] Treat malformed partial writeback markers such as
  `no-free-text-queue-head-guard` plus a bare `###` as interrupted closeout
  evidence, not as a successful response body.
- [x] Keep recovery idempotent: if the operator asks for manual repair after a
  partial write attempt, the response should be appended once, the free-text
  queue head should be struck once, and repeated recovery must not duplicate the
  response.

## Acceptance Criteria

- A direct-chat response to an active `agent-doc` prompt cannot reach final
  success while the session document still lacks a matching `### Re:` response.
- Consecutive direct-chat responses in one owned-pane session are each checked
  independently for document writeback.
- The final gate distinguishes chat-only answers from binary-owned closeout and
  prints a single actionable recovery command.
- `agent-doc session-check` catches the `no-free-text-queue-head-guard` / bare
  heading residue when no response body exists.
- Re-running recovery after a successful write is a no-op.

## Verification (2026-08-23)

| Acceptance criterion | Implementation | Regression test |
|---|---|---|
| Direct-chat answer cannot reach final success while the document lacks a matching response | `agent-doc-codex-stop-io/src/lib.rs` `active_session_prompt_requires_writeback` | `agent_doc_codex_stop_io::tests::stop_blocks_direct_chat_manual_queue_response_after_recursive_guard` |
| Consecutive direct-chat answers each checked independently | same, keyed per `turn_id` via `prompt_writeback_debt` | `agent_doc_codex_stop_io::tests::stop_blocks_consecutive_direct_chat_manual_queue_answers` |
| Final gate distinguishes chat-only from binary-owned closeout and prints one recovery command | `agent-doc-session-check-io/src/command.rs:1437` (`codex-final-gate`) | `session_check::tests::codex_final_gate_blocks_on_recursive_invocation_without_captured_response` |
| `session-check` catches `no-free-text-queue-head-guard` + bare `###` residue | `agent-doc-workflow/src/session_check.rs:358`, `agent-doc-queue/src/queue_closeout_guard.rs` | `queue_closeout_guard::tests::free_text_queue_head_provenance_decision_reports_bare_heading_residue` |
| Recovery after a successful write is a no-op | `agent-doc-session-check-io/src/command.rs:1423` (idempotent manual-patchback adoption) | `session_check::tests::codex_final_gate_adopts_manual_patchback_when_response_is_visible` |

All listed tests pass on `agent-doc` v0.35.275.
