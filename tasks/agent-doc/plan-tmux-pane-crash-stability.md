# Plan — stabilize repeated tmux pane crashes during `agent-doc` sessions

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
