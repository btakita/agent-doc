# Plan: JB Run Agent Doc closeout recovery state machine

Bug id: `#jbruncloseoutstate`
Tag: `[clean-session]`

## Symptom

`JB Run Agent Doc` has repeatedly failed on `tasks/agent-doc/agent-doc-bugs2.md`
after queue edits, restarts, compaction, or pre-finalize interruptions. Recent
visible failures include:

- `route could not drain the active closeout before dispatch: captured response
  baseline no longer matches current document`
- stale queue pauses that should self-heal after controller/supervisor freshness
  is proven
- repeated local fixes that add one more recovery exception in `route`, `repair`,
  `capture`, `session-check`, or IPC convergence

The immediate queue-edit repro was fixed by `#queueeditcap`, but the repeated
class of failures is structural: there is no single owner for the closeout
recovery decision.

## Root Cause

Closeout recovery is spread across too many boundaries:

- `route` tries to drain an open closeout before reroute.
- `repair` decides whether stale captures can be retired.
- `capture::validate_replay` decides whether captured response baselines can be
  refreshed.
- `session-check` reports committed/interrupted state.
- snapshot, CRDT, ack-content, editor IPC, and controller queue state each carry
  partial evidence.

That split makes each fix local and string-shaped. A new failure mode becomes
another exception around a diagnostic string instead of an exhaustively tested
transition in one state machine.

## Refactor Direction

1. Introduce a typed `CloseoutRecoveryDecision` boundary used by route, repair,
   session-check, and write/commit:
   - `AlreadyCommitted`
   - `ReplaySafe`
   - `RetireStaleCapture`
   - `ResetSidecarsFromVisible`
   - `QueuePromptForAfterCloseout`
   - `Blocked`
2. Move the evidence gathering behind one API:
   - visible markdown hash and structural component validity
   - snapshot hash
   - active cycle phase
   - active capture state
   - response-body presence or supersession proof
   - queue-only drift proof
   - editor IPC/ack-content freshness
   - controller/supervisor binary freshness
3. Make route consume only typed decisions. `JB Run Agent Doc` must not parse or
   rephrase low-level capture errors; it should either recover, queue behind the
   closeout, or surface one precise terminal blocker.
4. Make all recovery mutations go through one sidecar-reset/replay primitive so
   `reset --from-current --preserve-session`, stale-capture retirement, and
   safe queue-only replay cannot diverge.
5. Add SimWorld coverage for the full transition table, especially:
   - queue edit after response capture, before write
   - queue edit after write-applied response fragmentation
   - compaction archives a captured response
   - visible markdown is smaller than stale snapshot
   - stale supervisor ack-content offers a structurally valid but stale buffer
   - JB Run Agent Doc during open closeout with and without prompt context

## Lean / Formal Methods Answer

Lean is not the first tool to reach for here. The immediate gap is not theorem
proving; it is missing architecture: no typed transition boundary, no single
source of truth for recovery evidence, and too many string-matched recovery
paths.

Lean could help later for a small model of queue/cycle/capture invariants, but
it will not stop agents from producing poor Rust architecture by itself. The
practical next step is a Rust-level state machine with exhaustive enums,
property tests, and SimWorld scenarios. Once that exists, a Lean or TLA-style
model can validate the core invariants without trying to formalize the whole
editor/tmux/IPC system.

## Acceptance

- `route` has no direct string match for `captured response baseline no longer
  matches current document`; it handles a typed recovery outcome.
- A single recovery policy owns whether to replay, retire, reset sidecars, queue
  behind the active turn, or fail closed.
- `JB Run Agent Doc` never surfaces a manual reset command when the visible doc
  can be proven authoritative and safely recovered by the binary.
- Terminal failures name the missing proof, not a low-level implementation
  detail.
- SimWorld covers the transition table above and prevents future one-off
  recovery exceptions from bypassing the policy.
