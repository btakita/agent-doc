# Plan: unresolved exchange prompt preempts active auto queue

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-30:

1. The operator had a user prompt in `agent:exchange` after the last boundary:
   `What are #next-steps to complete review items?`
2. The document also had `queue_active: true` and an
   `<!-- agent:queue auto -->` with the same prompt queued behind an older
   queue head.
3. The prompt did not run. The `.agent-doc` snapshot already contained the
   exchange prompt, so the next agent-doc diff no longer treated it as user
   intent. The visible/snapshot diff was only queue bookkeeping:
   `- do [#gfwconnect]` removed from `agent:queue`.

Expected behavior: a live unresolved exchange prompt always runs before queue
continuation. `agent:queue auto` may queue follow-up work, but it must not make
an unanswered exchange prompt disappear into the snapshot or force the queue
head to run first.

## Likely Root Cause

Recent route/queue recovery work intentionally commits route-owned queued
snapshots before diffing again so a busy-pane `Run Agent Doc` handoff can survive
live edits. That path is too broad when the route-owned queued snapshot and the
visible exchange both contain prompt text.

The current code has pieces of the desired policy:

- `skill.rs` says a real user prompt mid-loop takes precedence over the
  auto-loop.
- `preflight.rs` synthesizes a queue prompt only when the document/harness diff
  is otherwise empty.
- `run.rs` prefers `diff::compute(file)` over a synthetic queue diff.

The missing invariant is snapshot-independent unresolved-prompt detection. If a
prompt line is after the latest exchange boundary and no response block answers
it, it is still executable user intent even when the snapshot already contains
the same line.

## Desired Behavior

Prompt selection precedence should be:

1. Unanswered user prompt in `agent:exchange` after the latest boundary.
2. Other user-intent prompt-bearing diff.
3. Active queue synthetic head, only when no unresolved exchange prompt exists.

If the unresolved exchange prompt text exactly matches the active queue head,
dispatch the exchange prompt first. After successful closeout, the queue head
may be consumed as the executed duplicate, but the agent must not skip the
response cycle.

## Implementation Outline

1. Add a pure helper that detects unresolved exchange prompts from document
   structure, not just from snapshot diff:
   - parse `agent:exchange`
   - find the latest `agent:boundary`
   - collect non-comment, non-empty user prompt text after that boundary and
     before the close marker
   - return `None` if that region is followed by a matching `### Re:` response
     in the same exchange segment

2. Use that helper in `preflight` before queue synthetic diff selection:
   - if unresolved exchange prompt exists, ensure `no_changes=false`
   - emit a synthetic prompt diff/source such as `exchange_unresolved` when
     ordinary `diff::compute` is empty because the prompt was accidentally
     baselined
   - keep `queue_prompts` populated for visibility, but set
     `user_intent_prompt_changes` so Claude auto-loop skips queue continuation

3. Use the same helper in direct `run`:
   - prefer unresolved exchange prompt over `active_queue_prompt_diff`
   - log the precedence decision so route/queue failures are diagnosable

4. Tighten route queue snapshot recovery:
   - when committing a route-owned queued snapshot, do not absorb live exchange
     prompt text into the snapshot unless that prompt was already answered
   - if queue recovery sees the same text in exchange and queue, preserve the
     exchange prompt as the next executable cycle

5. Tighten queue continuation final gates:
   - `queue_continuation::detect`, Codex Stop, and `session-check
     --codex-final-gate` should report/block queue continuation as deferred
     while an unresolved exchange prompt exists
   - diagnostics should name both the unresolved exchange prompt and the pending
     queue head

## Tests

- Preflight regression modeled on the live `monsterrodholders.md` snapshot:
  snapshot contains an unanswered exchange prompt plus `agent:queue auto`; file
  differs only by queue bookkeeping. Expected: `no_changes=false`,
  `user_intent_prompt_changes` contains the exchange prompt, and queue auto-loop
  is blocked.
- Direct run regression: same fixture calls `agent-doc run` with a mock agent;
  the mock receives the exchange prompt, not the queue head.
- Route recovery regression: route-owned queue snapshot plus later visible
  exchange prompt keeps the exchange prompt unbaselined and opens a fresh
  `preflight_started` cycle for it.
- Queue duplicate regression: exchange prompt text equals queue head; the
  response is written once, then queue consumption removes/strikes the matching
  head after closeout.
- Codex final-gate regression: active queue continuation marker exists, but an
  unresolved exchange prompt exists; the Stop hook refuses to continue the queue
  first and instructs running the exchange prompt cycle.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- `src/agent-doc/agent-doc-orchestration/src/route.rs`
- `src/agent-doc/agent-doc-orchestration/src/queue_continuation.rs`
- `src/agent-doc/agent-doc-orchestration/src/diff.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/tests/codex_hook_integration.rs`
