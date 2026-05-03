# Plan — pane loss after `response_captured` must not strand the closeout

**Pending:** `#jbcap`
**Status:** Draft

## Problem

Live repro from `src/session-share/tasks/buildparty-investor-demo/dev.md` on May 2, 2026:

- cycle `cycle-1777708207821` reached `response_captured`
- the persisted cycle file stayed at `response_captured`
- no `write_applied`, `commit_success`, or `commit_already_current` followed for that cycle
- later the owning pane `%19` was recorded as `registered_pane_missing`
- the session was rebound to `%25`
- `agent-doc session-check .../dev.md` now reports:
  - `INTERRUPTED: cycle cycle-1777708207821 is still response_captured — response was captured but no write/commit followed`

This is not just a pane-retention bug.

- `#jbpdrop` covers the tmux/window invariant: a completed run should not remove its pane from the `agent-doc` window
- this new bug covers the closeout invariant: once a response is captured, later pane loss must not leave the document in a stranded `response_captured` state without deterministic recovery

## Evidence

From `.agent-doc/logs/586ffeea-ca3a-490b-80bd-883dd0e95673.log`:

- `[1777708207] document_cycle phase=preflight_started cycle=cycle-1777708207821 event=preflight_started`
- `[1777708656] document_cycle phase=response_captured cycle=cycle-1777708207821 event=response_captured capture_id=cycle-1777708207821`
- no later `write_applied` / `commit_*` event for `cycle-1777708207821`
- `[1777742341] supervisor_exit code=missing_pane pane=%19 reason=registered_pane_missing last_known_window=@8`
- `[1777742341] session_end origin=sync_missing_pane`

From the persisted cycle file:

- `.agent-doc/state/cycles/fbdde1a9f60dab3cf06c00e1e747fb607d46afdad89003acd3745214fff0dd30.json`
- `phase = "response_captured"`
- `had_pending_mutations = true`
- `pending_done_ids = ["bpcontract"]`

## Desired Behavior

1. Once a cycle reaches `response_captured`, agent-doc must either:
   - advance to `write_applied` / `committed`, or
   - persist a replayable recovery artifact before any pane-loss/session-rebind path can end the session
2. Pane disappearance after `response_captured` must not silently discard the closeout path.
3. `session-check`, `preflight`, or `repair` must be able to resume the interrupted closeout deterministically when hashes prove safety.
4. If safe resume is impossible, the failure must be explicit and phase-labeled, with the replay source preserved.

## Hypotheses To Check

- the pane/session-loss path can interrupt after capture metadata is written but before write/commit finalization persists its own recovery state
- `sync_missing_pane` or registry-rebind logic can supersede the live owner without first checking whether the current cycle is in `response_captured`
- the capture exists, but the replay/repair path for this exact shape is either missing or gated by pending/backlog mutation state
- closeout recovery currently assumes the pane remains live long enough to finish commit, which is false in this repro

## Implementation Steps

- [ ] Trace the `dev.md` repro across:
  - `capture.rs`
  - `write.rs`
  - `preflight.rs`
  - `session_check.rs`
  - `repair.rs`
  - `sync.rs`
  - any pane-loss / registry-rebind path that can fire after `response_captured`
- [ ] Define the invariant:
  - `response_captured` is not allowed to become terminal just because the pane disappeared
  - before `session_end origin=sync_missing_pane`, the system must either finish closeout or persist a durable replay source
- [ ] Harden pane-loss handling:
  - if the active cycle is `response_captured`, prefer closeout recovery over plain session replacement
  - do not supersede or clear the owning session state until that decision has been made
- [ ] Add deterministic recovery:
  - next `preflight`, `repair`, or `session-check` should be able to resume commit from `response_captured` when file/snapshot hashes still prove the captured response is the intended one
  - if pending mutations make automatic resume unsafe, preserve a targeted operator-facing recovery path instead of generic interruption
- [ ] Improve diagnostics:
  - log `response_captured_pane_loss`
  - log whether a replay artifact exists
  - log whether resume-commit was attempted, succeeded, or was blocked by drift/pending mutations
- [ ] Add regression coverage for:
  - `response_captured` followed by pane loss before write/commit
  - recovery succeeds on next command when hashes match
  - recovery fails closed with explicit diagnostics when drift or pending mutations block it

## Files Likely To Change

- `src/agent-doc/src/capture.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- the `dev.md` repro no longer strands a cycle at `response_captured` after pane loss
- a pane-loss path after capture either finishes the closeout or leaves deterministic replay state behind
- `session-check` can distinguish this from generic preflight interruption and points to the correct recovery behavior
- regression tests cover the exact `response_captured -> pane loss -> resumed closeout` shape
