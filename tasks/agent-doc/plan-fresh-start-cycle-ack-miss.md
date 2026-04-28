# Plan — fix fresh-start cycle-ack misses that still look like tmux pane crashes

**Pending:** `#freshack`
**Status:** Implemented

## Problem

`#tmuxcrash` / `#bugs2tmux` closed the stale-owner / ambiguous-pane family, but `tasks/software/tsift.md` still shows a separate user-visible failure:

- the previous Codex-backed pane exits cleanly (`codex_exit code=0`, `restart_eval ... ctrl_d=true ... action=prompt_user`, `user_quit_after_eof`, `session_end`)
- a fresh replacement pane is then started for the same document (`%279`, then `%285` in `.agent-doc/logs/tsift-v0.1.log`)
- the next real prompt-bearing edits still hit `route_cycle_start_missing` in `.agent-doc/logs/ops.log` (`do #prunefix...`, `do #commlock...`)

So the user experiences “the tmux pane crashed” even though the pane did not literally die from a tsift/runtime crash. The remaining failure is that a fresh pane can be launched but never prove a real new `agent-doc` cycle.

## Root Cause To Prove

The existing fresh-start contract in `route.rs` already requires a real cycle acknowledgment, and the cleanup path now preserves the new pane instead of killing it. But the `tsift.md` evidence suggests one of these still happens:

1. the fresh pane starts, but the trigger never reaches the actual child harness
2. the trigger is injected, but startup readiness/prompt proof is wrong
3. the harness receives the trigger, but no durable cycle state is recorded
4. route/start fail closed correctly, but the pane/supervisor surface still looks like a dead crash instead of a clear startup-miss diagnostic

## Desired Behavior

1. A fresh pane start for a prompt-bearing session doc either reaches `preflight_started` (or later) for a newer cycle, or it fails with a precise startup-miss reason that is visible both in logs and in the pane.
2. A preserved fresh pane must not look like a mysterious dead shell after a missed acknowledgment.
3. Re-running `agent-doc` after this shape should deterministically recover or fail closed with exact provenance instead of repeating the same ambiguous “pane crashed” experience.

## Implementation Steps

### Reconstruct the exact `tsift.md` startup-miss chain

- [ ] Rebuild the known `tsift.md` episodes from persisted evidence:
  - clean-exit pane `%267`
  - fresh-start panes `%279` and `%285`
  - matching `route_cycle_start_missing` entries for `#prunefix` and `#commlock`
- [ ] Capture the exact state tuple for each episode:
  - supervisor log tail
  - ops.log route/start entries
  - cycle id (or absence)
  - pane id / tmux session / window
  - whether the child harness prompt was ever observed

### Audit the fresh-start path end to end

- [ ] Trace `route.rs` fresh-start flow after trigger dispatch:
  - pane creation / registration
  - trigger injection
  - acknowledgment wait
  - failure cleanup / preservation
- [ ] Audit `start.rs` supervisor startup and trigger handoff:
  - child PTY writer
  - prompt readiness proof
  - shutdown / cancellation races
  - any path where the harness starts but the ack observer is looking at the wrong process/output
- [ ] Audit the cycle-state writer path to prove whether a real `preflight_started` can happen but still be missed by the ack waiter.

### Tighten the user-visible failure contract

- [ ] If fresh start misses cycle acknowledgment, emit a stable failure state that says “fresh pane started but no new cycle began” instead of surfacing as a generic pane-crash impression.
- [ ] Decide whether the preserved pane should show a supervisor diagnostic banner / prompt telling the user what failed and how to recover.
- [ ] Ensure reroute / rerun logic distinguishes this startup-miss state from stale-owner ambiguity and from a real dead pane.

### Logging and diagnostics

- [ ] Add explicit provenance for fresh-start misses:
  - pane id
  - supervisor pid
  - child pid if present
  - whether prompt proof succeeded
  - whether trigger write succeeded
  - whether any cycle-state file changed
  - final fail-closed reason
- [ ] Make `session-check` / related recovery surfaces identify this exact class when the document still has prompt-bearing drift but the latest fresh pane never opened a new cycle.

### Tests

- [ ] Regression: fresh pane starts, trigger write is consumed, but no cycle state appears -> route fails with precise startup-miss reason and preserves pane provenance
- [ ] Regression: fresh pane starts and cycle state appears -> acknowledgment succeeds as normal
- [ ] Regression: preserved fresh pane from a startup miss is recognized on rerun and does not masquerade as a healthy live session
- [ ] Regression: tsift-like clean-exit -> fresh-start -> startup-miss sequence yields deterministic diagnostics instead of a generic tmux-crash experience

### Verification

- [ ] Verify from a real tmux-backed Codex session on `tasks/software/tsift.md` that the old symptom no longer appears as an unexplained pane crash
- [ ] Patch the concrete outcome back into `tasks/agent-doc/agent-doc-bugs2.md` and reduce backlog only after the real-session verification passes

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- The `tsift.md` “pane crashed” follow-up can be classified as a fresh-start startup miss with exact evidence.
- Fresh-start acknowledgment misses no longer present to the user as an unexplained tmux pane crash.
- Re-running after this state either recovers deterministically or fails closed with a precise startup-miss diagnostic.
