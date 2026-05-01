# Plan — simplify `Run Agent Doc` routed reopen and acknowledgment flow

**Pending:** `#runsm`
**Status:** Draft

## Problem

`Run Agent Doc` is still failing in user-visible ways after many targeted fixes because the routed reopen path in `src/agent-doc/src/route.rs` is carrying too many responsibilities at once:

- stale registration cleanup
- stash rescue
- busy-pane auto-fix
- supervisor restart
- bare reopen dispatch
- fresh-restart retry
- document-cycle acknowledgment inference

The `monsterrodholders.md` logs in `tasks/agent-doc/agent-doc-bugs2.md` show the practical result:

- route accepts the bare `agent-doc <FILE>` reopen
- route then waits for a *document-observed* cycle acknowledgment
- if that proof never materializes, route falls through more recovery branches and the user still ends up with "accepted, then nothing" or a startup-miss failure

This is now an architecture problem, not just another missing conditional.

## Root Cause To Prove

The main issue to prove and then remove is that `route.rs` is coupling **dispatch** with **post-dispatch session repair** and using the document's later cycle-state mutation as the primary routed-success proof.

That creates three unstable behaviors:

1. One function is classifying pane ownership, mutating tmux/session state, and deciding retry policy.
2. A routed reopen can be "accepted" at the pane level but still fail because a later document-side signal never settles.
3. Every new edge-case fix adds another branch to an already stateful retry chain, so the same symptom keeps resurfacing under a slightly different provenance.

## Desired Behavior

1. `Run Agent Doc` should have one bounded same-document recovery path:
   - reuse an authoritative ready pane, or
   - perform one fresh restart, or
   - fail closed with one stable reason.
2. Route success should be tied to an explicit supervisor / harness dispatch-start proof, not only to later document diff observation.
3. Startup-miss should remain a health signal, but it should no longer be the main mechanism for proving that a routed reopen even started.
4. JetBrains / tmux should surface one copyable failure message with exact stage information:
   - ownership check
   - dispatch acceptance
   - harness ready proof
   - cycle-start proof

## Implementation Steps

### Define the minimal routed state machine

- [ ] Write down the only supported same-document route states:
  - ready authoritative pane
  - authoritative pane requiring one fresh restart
  - no authoritative pane, start fresh
  - fail-closed
- [ ] Remove or quarantine opportunistic branches that mutate state mid-route without improving determinism.

### Separate classification from side effects

- [ ] Extract a pure "route decision" layer from `route.rs` that decides:
  - reuse
  - fresh restart
  - fresh start
  - fail-closed
- [ ] Keep tmux/session mutations in a smaller execution layer so logs match one decision path instead of a chain of retries.

### Replace the current success proof

- [ ] Introduce an explicit routed-dispatch start proof from the supervisor / harness side.
- [ ] Keep document `cycle_state` acknowledgment as a later health check, not the sole routed-success contract.
- [ ] Make the failure tell the user whether the reopen:
  - never reached the pane
  - reached the pane but was never consumed
  - started the harness but never opened a new document cycle

### Collapse same-document busy handling

- [ ] Remove the current "auto-fix, maybe no-op, maybe reopen, maybe fresh restart, then maybe wait again" chain for healthy same-document panes.
- [ ] Reduce this to one bounded retry policy:
  - one scoped cleanup if it produces a deterministic state transition, or
  - skip directly to fresh restart
- [ ] Do not report success from a no-op busy-pane path.

### Verification

- [ ] Re-run the live `monsterrodholders.md` routed repro after the simplification pass.
- [ ] Verify that `Route Already In Progress` / busy-pane follow-ups either recover in one retry or fail with one stable, copyable explanation.
- [ ] Confirm the next implementation turn patches the result back into `tasks/agent-doc/agent-doc-bugs2.md`.

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/editors/jetbrains/...`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- Routed same-document reopen uses one bounded state machine instead of layered recovery loops.
- A routed reopen can no longer look "accepted" and then silently stall without a precise stage-specific error.
- The `monsterrodholders.md` repro either works or fails once with a deterministic explanation that tells the user exactly what broke.
