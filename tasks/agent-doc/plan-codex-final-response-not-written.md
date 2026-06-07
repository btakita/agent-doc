# Plan - `agent-doc-bugs2` `#codex-final-response-not-written`

## Problem

A Codex-owned `agent-doc` turn on `src/boost-client/tasks/monsterrodholders.md`
completed implementation, commits, pushes, and a chat close-out, but the final
assistant response did not land through the session document's normal
`agent-doc write` / commit boundary. The user had to request a manual patchback.

The visible failure chain was:

- `agent-doc` refused recursive direct invocation because the current Codex pane
  already owned `tasks/monsterrodholders.md`.
- Codex continued manually and produced the final close-out outside the normal
  write capture path.
- A later recovery commit placed the response in the document, but
  `session-check` still treated the response as a direct historical patchback
  rather than a binary-owned closeout.

## Additional Repro - 2026-06-03

The same failure recurred after the PDP fit-guide spacing task in
`src/boost-client/tasks/monsterrodholders.md`:

- Codex completed the repo change, pushed `monsterrodholders-dev` commit
  `152fbce8`, deployed with `./scripts/git-to-wp.sh`, verified the origin page,
  and pushed `boost-client` commit `b2b7953`.
- The chat final response summarized the commits and verification, but no
  matching `### Re:` response was appended to `agent:exchange`.
- The user had to issue: "response was not written into the document.
  #agent-doc-bug. Write response into the document".
- Manual repair appended the missing closeout response into the document. This
  is new evidence for backlog item `#codex-final-response-repeat` and the
  existing `#codex-final-response-live-verify` guard.

This cycle only records the repro and manual document repair. Do not implement
the agent-doc fix here.

## Additional Repro - 2026-06-06 (systematic queue-drain, strongest evidence)

User: *"monsterrodholders.md is running through the queue but I don't see any
responses."* Non-disruptive inspection of `src/boost-client/tasks/monsterrodholders.md`
(`agent: codex`, `agent_doc_format: template`, `agent_doc_write: crdt`,
queue-driven) confirmed the symptom directly:

- The exchange contains **zero `### Re:` blocks** — only a compacted
  `### Session Summary`. Every assistant response since the last compaction is
  missing.
- The queue is actively draining: `agent:status` advances
  (`Completed … #pacific-mountain-proof-audit`, `Gated … #regional-draft-pipeline`)
  and recent commits (`bc51baf`, `54e493d`, `65e339a`, …) land real deliverables
  (e.g. `bc51baf` bumps the `src/monsterrodholders-dev` submodule pointer).
- But every one of those commits mutates **only** the `agent:status` / `agent:queue`
  / backlog components — `git show <c> -- tasks/monsterrodholders.md` shows **0**
  added `### Re:` lines across the last three commits.

So this is not the one-off owned-pane manual-fallback shape of the earlier repros:
it is the **systematic auto-loop case** where each `do [#id]` Codex cycle does the
work, commits status/queue/code, and advances the queue, yet **never writes an
assistant response body into `agent:exchange`**. The user only sees the queue
moving (status changes), never a response — which is exactly the reported
experience. This is the highest-value reproduction for the proposed fix: the
binary-owned continuation/closeout must require the `### Re:` response body in
`agent:exchange` (Goal/Proposed-Fix #2 and #4) for queue-drain cycles, not only
for the recursive-fallback path. Captured as `#codex-queue-drain-no-response-body`.

## Goals

- A Codex final answer for an owned-pane/manual fallback path must either be
  written into `agent:exchange` before the user sees it, or the stop/final gate
  must fail closed with an exact recovery command.
- Recovery must be idempotent when the response body is already visible in the
  document: no duplicate response block, no misplaced boundary, no lost
  follow-up prompt.
- `session-check` must distinguish a recoverable already-visible response from a
  genuinely unsafe historical patchback that mutated non-exchange components.

## Proposed Fix

1. Add an integration regression for an owned-pane recursive/direct invocation:
   the route/preflight path refuses recursion, the agent completes manually, and
   the Codex stop hook receives a non-empty final assistant message.
2. Teach the Codex stop/final gate to treat a non-empty final message plus an
   open or bypassed `agent-doc` cycle as mandatory `agent-doc finalize` /
   `write --commit` work before allowing the chat response to complete.
3. Extend the write recovery path for "response already present" to adopt the
   current visible response when the only unsafe evidence is stale snapshot
   drift from the same response marker, while still refusing non-exchange drift
   that cannot be proven safe.
4. Add a `session-check` assertion that a committed turn contains the final
   assistant response body in `agent:exchange` and that the saved snapshot,
   capture record, and HEAD agree after recovery.
5. Cover the follow-up prompt case: if a user prompt was typed after the visible
   response, recovery must preserve it as unanswered work and must not absorb it
   into the recovered assistant response.

## Acceptance Criteria

- Reproducing the `monsterrodholders.md` owned-pane fallback writes the final
  Codex response into `agent:exchange` before final chat output.
- Running `agent-doc session-check` after the turn returns OK without requiring
  manual `reset --from-current`.
- Replaying recovery with the same response body is a no-op, not a duplicate.
- If a follow-up prompt appears after the recovered response, it remains visible
  and queued for a later turn.

## Implementation status

- **Proposed-Fix #4 — DONE (`#codex-queue-drain-no-response-body`).**
  `session_check::check_committed_without_response_body_guard` no longer trusts
  capture metadata (`capture_id`/`response_sha256`) as sole proof for a
  **queue-drain** turn: a committed turn that did binary-owned work
  (`had_pending_mutations`) and recorded a queue head
  (`queue_task_id`/`active_queue_heads`) must now actually contain a `### Re:`
  response body in the committed `agent:exchange` (new
  `committed_exchange_has_response_body` helper). The systematic Codex symptom —
  capture record set, but the commit mutated only status/queue/backlog leaving
  exchange at zero `### Re:` (only a compacted `### Session Summary`) — now fails
  closed with the `agent-doc write --commit` recovery hint. Non-queue turns keep
  trusting the capture record (no behavior change). Tests:
  `committed_without_response_body_guard_fires_on_queue_drain_captured_but_no_exchange_body`,
  `committed_without_response_body_guard_passes_queue_drain_with_exchange_body`.
- **Remaining:** Proposed-Fix #1/#2 (Codex stop-hook owned-pane recursion gate +
  mandatory finalize before chat completion) and #3/#5 (write-recovery adopt of an
  already-visible body + follow-up-prompt preservation) — Codex-harness-specific,
  separate cycle.
