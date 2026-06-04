# Plan - finalize owned-pane response patchback

**Pending:** `#finalize-owned-pane-response-patchback`
**Status:** Draft

## Problem

On 2026-06-04, a Codex-owned turn for:

`src/boost-client/tasks/monsterrodholders.md`

completed the MRH blog pipeline work, committed and pushed the relevant
submodule/root changes, and produced a final chat response. The task document
did get a status/review update, but the actual final response was not appended
to `agent:exchange` until the operator asked for a manual patchback.

## Why Finalize Did Not Work Correctly

The response did not travel through a binary-owned finalize/write capture path.
The observed chain was:

1. `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
   was invoked from inside the Codex pane that already owned that same document.
2. `agent-doc` correctly refused recursive direct invocation with
   `recursive direct invocation would deadlock`.
3. The assistant continued the task manually in the live Codex chat, edited the
   document/status by hand, committed and pushed the work, and then emitted the
   final answer directly to the user.
4. Because that final answer was not passed to `agent-doc write --commit`,
   `agent-doc finalize`, or an equivalent capture record, the binary never had
   the final response body to append into `agent:exchange`.
5. The final gate/session-check path did not fail closed when the owned-pane
   manual fallback produced a non-empty final response outside the document.

So this was not a content-generation or git-push failure. It was an agent-doc
closeout ownership failure: after recursive same-pane dispatch was refused,
manual fallback allowed final chat output without proving that the response body
had been captured in the session document.

## Desired Behavior

When a same-pane owned-document invocation is refused but the agent continues
manually, closeout must be explicit and verifiable:

- the final response body must be written through `agent-doc write --commit` /
  `finalize` before chat output completes, or
- the stop/final gate must block the chat response with an exact recovery
  command, and
- session-check must verify that the final response body appears in
  `agent:exchange`, not merely that a status/review line or git commit exists.

## Plan

1. Add a regression fixture for an owned Codex pane invoking `agent-doc <FILE>`
   on its own document and receiving `recursive direct invocation would
   deadlock`.
2. Model the manual-fallback path:
   - document has unresolved exchange prompt
   - assistant performs side-effect work manually
   - assistant has a non-empty final response
   - no pending/finalize capture record exists
3. Extend final-gate/session-check logic to require one of:
   - committed exchange response body matching the final assistant response
   - pending capture replayed by `write --commit` / `finalize`
   - explicit blocked recovery diagnostic
4. Teach recursive same-pane guidance to print a safe recovery command:
   - `agent-doc write --commit <FILE>` when final response text is available on
     stdin
   - a session-check repair command when a visible response is already present
5. Add a recovery adoption path for manual patchback:
   - if the response is already visible in `agent:exchange`, adopt it
     idempotently
   - preserve later user prompts after the response boundary as unanswered work
   - refuse non-exchange drift
6. Improve final diagnostics for nested repos/submodules by composing with
   `tasks/agent-doc/plan-submodule-response-commit-proof.md`.

## Tests

- Same-pane recursive refusal plus manual final answer fails closed until the
  response body is written into `agent:exchange`.
- `write --commit` after recursive refusal appends the response once and commits
  the owning repo.
- Re-running recovery when the response is already visible is a no-op.
- A later user follow-up prompt after the recovered response remains queued.
- Submodule target doc closeout reports both target-doc commit proof and parent
  pointer state without implying the final response was captured if it was not.

## Non-Goals

- Do not weaken the recursive direct-invocation deadlock guard.
- Do not auto-commit unrelated parent-repo or submodule changes.
- Do not treat status/review metadata as a substitute for the final response
  body in `agent:exchange`.
