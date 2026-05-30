# Plan: Active persisted queues should continue without requiring `auto`

## Bug

An already-active queue can stall after a successful closeout when the document
has `queue_active: true` but the queue opening tag is plain
`<!-- agent:queue -->`.

Observed state on 2026-05-30:

- `tasks/agent-doc/agent-doc-bugs2.md` had `queue_active: true`
- the queue opening tag was `<!-- agent:queue -->`, not
  `<!-- agent:queue auto -->`
- five ready queue prompts remained, starting with
  `do [#pending-gate-snapshot-desync]`
- the prior turn closed out, but the next queue head did not dispatch

This is distinct from `#queue-auto-no-continue`: that item covers an
`agent:queue auto` continuation failure. This case is a persisted active queue
that no longer has the `auto` attribute.

## Likely Cause

The original queue design separates activation from continuation:

- `auto` starts an inactive queue without an exchange prompt
- `queue_active: true` means the queue is already activated and should keep
  consuming ready prompts on later cycles

Several continuation paths appear to conflate those two concepts:

- `codex_hook.rs::active_auto_queue_prompt` returns `None` unless the queue
  component still has the `auto` attribute
- `run.rs::should_continue_auto_queue` stops when the queue consumption outcome
  is not marked `auto`
- the Claude Code auto-loop instructions check `preflight.queue_trigger ==
  "auto"`, so a persisted active queue (`queue_trigger == "persisted"`) is not
  eligible for automatic continuation

That leaves a valid active queue in a silent resumable-but-not-running state.

## Desired Behavior

Treat `auto` as a queue start trigger only. Once a queue is active, continuation
should be driven by active queue state:

- `queue_active: true`
- at least one remaining live prompt
- no stop fence at head
- no deferred time gate at head
- no head-prompt modification since the snapshot
- no real user-intent prompt that should preempt the queue
- previous closeout/session-check succeeded

Inactive plain queues must remain inert. A queue block without `auto` and
without `queue_active: true` should never self-start.

## Implementation Outline

1. Rename or split `active_auto_queue_prompt` so the shared helper detects
   active ready queue heads, not only `auto` ready queue heads.
2. Update the Codex Stop hook to continue when the active queue helper returns
   a ready prompt, regardless of whether the opening tag has `auto`.
3. Update the direct run continuation gate:
   - keep requiring a successful closeout and queue-owned cycle
   - allow persisted-active queue consumption outcomes to continue
   - keep the existing stop/time/head-modified diagnostics
4. Update the Claude Code auto-loop instruction and rendered skill tests so
   `queue_trigger == "persisted"` is also continuation-eligible when
   `queue_active == true` and `queue_prompts` is non-empty.
5. Add a stop diagnostic when an active queue is intentionally left resumable
   instead of continued, naming the stop reason and the next head prompt.
6. Update the queue spec to state the separation explicitly: `auto` starts,
   `queue_active` continues.

## Tests

- Codex Stop hook regression: document has `queue_active: true`, plain
  `<!-- agent:queue -->`, and a ready prompt; the hook schedules the next
  `agent-doc <FILE>` cycle after a clean closeout.
- Negative Stop hook regression: plain `<!-- agent:queue -->` with
  `queue_active: false` does not schedule continuation.
- Direct run integration: a queue started by exchange trigger consumes item 1,
  persists `queue_active: true`, then continues item 2 even after `auto` is not
  present.
- Halt regression: stop fence, future time gate, and head-modified cases do not
  auto-continue and print the existing stop reason.
- Skill rendering regression: Claude Code auto-loop instructions include
  persisted active queues; Codex/OpenCode guidance remains harness-specific.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/codex_hook.rs`
- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- `src/agent-doc/agent-doc-orchestration/src/write.rs`
- `src/agent-doc/agent-doc-orchestration/src/queue.rs`
- `src/agent-doc/src/skill.rs`
- `src/agent-doc/specs/07-orchestration-commands.md`
- `src/agent-doc/tests/run_integration.rs`
