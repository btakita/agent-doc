# Plan - recursive direct invocation guard should close its empty preflight cycle

**Pending:** `#recguard-abandon`
**Status:** Draft

## Problem

The recursive direct-invocation guard correctly prevents a Codex-backed
`agent-doc <FILE>` command from dispatching back into the same pane that is
already waiting for that command to return. However, the guard currently records
`recursive_direct_invocation_blocked` and leaves the cycle in
`preflight_started`.

Live repro, 2026-05-30:

- Command run from inside the owning Codex pane:
  `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
- Target owner:
  `current_pane=%28`, actor generation `238`, actor state `ready`
- Result:
  the command failed with `recursive direct invocation would deadlock`
- Follow-up `session-check` still failed because cycle
  `cycle-1780112251743` remained `preflight_started`
- Manual `agent-doc cancel <FILE>` was required to abandon the empty cycle

The prior `#preflighthang` fix made the failure bounded and diagnosable, but the
remaining cycle state still blocks the owner session until an explicit cancel or
stale-cycle repair runs.

## Goal

When `run.rs` detects recursive direct invocation before any response capture,
it should fail closed and immediately mark the newly opened empty preflight
cycle as terminal/recoverable in a way that `session-check` accepts.

## Implementation Steps

- [ ] Reproduce the live shape in a focused regression:
  - Codex harness detected
  - `TMUX_PANE` matches the registered/actor owner
  - `run` opens preflight and hits `recursive_codex_direct_invocation_diagnostic`
  - no response capture exists
- [ ] Change the recursive guard path in `run.rs` to abandon or otherwise close
      the empty preflight cycle before returning the diagnostic.
- [ ] Keep the failure user-visible; the command should still refuse recursive
      self-dispatch instead of pretending work was completed.
- [ ] Update `session_check.rs` expectations so the post-failure check reports a
      terminal abandoned/cancelled guard state instead of an interruption.
- [ ] Verify the existing direct invocation timeout behavior still leaves enough
      diagnostic detail for non-recursive hangs.
- [ ] Update the command/spec text for the recursive self-dispatch branch.

## Files Likely To Change

- `agent-doc-orchestration/src/run.rs`
- `agent-doc-orchestration/src/session_check.rs`
- `tests/run_integration.rs`
- `specs/07-core-commands.md`
- `VERSIONS.md`

## Exit Criteria

- Recursive same-pane direct invocation fails fast with the existing deadlock
  diagnostic.
- The cycle is not left open in `preflight_started`.
- `agent-doc session-check <FILE>` succeeds or reports a terminal abandoned
  state immediately after the guarded failure.
- Operators no longer need a manual `agent-doc cancel <FILE>` for this guard
  path.
