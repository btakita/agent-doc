# Plan — prevent unexpected tmux session spawn and live pane eviction

Source: `tasks/agent-doc/agent-doc-bugs2.md`

- [ ] Reconstruct the exact failure from logs and registry state: capture the invoking pane/session, configured `tmux_session`, resolved target session, pre-existing `sessions.json` entry, and every tmux mutation (`split-window`, `new-window`, `join-pane`, `move-window`, `kill-pane`, `kill-window`) that happened during the bad run.
- [ ] Audit every path in `src/agent-doc` that can create a fresh tmux location:
  `route::resolve_target_session`, `route::auto_start_in_session`, `sync` provisioning, terminal launch, and any fallback to harness defaults such as `"codex"` when no explicit live session is available.
- [ ] Audit every path that can destroy or displace an existing pane/window:
  route orphan cleanup, stash-pane eviction, wrong-session repair, stash-window purge, unregistered-stash purge, and sync layout repair.
- [ ] Define a stricter invariant for session targeting:
  if a document already has a live registered pane, `agent-doc` must route there or fail closed; it must not silently fall back to a different tmux session name and spawn a parallel session.
- [ ] Define a stricter invariant for cleanup:
  `agent-doc` may only kill panes/windows it can prove are stale throwaway artifacts from the current recovery path, never a live agent pane merely because it is in the wrong session/window.
- [ ] Tighten target-session resolution so auto-detect prefers the caller's real attached tmux session, refuses stale configured-session drift more explicitly, and does not create a new `"codex"` or `"claude"` session unless that creation was an intentional explicit target.
- [ ] Replace destructive wrong-session handling with non-destructive recovery where possible:
  deregister stale ownership, re-route to the live pane, or move/join the pane into the target session when safety checks pass.
- [ ] Add explicit provenance logging for pane lifecycle decisions:
  log why a new session/pane was spawned, why an existing pane was rejected, and why any kill/join/swap decision was considered safe.
- [ ] Add regression coverage for:
  - live registered pane in another session must not trigger silent parallel-session spawn
  - stale configured `tmux_session` must not fall through to an unintended harness-default session
  - wrong-session live Codex pane must survive repair or produce a fail-closed error
  - stash cleanup must not kill an active agent pane
  - fresh auto-start orphan cleanup must kill only panes created by the failed attempt
- [ ] Run targeted tmux/manual verification from a real Codex-backed session before closing the item, because this bug sits at the boundary between registry state, tmux state, and live harness behavior.
