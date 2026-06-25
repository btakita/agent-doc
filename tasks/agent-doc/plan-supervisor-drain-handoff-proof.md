# Plan: supervisor drain handoff must prove the current queue head

## Problem

On 2026-06-23, repeated direct runs of
`agent-doc src/boost-client/tasks/monsterrodholders.md` reported:

- `cycle-1782184588500` was `committed`.
- `ui_outcome=deferred_for_supervisor_drain`.
- `next_action=yield_to_supervisor_clear_and_continue`.

The same `session-review --next-context` still showed an active queue prompt:

```text
scripts/hermes-serve.sh should start ollama is not already started. scripts/hermes-serve.sh exits with code 1 if ollama is not running without any error mess...
```

The operator then reported: "The queue item was either not processed or the
response not written into the document."

This is worse than a normal deferred head: the user-facing closeout says the
agent should yield to supervisor drain, but there is no proof surfaced to the
agent or operator that the current visible queue head was claimed, dispatched,
completed, and materialized in the document.

## Related Work

- `#qfocsup` defines the intended `[focused-cycle]` path: the in-session loop
  yields, then supervisor idle-watch force-`/clear`s and re-dispatches the head.
- `#tb4q` is the current live verification item for that path.
- `#mrh-response-loss-stalled-queue` already covers an MRH response-loss plus
  queue-stall closeout. This plan is the guardrail for the specific
  `session-check` / supervisor-handoff false-success surface.

## Root-Cause Hypothesis

`session_check.rs` currently treats any
`live_drainable_continuation_head(file, content)` result after in-session
continuation detection returns `None` as sufficient to emit
`DeferredForSupervisorDrain`.

That is a statement about drainability, not a terminal proof. It does not bind
the result to:

- the current visible queue head text or id,
- the cycle that just committed,
- a durable supervisor handoff marker,
- a subsequent route/dispatch-start proof,
- queue consumption,
- or response materialization in `agent:exchange` or a compact archive.

If the supervisor is stale, not installed, already busy, misses the head, or if
the response-write path loses materialization, the direct agent can still end
with "yield to supervisor" while the document remains unresolved.

## Desired Behavior

`deferred_for_supervisor_drain` must mean "a fresh supervisor drain is actively
owned or can be verified," not merely "a supervisor-drainable head exists."

When `session-check` emits `yield_to_supervisor_clear_and_continue`, it should
also make one of these facts true:

1. A durable handoff marker exists for the current document, queue-head hash/id,
   and current document revision/cycle.
2. The supervisor has already claimed that marker and logged a matching
   clear-and-continue dispatch attempt.
3. A later `session-check` can prove the marker resolved by seeing the head
   consumed and its response materialized.

If none of those proofs exists, `session-check` must fail closed with an
actionable diagnostic instead of returning an OK/yield state.

## Implementation Sketch

1. Add a durable supervisor-handoff record keyed by canonical document path and
   active queue head identity. Include the document revision/base hash, current
   cycle id, queue head text/id, timestamp, and expected drain mode
   (`focused-cycle` or `clean-session`).
2. In `session_check.rs`, when the `#qfocsup` branch selects
   `DeferredForSupervisorDrain`, create or refresh that marker before printing
   success. If marker creation fails, exit non-zero.
3. In `start/idle_watch.rs`, when idle-watch force-clears and dispatches a
   focused-cycle or clean-session head, claim the marker and log the claim,
   clear delivery, route-dispatch proof, and route outcome under the same marker
   id.
4. At closeout, clear the marker only after both conditions hold:
   the original queue head is no longer active, and a response for that head is
   present in exchange or compact archive.
5. If the marker expires, points at a different visible head, or lacks dispatch
   proof, surface a `supervisor_drain_handoff_unproven` diagnostic that names the
   recovery command and refuses to report "OK".
6. Update `session-review --next-context` to display outstanding handoff markers
   so an agent can distinguish "yielding, supervisor owns it" from "the queue is
   still stuck."

## Regression Coverage

- Unit: `session-check` with a supervisor-drainable focused-cycle head creates a
  marker tied to the current head and emits `deferred_for_supervisor_drain`.
- Unit: if marker creation fails or references a stale head hash, `session-check`
  exits non-zero and names the current head.
- SimWorld: direct run commits a cycle, yields to supervisor, idle-watch claims
  the marker, force-clears, dispatches, writes a response, consumes the head, and
  clears the marker.
- Negative SimWorld: idle-watch never claims the marker; the next
  `session-check` reports `supervisor_drain_handoff_unproven` instead of OK.
- Negative SimWorld: response write is lost while the queue head is consumed;
  marker remains unresolved and points at missing materialization.
- Live verification: rerun the MRH `hermes-serve.sh should start ollama` queue
  shape and confirm the operator-visible state is either a written response or a
  fail-closed handoff diagnostic, never a silent yield.

## Exit Criteria

- No `agent-doc <file>` invocation can end with a final OK/yield message unless
  the supervisor handoff has a durable, current-head proof.
- A current queue head cannot remain active after an OK/yield without an
  outstanding marker that names exactly who owns the next action.
- Missing response materialization after supervisor drain is reported as a
  response-loss closeout failure, not as a successful deferred drain.
