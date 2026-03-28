---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Investigating stash-aware claim flow — fix spec ready, awaiting implementation decision.
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

### Implementation complete

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
continue:
- [ ] tmux-router 0.4: Add `alive_pane_ids()` bulk method, update dep

### tmux-router 0.3.1 published

**Done.** Added `alive_pane_ids()` bulk method to tmux-router and published v0.3.1.

**Changes:**
- `tmux.rs`: Added `alive_pane_ids()` -- single `tmux list-panes -a -F #{pane_id}` returning `HashSet<String>` for O(1) lookup
- `registry.rs`: `prune()` now uses `alive_pane_ids()` instead of per-entry `pane_alive()` calls
- Published to crates.io as tmux-router v0.3.1
- agent-doc updated lockfile to use v0.3.1 -- 560 tests pass, release binary built

**Total subprocess reduction across the full claim flow:**
- Before: ~20-40 tmux subprocess calls
- After: 3 total (1 `list-panes -a` in registry prune, 1 `list-windows -a` in resync, 1 `list-panes -a` in resync)

All 4 optimizations complete.

Update agent-doc.
audit docs
update README.md, SPEC.md, ontology, module-harness

I switched back to this document.
There was a running task in another task. But when I switched back, agent-doc created a new tmux pane -> claude session. Why?

### agent-doc updated, docs audited

**Committed and pushed** `66335e2` — v0.27.0: Socket IPC, bulk resync, FFI listener.

**Documentation updates:**
- **README.md** — added socket IPC and bulk resync to key features
- **SPEC.md** — added section 6.3 (Socket IPC protocol: NDJSON message types, fallback behavior), updated resync section with bulk metadata fetching details
- **CLAUDE.md** — added `ipc_socket.rs` to module layout, Shared Foundation pattern with ontology

**Audit results:** 1 pre-existing warning (large table in CLAUDE.md). All stale-doc issues resolved.

**tmux-router 0.3.1** also published (bulk `alive_pane_ids()` in registry prune).

Ready for version bump and release.
### Why switching back created a new session

**What happened:**

1. When you switched away to another task document, agent-doc **stashed** this pane (moved it to the stash window to free up the layout)
2. When you switched back, the JetBrains plugin fired a **new claim** for `agent-doc-claim-error.md`
3. The claim flow saw no active (unstashed) pane for this document, so it **spawned a new tmux pane + new claude session**
4. The old stashed pane with the previous session was still sitting in the stash window (or got pruned)

I don't see the stash window in the tmux session. Why?

**Root cause:** The claim path doesn't check for existing stashed panes that could be **unstashed and resumed**. The `return_stashed_panes` logic exists in `prune()` (resync), but that's a background process — it doesn't run inline during the claim flow.

**Potential fix:** During claim, before spawning a new pane:
1. Check if there's a stashed pane already registered for this document
2. If found, unstash it back to the target window instead of spawning new
3. Only spawn new if no stashed pane exists

This would preserve the existing claude session context instead of starting fresh each time you switch between documents.

Want me to look at the claim flow code and spec a fix?
Yes
### Stash window not visible + claim flow analysis

**Why no stash window:** The stash window was **already purged**. `purge_stash_windows_bulk()` (resync.rs:556) kills stash windows when all panes inside are idle shells (zsh/bash) and the window is >30 seconds old. Timeline:

1. You switched away → this pane was stashed
2. The claude session in the stashed pane eventually exited (or was killed), leaving an idle shell
3. A subsequent `prune()` call (from another claim or route) ran `purge_stash_windows_bulk()`
4. All panes idle + >30s → stash window killed → pane destroyed
5. When you switched back, `claim::run()` found no registered pane → spawned new

**Claim flow analysis** (claim.rs:83-258):

The claim flow does call `prune()` at line 84 (which runs `return_stashed_panes_bulk()`), but there are two gaps:

1. **Timing gap:** `return_stashed_panes_bulk()` only returns *non-idle* panes. If the claude process exited and the shell went idle, the pane is skipped for return — and then killed by the subsequent purge step.

2. **No stash-aware claim resolution:** After `prune()`, the claim resolves a pane via priority: `--pane` > `--position` > `TMUX_PANE` > active pane. None of these paths check if a stashed pane still exists for this file's session. `validate_file_claim()` (line 87) only removes dead-pane entries — it doesn't attempt to rescue stashed panes.

**Fix spec — stash-aware claim:**

In `claim::run()`, after `prune()` and `validate_file_claim()` but before pane resolution:

```rust
// After line 87 in claim.rs
// Check if there's a stashed pane for this file that can be rescued
if let Some(entry) = sessions::find_by_file(&registry_path, &canonical_file) {
    if is_pane_in_stash(&entry.pane_id, &all_pane_metadata) {
        // Pane exists in stash — unstash it to target window
        if let Ok(()) = tmux::join_pane(&entry.pane_id, &target_window) {
            // Successfully rescued — use this pane instead of spawning new
            return Ok(entry.pane_id);
        }
    }
}
```

This adds ~1 check (O(1) lookup in pre-fetched metadata) before the spawn path.

**Alternative approach:** Keep stashed panes with active claude sessions alive longer — exempt non-idle stash panes from the 30s grace period entirely. This would mean the stash window persists as long as claude is running, giving `return_stashed_panes_bulk()` time to work.

Want me to implement option 1 (stash-aware claim), option 2 (longer stash retention), or both?
<!-- agent:boundary:6797b886 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement stash-aware claim resolution (rescue stashed panes before spawning new)
- [ ] Consider longer stash retention for non-idle panes
- [ ] Bump agent-doc version to 0.27.0 in Cargo.toml + pyproject.toml
- [ ] Publish to crates.io, PyPI, GitHub Releases
<!-- /agent:pending -->
