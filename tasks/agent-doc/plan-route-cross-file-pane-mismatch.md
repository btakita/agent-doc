# Plan — route cross-file pane mismatch (#rtxf)

**Pending:** `#rtxf`
**Status:** Draft

## Problem

`route` dispatches a command for file A to a pane registered for file B when no registered pane exists for file A. The fallback path in `auto_start_in_session` calls `find_registered_pane_in_session`, which returns the first alive pane in the target tmux session regardless of which file it owns.

**Reproduction (2026-04-29):** User triggered route for `tasks/claudescore-3.md` (session-share submodule). All registered panes for that file were stashed/dead. Route fell through to `auto_start_in_session`, which called `find_registered_pane_in_session(tmux, "14", "")`. That function iterated the full registry and returned pane `%401`, which was alive and in session 14 — but registered for `tasks/monsterrodholders.md` in `src/boost-client/`. Route then sent the claudescore-3 prompt to pane %401, which was already running a monsterrodholders session. The prompt was "accepted" (tmux send-keys succeeded) but no new document cycle started, so route eventually errored with `routed codex trigger ... was accepted ... but no new document cycle started`.

Secondary issue: three stale panes (%398, %404, %405) accumulated from prior failed route/start attempts. `resync` logged them as "unregistered — skipping kill (may be rescuable)" but did not clean them, allowing orphan buildup across retries.

## Root Cause

`find_registered_pane_in_session` (route.rs ~line 1268) iterates **all** registry entries and returns the first alive pane in the target tmux session. It does not filter by file path or session ID. This function is used as a split-target finder during `auto_start_in_session` (line 1530-1532), but the returned pane is then treated as the dispatch target — not just a layout anchor.

```rust
fn find_registered_pane_in_session(
    tmux: &Tmux,
    session_name: &str,
    exclude_pane: &str,
) -> Option<String> {
    let registry = sessions::load().ok()?;
    for entry in registry.values() {      // iterates ALL entries
        // ... alive check, session check ...
        return Some(entry.pane.clone());   // first match, any file
    }
    None
}
```

The test at line 3344 (`find_registered_pane_filters_by_session`) only verifies session filtering, not file filtering.

## Desired Behavior

1. **Route must never dispatch to a pane registered for a different file.** If no pane is registered for the target file, route should either start a new pane or fail closed — never reuse an unrelated pane.

2. **`find_registered_pane_in_session` should be split-target only.** When used to find a layout anchor for `split-window`, returning any alive pane in the session is fine. But the dispatch target must always be file-verified.

3. **Stale pane cleanup should be more aggressive.** Unregistered panes running `agent-doc` that have been stashed should be killed during `resync --fix`, not just logged as "may be rescuable" indefinitely.

## Implementation

### Step 1 — Separate split-target from dispatch-target in auto_start_in_session

In `auto_start_in_session` (~line 1480), the return value of `find_registered_pane_in_session` is used both as a split anchor and as a dispatch target. These must be decoupled:

- **Split anchor:** any alive pane in the session (current behavior is fine for layout)
- **Dispatch target:** must be a newly created pane or a pane verified to own the target file

After `split-window` creates a new pane, route should dispatch to that **new** pane, not to the anchor pane.

### Step 2 — Add file-match guard to route dispatch

Before sending keys to any pane, verify the pane is registered for the target file path:

```rust
// In the dispatch path, after resolving the target pane:
if let Some(registered_file) = sessions::file_for_pane(&pane_id) {
    if registered_file != target_file {
        return Err(anyhow!(
            "pane {} is registered for {}, not {}; refusing cross-file dispatch",
            pane_id, registered_file, target_file
        ));
    }
}
```

### Step 3 — Kill stashed unregistered panes in resync --fix

In `resync.rs`, change the "unregistered — skipping kill (may be rescuable)" path to kill stashed panes that have been unregistered for more than one resync cycle (or immediately when `--fix` is passed). Orphan accumulation across retries is the secondary symptom here.

### Step 4 — Add regression tests

- Test that `find_registered_pane_in_session` result is never used as a dispatch target for a different file
- Test that route fails closed when no file-matched pane exists and auto-start cannot create one
- Test that `resync --fix` kills stashed unregistered panes

## Verification

- `cargo test` — all existing + new tests pass
- Manual repro: register two files in the same tmux session, kill one file's pane, trigger route for the killed file — should fail closed or start a new pane, never dispatch to the other file's pane
