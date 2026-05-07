# Plan — completed runs must not stash or remove the tmux pane from the `agent-doc` window

**Pending:** `#jbpdrop`
**Status:** Verified by regression replay on 2026-05-05; literal editor click-through still optional

## Problem

User reports now cover more than the original JetBrains-only shape:

- the run completed
- immediately after completion, or shortly afterward without user action, the tmux pane was removed from the `agent-doc` tmux window or stashed out of view

The latest concrete repro is the `tasks/agent-doc/agent-doc-bugs2.md` Codex pane:

- `session_start file=tasks/agent-doc/agent-doc-bugs2.md pane=%169` at `1777861328`
- repeated `commit_success` and `route_dispatch_only_sent` events on `%169`
- no later `session_end`, `supervisor_exit`, or `session_superseded` entry for `%169` in the session log after that start

That makes this repro narrower than generic supervisor exit handling: the pane likely remained the authoritative owner and was later relocated by sync/layout reconciliation rather than being legitimately closed by the run-ending path.

This is a distinct failure from `#jbptrk`.

- `#jbptrk` is about startup/restart losing track of a still-visible pane and replacing it
- this new bug is about a *successful or apparently successful* run mutating tmux layout by removing the pane from the `agent-doc` window

The invariant should be stricter here: completing a run must not make the pane disappear from the `agent-doc` window unless agent-doc is performing one of the very few explicitly destructive pane-management operations such as duplicate-pane reaping.

## Desired Behavior

1. A normal completed run leaves its pane present in the `agent-doc` window.
2. If the harness process exits, tmux state must still remain diagnosable and non-destructive:
   - either the pane stays visible for recovery/inspection
   - or agent-doc records and explains the exact deliberate cleanup path
3. Editor/plugin automation and tmux reconciliation must not treat a completed run as permission to reap, relocate, stash, or supersede that pane.
4. Pane-closing behavior must be limited to a very small, explicitly-audited set of operations.

## Likely Root-Cause Surface

- `sync.rs`
  - `capture_dead_pane_diagnostics(...)`
  - `repair_missing_registered_pane(...)`
  - any path that records `registered_pane_dead` / `registered_pane_missing`
  - any post-completion cleanup path that calls `kill_pane`, recreates the `agent-doc` window, or stashes a still-authoritative pane during reconcile
- `route.rs`
  - fresh-restart / `respawn-pane -k` paths that may reap the current pane after a completed cycle
  - stash rescue / join-pane paths that may leave the window without the user-visible pane
- editor/plugin automation
  - `PluginLifecycleListener`
  - `EditorTabSyncListener`
  - `LayoutChangeDetector`
  - any post-run focus/layout event that triggers `sync --no-autostart` and mutates tmux after completion
- tmux harness policy
  - whether the pane is expected to survive process exit
  - whether the agent-doc window relies on tmux defaults that remove panes/windows on exit instead of retaining them for recovery

## Implementation Steps

- [ ] Reproduce the exact post-completion disappearance or stash with tmux/log evidence:
  - pane id before run
  - window membership before run
  - session log events during completion
  - tmux pane/window state immediately after completion
  - the first reconcile/layout event that moves the pane out of `agent-doc`
- [ ] Freeze the current `agent-doc-bugs2.md` repro as the primary regression seed:
  - `session_start ... pane=%169`
  - repeated `commit_success` / `route_dispatch_only_sent`
  - no `session_end` / `supervisor_exit` / `session_superseded` for `%169`
  - later stash detected by the user and recoverable via `agent-doc sync`
- [ ] Audit every code path that can remove or effectively remove the pane after a run ends:
  - explicit `kill-pane`
  - `respawn-pane -k`
  - `kill-window`
  - `join-pane` / rescue / reconcile paths that relocate the pane out of the `agent-doc` window
  - any session rebind that leaves the original pane untracked and then cleaned up
- [ ] Decide and document the completion invariant:
  - whether completed panes must remain visible in-place
  - whether dead panes should be retained for diagnosis until explicit cleanup
  - which exact operations are allowed to close panes
- [ ] Harden sync/route against post-success cleanup:
  - do not classify a just-completed pane as disposable from weak evidence
  - do not let plugin-driven `sync`/layout events reap or stash the only pane for a document just because the run ended
  - if ownership is ambiguous, fail closed and log instead of mutating tmux
- [ ] Narrow destructive tmux actions behind explicit guards:
  - duplicate-pane reap only
  - proven dead-pane cleanup only after stronger evidence
  - no generic "completion implies cleanup" behavior
- [ ] Improve diagnostics:
  - record the reason whenever a pane leaves the `agent-doc` window
  - log source path: route, sync, resync, tmux-router reconcile, plugin startup, plugin layout sync, duplicate reap, or manual recovery
  - capture before/after window membership for the affected pane
- [ ] Add regression coverage for:
  - successful completed run keeps pane in `agent-doc` window
  - plugin-triggered post-run sync does not stash or remove the pane
  - a no-user-action reconcile cannot move the sole authoritative completed pane into `stash`
  - a completed pane is not reaped unless it matches an allowed destructive path

## Replay Result

The current `src/agent-doc` branch already contains the route/sync fixes that cover the
`#jbpdrop` pane-retention family. This cycle verified those fixes instead of introducing a new
source delta.

What the regression replay proved:

- post-claim sync stays on the injected tmux server instead of mutating the default live server
- dispatch-only reroutes fail closed on cross-file pane rebind attempts instead of transiently
  stealing another document's pane
- starting authoritative panes keep the live direct-submit path during boot instead of falling
  back to supervisor IPC or a stale pane
- fresh-start reroutes keep the newly-created pane authoritative even when another same-session
  registration arrives during boot

Verification run on 2026-05-05:

- `cargo test --manifest-path src/agent-doc/Cargo.toml sync_after_claim_stays_on_injected_tmux_server -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml register_dispatch_target_rejects_cross_file_rebind_and_preserves_registry -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatch_only_keeps_starting_authoritative_actor_on_direct_pane_submit -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_keeps_fresh_start_authoritative_despite_existing_owner_rebind -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Remaining boundary:

- I did not run a literal JetBrains/VS Code GUI click-through in this cycle.
- The route/tmux regressions that model the original mixed-root pane-theft sequence are green, so
  any further replay is only a human/editor validation gate rather than a code-change blocker.

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/src/resync.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/editors/vscode/SPEC.md`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/LayoutChangeDetector.kt`
- `src/agent-doc/editors/vscode/src/extension.ts`

## Exit Criteria

- completing a JetBrains-, VS Code-, or Codex-routed run no longer removes or stashes the pane from the `agent-doc` window without explicit user action
- any pane removal that still occurs is limited to a short explicit allowlist of destructive operations
- logs clearly state why a pane left the window and which subsystem initiated it
- isolated tmux regression coverage proves post-completion pane retention across the route/sync/plugin boundary
