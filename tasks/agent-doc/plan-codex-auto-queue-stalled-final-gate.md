# Plan: #codex-auto-queue-stalled-final-gate

## Problem

`#codex-auto-queue-live-verify` failed in a live Codex run against
`src/boost-client/tasks/monsterrodholders.md`.

Observed sequence:

1. The document had `queue_active: true` and an active
   `<!-- agent:queue auto -->`.
2. The completed queue head `#seocat` was removed/marked done and the next head
   became `do [#seopdp]`.
3. The session document and repo changes were committed and pushed.
4. Codex sent the final answer instead of being blocked/continued into the next
   queue item.
5. The operator reported: "The queue stalled. #agent-doc-bug. See
   [#codex-auto-queue-live-verify]".

Expected behavior: if a Codex-managed `agent-doc` turn closes cleanly while an
`agent:queue auto` still has a ready prompt, Codex must not send a normal final
answer. It should be forced to continue with `agent-doc <FILE>` or fail closed
with a precise recovery command.

## Current Design Gap

Codex queue continuation currently depends mainly on
`agent-doc hook codex-stop` finding a tracked Codex session state and then
calling `active_auto_queue_prompt(file)`.

That is too narrow for the live failure:

- A response can close the document manually or through an unusual recovery path
  after `agent-doc <FILE>` direct invocation is rejected as recursive.
- The Stop hook can miss the document if `UserPromptSubmit` did not persist state
  for the exact API/session/root shape.
- `session-check` returning OK is not enough: a clean document can still have a
  required auto-queue continuation.
- The only durable proof after closeout is the document itself:
  `queue_active: true`, `agent:queue auto`, and a ready first prompt.

## Proposed Fix

Add a binary-owned "queue continuation required" final gate that is independent
of fragile Codex in-memory/session hook state.

1. Add a shared detector, probably near `queue`/`preflight` or a small new
   `queue_continuation` module:
   - input: session document path
   - output: `None` or `{ head_prompt, head_id, reason }`
   - true only when:
     - frontmatter has `queue_active: true`
     - `agent:queue` has `auto`
     - `resolve_activation(..., has_auto=true, exchange_triggered=false,
       persisted_active=true)` is active
     - head is not a stop fence
     - head is not a future time gate
     - snapshot comparison does not prove the head was edited/modified

2. Persist a durable continuation marker after closeout when the detector is
   true:
   - path under `.agent-doc/queue-continuations/<doc-hash>.json`
   - fields: `file`, `head_prompt`, optional `head_id`, `created_at`,
     `source_command`, `commit_head`
   - write it from successful binary closeout paths: `finalize`,
     strict `write --commit`, `repair` closeout, and any helper shared by the
     Codex Stop-hook auto-close path
   - clear it when queue drains, `auto` is removed, `queue_active` is false, or
     the head prompt advances

3. Teach `codex-stop` to consult the durable marker before clearing state:
   - if tracked session state exists, keep current behavior but use the shared
     detector
   - if tracked session state is missing but a marker exists under the current
     project root for the active document, block the final answer and instruct:
     `agent-doc <FILE>`
   - if `stop_hook_active=true` and the marker/head is unchanged from the last
     continuation request, fail closed with the existing non-advancing-head
     message

4. Make `agent-doc session-check` expose this state without breaking normal
   non-Codex checks:
   - keep exit 0 for a clean document by default
   - add a typed detail such as `queue_continuation_required=true` /
     `next_queue_prompt=...`
   - optionally add a strict mode used by Codex direct-exec closeout paths,
     for example `session-check --codex-final-gate`, that exits nonzero when
     auto-queue continuation is required

5. Update installed Codex guidance/runbooks:
   - after any `agent-doc` managed turn, if closeout or session-check reports
     an active auto queue continuation, do not send the final answer
   - run `agent-doc <FILE>` to continue or remove/halt `auto` intentionally
   - make the live-heading guidance from `#codex-auto-queue-live-verify`
     explicit: exact heading or `--done` proves consumption, but continuation is
     still required when a later ready head remains

## Tests

Add deterministic coverage before live verification:

1. `codex_hook_integration`: tracked state present, session-check OK, active
   queue marker/head present -> Stop hook blocks with the next prompt.
2. `codex_hook_integration`: tracked state missing, durable continuation marker
   present -> Stop hook still blocks and names `agent-doc <FILE>`.
3. `codex_hook_integration`: `stop_hook_active=true` and marker head unchanged
   -> Stop hook returns `continue=false` with the non-advancing-head recovery.
4. `finalize_integration` or a new queue-continuation integration test:
   finalizing a queue head with remaining prompts writes the continuation marker;
   draining/removing auto clears it.
5. `session_check` test: clean document with active auto queue reports
   `queue_continuation_required` in structured output and strict Codex final
   gate mode exits nonzero.
6. Regression fixture modeled on
   `src/boost-client/tasks/monsterrodholders.md`: `#seocat` consumed, `#seopdp`
   remains, `queue_active: true`, `agent:queue auto`, no document dirt, and
   Stop hook must not allow final answer.

## Live Verification

Use `#codex-auto-queue-live-verify` as the live gate after the fix:

1. Create or reuse a Codex-owned document with an `agent:queue auto` containing
   two safe no-op items.
2. Let the first item close cleanly with exact heading or `--done` proof.
3. Confirm Codex is blocked/continued before final answer and the second item
   starts.
4. Confirm a repeated stop with an unchanged head fails closed instead of
   looping forever.
5. Patch the result back to `tasks/agent-doc/agent-doc-bugs2.md` and close both
   the implementation item and `#codex-auto-queue-live-verify` only after the
   live proof passes.

## Non-Goals

- Do not loosen queue-head consumption matching. Exact heading / `--done` proof
  remains required to consume a completed head.
- Do not auto-run queues without `auto`; that is covered separately by
  `#active-queue-persisted-no-continue`.
- Do not rely on agent instructions alone. The final gate must be binary-owned
  and testable.
