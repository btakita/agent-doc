---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Bug filed — awaiting implementation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Session initialized. Write in the exchange to describe the claim error you're seeing.

### Bug: Claim delay (~20-30s) from redundant tmux subprocess calls

**Symptom:** Running "Claim for Tmux Pane" from JetBrains plugin shows errors and takes 20-30 seconds:
```
resync: cannot return stashed pane N72 (...) no valid target found
resync: cannot return stashed pane N82 (...) no valid target found
resync: cannot return stashed pane N85 (...) no valid target found
```

**Root cause:** The `prune()` call chain in `resync.rs` spawns ~20-40 synchronous tmux subprocesses (each ~200-500ms):
- `pane_alive()` in `registry.rs` runs `tmux list-panes -a` **per registry entry** instead of once
- `tmux list-windows -a` is called 3 separate times (once in each of `return_stashed_panes`, `purge_stash_windows`, `purge_unregistered_stash_panes`)
- Each `find_return_target()` failure triggers multiple subprocess calls before concluding "no valid target"
- The "cannot return" errors are harmless but expensive to compute

**Key files:**
- `src/agent-doc/src/resync.rs` — `prune()`, `return_stashed_panes`, `purge_*` functions
- `src/tmux-router/src/tmux.rs` — `pane_alive()`, `join_pane()`, `stash_pane()`
- `src/tmux-router/src/registry.rs` — `prune()` with per-entry `pane_alive` calls

**Fix plan:**
1. **Bulk `pane_alive` check**: One `tmux list-panes -a -F "#{pane_id}"` → HashSet, instead of N subprocess calls
2. **Share window listing**: Call `tmux list-windows -a` once, pass to all 3 prune functions
3. **Deregister stranded panes**: When `find_return_target()` fails, remove the entry so it doesn't retry every cycle
4. **Batch pane metadata queries**: Single `tmux list-panes -a -F` with all needed fields instead of per-pane `display-message` calls

**Expected improvement:** Claim time from ~20-30s down to <2s.

Implement

### Implementation complete (HEAD)

**Root cause:** `prune()` spawned ~20-40 synchronous tmux subprocesses (each ~200-500ms) because:
1. `registry::prune()` called `pane_alive()` per entry (`tmux list-panes -a` N times)
2. `list-windows -a` called 3 times (once per prune function)
3. Per-pane `pane_current_command()` and `pane_window_name()` each spawned a subprocess
4. Stranded panes retried `find_return_target()` every cycle without deregistering

**Fix:** Refactored `prune()` to fetch all metadata in 2 subprocess calls total, then pass to bulk variants:

1. **`fetch_all_window_metadata()`** -- single `tmux list-windows -a -F` with all needed fields
2. **`fetch_all_pane_metadata()`** -- single `tmux list-panes -a -F` with all needed fields
3. **`purge_stash_windows_bulk()`** -- uses pre-fetched metadata, no per-window subprocess calls
4. **`purge_unregistered_stash_panes_bulk()`** -- uses pre-fetched metadata, no per-pane subprocess calls
5. **`return_stashed_panes_bulk()`** -- uses pre-fetched metadata + deregisters stranded panes on first failure
6. **`find_return_target_bulk()`** -- looks up from pre-fetched maps instead of subprocess calls

**Subprocess reduction:** ~20-40 calls down to 3 total (1 `list-windows -a`, 1 `list-panes -a`, 1 `registry::prune`).

**Stranded pane fix:** `return_stashed_panes_bulk()` now deregisters panes when `find_return_target()` fails, preventing repeated expensive retries every cycle.

**Test results:** 560 tests pass, clippy clean. Release binary built.
<!-- agent:boundary:991bcd7b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Share `tmux list-windows -a` across all prune functions
- [x] Deregister stranded panes on first `find_return_target()` failure
- [x] Batch pane metadata queries via single `tmux list-panes -a -F`
- [ ] tmux-router 0.4: Add `alive_pane_ids()` bulk method, update dep
<!-- /agent:pending -->
