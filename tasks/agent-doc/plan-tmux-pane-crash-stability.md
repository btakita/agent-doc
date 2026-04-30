# Plan — stabilize repeated tmux pane crashes during `agent-doc` sessions

**Pending:** `#tmuxcrash`
**Status:** Draft

## Problem

Multiple April 29, 2026 reports still surface to the user as "the tmux pane crashed", but the persisted evidence shows a mixed failure family:

- `tasks/software/tsift.md`
  - `.agent-doc/logs/tsift-v0.1.log` records `session_start file=tasks/software/tsift.md pane=%411 session=tsift-v0` with no matching `codex_exit` or `session_end`
  - `.agent-doc/logs/ops.log` records `route_cycle_start_missing file=tasks/software/tsift.md pane=%411 harness=codex marker=prompt_target: do #afyn. spec-test-build-install-commit-push`
  - `.agent-doc/state/startup-miss/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c.json` still exists
- `tasks/agent-doc/agent-doc-bugs2.md`
  - `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log` records `session_start ... pane=%412` with no matching `codex_exit` or `session_end`
  - `.agent-doc/logs/ops.log` records `route_cycle_start_missing file=tasks/agent-doc/agent-doc-bugs2.md pane=%407 harness=codex marker=prompt_target: do #xfrr. spec-test-build-install-commit-push`
  - `.agent-doc/state/startup-miss/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0.json` still exists

So the bug is broader than one literal tmux pane death. The current system can leave behind fresh-start panes, startup-miss markers, and supervisor logs that never reach a clean exit boundary, while the user only sees "the pane crashed".

Source: `tasks/software/tsift.md`

- [ ] Reconstruct the three known crash episodes from persisted evidence before changing behavior:
  the original `tsift.md` crash, the earlier `claudescore-3.md` failed-resume crash family, and the new `claudescore-3.md` report where the tmux pane died while a Codex turn was still running. For each one, capture the document session UUID, `.agent-doc/logs/<session>.log`, supervisor exit reason, pane id, tmux server/session/window ids, and any crash text emitted by the child Codex process.
- [ ] Distinguish the failure boundary precisely:
  determine whether the pane died because the child harness exited, the `agent-doc start` supervisor exited or panicked, tmux killed/replaced the pane, or route/sync recovery detached the live pane and left the user looking at a dead shell.
- [ ] Audit every crash-sensitive path in `src/agent-doc` that can terminate or replace the active pane:
  `start.rs` supervisor restart/quit flow, rescue/restart after clean and non-clean exits, `route.rs` pane replacement, `sync.rs` stash/rescue reconciliation, and any stop-hook or IPC path that can abort the session after a completed turn.
- [ ] Add explicit crash provenance logging:
  for every pane death or restart decision, record the owning document, session UUID, pane id, exit code/signal, restart policy chosen, and the exact reason a pane was replaced, rescued, or abandoned.
- [ ] Define a stricter fail-closed invariant for repeated crashes:
  if the same document pane exits unexpectedly more than once inside a short window, `agent-doc` should stop auto-restarting blindly and surface a stable diagnostic state instead of cascading more tmux churn.
- [ ] Add a manual and automated reproducer for the suspected crash classes:
  interrupted turn, clean-exit auto-resume, supervisor panic, tmux-pane replacement during sync, stop-hook closeout after an aborted turn, and child/pane death during an in-flight Codex turn.
- [ ] Add regression coverage for:
  repeated unexpected child exits in the same pane, crash-loop backoff / diagnostic mode, route/sync not swapping out a live pane during recovery, and post-crash session ownership staying attached to the correct document.
- [ ] Verify the fix from a real tmux-backed Codex session on this repo before closing the item, because this bug sits at the boundary between tmux state, supervisor state, and live harness behavior.
