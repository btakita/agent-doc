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

Per `#agent-doc-bug`, this is a plan only. Do not implement the fix in this
cycle.
