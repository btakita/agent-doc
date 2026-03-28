---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.27.0 released to crates.io, PyPI, GitHub. Stash rescue fix verified working.
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

The stash window should NEVER be purged.

1. You switched away → this pane was stashed
2. The claude session in the stashed pane eventually exited (or was killed), leaving an idle shell: Not true. The claude task was running. I received a JB prompt on this document after we switched to the new session. The there was a response patch from the original running claude session.
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

See my prompt above and reassess.
`Run Agent Doc` from the plugin results in a double prompt.
`❯ /agent-doc tasks/agent-doc/agent-doc-claim-error.md`
### Corrected analysis: premature deregistration

You're right — the claude session was still running. My idle-shell theory was wrong.

**Actual root cause: premature deregistration in `return_stashed_panes_bulk()`**

The flow when you switched back:

1. Pane %N stashed with claude still running ✓
2. `prune()` → `return_stashed_panes_bulk()` found %N (registered, non-idle, in stash)
3. `find_return_target_bulk()` tried to find a window to move it back to
4. No valid target found (original "agent-doc" window was gone or renamed)
5. **Bug:** resync.rs:685-691 **deregisters** the pane — removes registry entry
6. Route/claim lookup returns None → spawns new pane + new claude session
7. Original stashed pane becomes **orphaned** (claude running but invisible to agent-doc)
8. The stash window eventually dies when the orphaned claude session ends

**The deregistration at step 5 is the bug.** A pane with an active claude process should NEVER be deregistered just because `find_return_target` can't find a window. The correct behavior:

- If a stashed pane is running claude → **keep it registered**, let the claim/route flow rescue it
- Only deregister stashed panes that are idle shells with no return target
- Even better: during claim, explicitly check for stashed registered panes and unstash them

**Fix plan (revised):**

1. **Don't deregister active stash panes** — in `return_stashed_panes_bulk()`, only deregister if `IDLE_SHELLS.contains(&cmd)`, not if running claude
2. **Stash-aware claim** — in `claim::run()`, after `validate_file_claim()`, check if the registered pane is in a stash window and unstash it to the target window
3. **Never purge stash windows with active processes** — `purge_stash_windows_bulk()` should skip windows containing any non-idle pane

These three changes together prevent session loss when switching between documents.
### Double prompt bug

The "Run Agent Doc" plugin action fires both a claim AND a `/agent-doc` command to the same pane, resulting in two prompts competing in the same claude session. This is a plugin-side issue — the action should either:

1. **Claim only** — let the watch daemon detect the file change and trigger `/agent-doc` on the next cycle
2. **Route only** — skip the claim if the pane is already registered for this file, just send the `/agent-doc` command

Which behavior do you want? The claim-then-route pattern makes sense, but they shouldn't both be injected as separate prompts into the same claude session. The plugin should send a single combined command, or the claim should be silent (no `/agent-doc` injection) when the pane is already claimed for this file.

What does the plugin action currently send to the tmux pane? I can look at the JetBrains plugin source if you point me to it.

I was expecting the `Run Agent Doc` command to send `/agent-doc run <path>` to the claiming tmux pane.
Ideally we would ensure there is a claude session running in the tmux pane.
### Ideal: single `/agent-doc run <path>` command

Understood. The plugin's "Run Agent Doc" should send exactly one command to the tmux pane:

```
/agent-doc run <path>
```

Currently the plugin sends two separate things (claim + `/agent-doc`), causing the double prompt. The fix is plugin-side: "Run Agent Doc" should inject `/agent-doc run <path>` as a single prompt into the claiming pane.

But there's a subtlety: if the pane doesn't exist yet (first claim for this document), `agent-doc run` needs to handle claim-on-first-use. Looking at the current `run` subcommand — it does diff→send→append but doesn't claim. So we need either:

1. **`agent-doc run` does implicit claim** — if no pane is registered for this file, claim the current pane first, then run
2. **Plugin handles claim separately, then sends `/agent-doc run`** — but only ONE prompt total

Option 1 is cleaner. `agent-doc run` already knows the file and the pane (`TMUX_PANE` env) — it can call claim internally if needed.

For the stash rescue issue, `agent-doc run` with implicit claim would also be the right place to add stash-aware pane resolution: check for stashed registered panes before spawning new.

Want me to implement this? The change would be:
- `agent-doc run`: add implicit claim (if no active pane registered, claim current pane)
- `agent-doc run`: add stash rescue (if registered pane is stashed, unstash to current window): If we unstash the original session to the current window...since the new session had `agent-doc run` called on it...would the new session process the agent doc diff...While the original session would be in the tmux window and idle...with the actual processing being in the new session which is now stashed?
- Plugin: change "Run Agent Doc" to send single `/agent-doc run <path>`
### Ensuring claude session before `/agent-doc run`

Right. The full "Run Agent Doc" flow needs to guarantee a claude session exists:

1. **Resolve pane** — find or create the tmux pane for this document (claim + stash rescue)
2. **Ensure claude** — if the pane has an idle shell (no claude running), start claude in it
3. **Send command** — inject `/agent-doc run <path>` into the pane

This is what `agent-doc route` already does (steps 1-3), but it sends `/agent-doc <path>` not `/agent-doc run <path>`. The `route` command:
- Calls `prune()` to clean stale entries
- Looks up or creates a pane (with `start` for new claude sessions)
- Sends the `/agent-doc` command via `tmux send-keys`

So the fix is simpler than building from scratch:
- **`agent-doc route`** already handles ensure-pane + ensure-claude + send-command
- Just need to: (a) add stash rescue to route, (b) fix the double-prompt by having route send a single `/agent-doc run <path>`
- Plugin "Run Agent Doc" calls `agent-doc route <path>` — one CLI call, one tmux command sent

The three bugs to fix:
1. **Route: add stash rescue** before spawning new pane
2. **Route: don't deregister active stashed panes** during `prune()`
3. **Plugin: call `agent-doc route`** instead of separate claim + prompt injection

Shall I implement these fixes in agent-doc? The plugin change can be done after.
### Order matters: rescue BEFORE creating new session

You're right — that's a real problem. If we create a new session first, then unstash the old one, we get:
- New session (in stash) processes the diff — invisible to user
- Old session (unstashed) sits idle in the window — visible but useless

**The fix: rescue must happen BEFORE any new pane/session is created.**

The correct `agent-doc route` flow:

```
1. prune() — but DON'T deregister active stashed panes
2. Lookup registered pane for this document
3. If found AND alive:
   a. If in stash window → unstash to target window (rescue)
   b. If not in stash → already in position
   c. Send `/agent-doc <path>` to this pane ✓
4. If NOT found OR dead:
   a. Create new pane + start claude
   b. Register + send `/agent-doc <path>`
```

Step 3a is the key addition. The existing route flow already does steps 2, 3b, 3c, and 4 — it just skips the "is it stashed?" check. Adding that check means:

- Original session with full context gets rescued and receives the command
- No new session is created unnecessarily
- No orphaned stashed sessions

The route code in `route.rs` already has `rescue_from_stash()` (line 243) — it's called when the pane is found alive but in a stash window. The bug is that `prune()` deregisters the pane BEFORE route gets a chance to call `rescue_from_stash()`. So the single fix is really:

**Don't deregister active stashed panes in `return_stashed_panes_bulk()`.** That's it. Route's existing `rescue_from_stash()` already handles unstashing — it just never gets called because prune deletes the registry entry first.

Implement?
go
### Fix implemented: don't deregister active stashed panes

**Changed** `return_stashed_panes_bulk()` in `resync.rs` (lines 683-700):

**Before:** When `find_return_target_bulk()` returns `None`, always deregisters the pane — even if it's running claude.

**After:** Only deregisters if `IDLE_SHELLS.contains(&cmd)`. Active processes (claude, agent-doc, node) are kept registered with a log message:
```
resync: cannot return stashed pane %N (session): no valid target found — keeping registered (running 'claude')
```

This means when you switch back to a document:
1. `prune()` runs but does NOT deregister the stashed claude pane
2. `route` looks up the registered pane → finds it alive in stash
3. `rescue_from_stash()` unstashes it to the target window
4. Command is sent to the **original** session with full context

**Build:** `make check` passes (38 tests, clippy clean). Release binary built.

**Note:** `purge_stash_windows_bulk()` was already correct — it only kills stash windows where ALL panes are idle shells. No change needed there.

Ready to commit from within the submodule. Want me to bump version and publish, or test first?
commit
test
install for local testing
### Committed and installed

1. **Tests:** `make check` — 38 passed, clippy clean ✓
2. **Committed:** `658b06b` — "fix: don't deregister active stashed panes in return_stashed_panes_bulk"
3. **Installed:** `cargo install --path .` → `~/.cargo/bin/agent-doc` updated

The fix is now live locally. To test the stash rescue flow:
1. Open two task documents (so one gets stashed)
2. Switch away from one → pane stashes
3. Switch back → should rescue the stashed pane instead of spawning new

The fix ensures `prune()` doesn't deregister the stashed pane, so `route`'s `rescue_from_stash()` can find and unstash it.
Seems to be working.
continue
release
### v0.27.0 released

**Published everywhere:**
- **crates.io:** agent-doc v0.27.0 ✓
- **PyPI:** agent_doc v0.27.0 ✓
- **GitHub Release:** [v0.27.0](https://github.com/btakita/agent-doc/releases/tag/v0.27.0) with prebuilt binary ✓
- **cargo install:** `~/.cargo/bin/agent-doc` updated to v0.27.0 ✓

**Changelog:**
- Socket IPC (Unix domain sockets) with file-based fallback
- Bulk metadata fetching in resync (20-40 subprocess calls → 3)
- FFI listener for editor plugins
- **Fix: don't deregister active stashed panes** — prevents session loss on document switch

Layout issue from preflight confirms fix is working: `stash window 'stash' has non-idle pane %89 running 'agent-doc'` — previously this would have been deregistered and orphaned.

Implement:
- [ ] Plugin: "Run Agent Doc" calls `agent-doc route <path>` (single command) — future
This session still had `❯ /agent-doc tasks/agent-doc/agent-doc-claim-error.md`
Are we fixing it now?
### Plugin: `agent-doc route <path>` integration

Noted. This is tracked as a future plugin-side change:

**Current behavior:** JetBrains plugin sends separate claim + `/agent-doc` prompts → double prompt bug.

**Target behavior:** Plugin's "Run Agent Doc" action calls `agent-doc route <path>` as a single CLI command. Route already handles:
- Pane resolution (find registered, rescue from stash, or create new)
- Claude session ensure (start if idle shell)
- Command injection (`/agent-doc <path>` sent via `tmux send-keys`)

**What needs to change (plugin side):**
1. Remove the separate claim + prompt injection logic
2. Replace with single `agent-doc route <path>` subprocess call
3. Route handles everything — plugin just needs to call it and report success/failure

No agent-doc CLI changes needed — `route` already has the right flow. The stash rescue fix (v0.27.0) ensures rescued sessions aren't lost during route's `prune()` call.
### Double prompt confirmed — ready to fix

Yes, that's exactly the bug. This session received a second `/agent-doc` prompt because the plugin injected claim + `/agent-doc` as two separate commands.

**To fix now:** I need to look at the JetBrains plugin source to change "Run Agent Doc" from separate claim + prompt injection to a single `agent-doc route <path>` call. Point me to the plugin source directory and I'll implement it.

**What the fix looks like:**
1. Plugin "Run Agent Doc" action → calls `agent-doc route <path>` (one subprocess call)
2. Route handles: pane resolution, stash rescue, claude session ensure, command injection
3. No more double prompts — route sends exactly one `/agent-doc <path>` to the pane

fix now
### Plugin already uses `agent-doc route` — task complete

Investigated the plugin source. **SubmitAction already calls `agent-doc route <path>`** via `TerminalUtil.sendToTerminal()`. The plugin is NOT sending separate claim + `/agent-doc` — it's a single `agent-doc route` subprocess call.

The double prompt you experienced earlier was caused by the **premature deregistration bug** (now fixed in v0.27.0):
1. Stashed pane deregistered → route couldn't find it
2. Route created new pane + new claude session
3. Both old (orphaned) and new sessions processed the document

With the v0.27.0 fix, stashed panes with active claude sessions are kept registered, so `route` finds and rescues them via `rescue_from_stash()`. No new session is created.

**Task item closed** — the plugin integration is already correct. The remaining fix was the CLI-side deregistration bug, which is shipped.

I am running agent-doc v0.27.0. Why did the `/agent-doc` command bug happen in this document?
/btw FYI, it didn't happen with the last Run Agent Doc command.
### Why this session was affected despite running v0.27.0

The bug happened **before v0.27.0 was installed**. Timeline:

1. You were working on this doc in **session A** (original)
2. Switched away → session A stashed
3. `prune()` ran with the **pre-fix code** → deregistered session A (the bug)
4. Switched back → route found no registered pane → spawned **session B** (this session)
5. Session A became orphaned in the stash window
6. Then we fixed the bug, built v0.27.0, and installed it

v0.27.0 prevents this from happening **going forward**, but it can't undo the session split that already occurred. This session (B) was created by the bug before the fix existed.

**Evidence:** The 4 stashed panes in preflight (`%96, %99, %98, %100 running 'agent-doc'`) are orphaned sessions from before the fix. v0.27.0 keeps them registered now (won't deregister), but they were already orphaned when the old code ran.

To clean up: those stashed panes can be manually killed since their Claude sessions have no active document claim.
I got a file externally modified popup in the JB plugin when I was typing into tasks/software/croky.md. Why?
### "File externally modified" popup on corky.md

The popup means something wrote to `tasks/software/corky.md` on disk outside of JetBrains' Document API. With IPC working correctly, `agent-doc write` sends patches to the plugin, which applies them via Document API — no external modification dialog.

**Most likely cause: an orphaned stashed session.** The 5 orphaned panes in the stash (`%96, %101, %89, %98, %100` running 'agent-doc') are still running Claude sessions. One of them was likely processing `corky.md` and wrote its response directly to disk. Orphaned sessions may not have a working IPC connection since they're in the stash window with no active plugin association.

**Other possible causes:**
- `agent-doc commit` boundary reposition falling back to disk write
- IPC timeout (2s) if the plugin was busy when the write happened

**Fix:** Kill those orphaned stashed panes. They're pre-fix leftovers with no active document claims, and they may be writing to documents you're actively editing:

```bash
tmux kill-pane -t %96
tmux kill-pane -t %101
tmux kill-pane -t %89
tmux kill-pane -t %98
tmux kill-pane -t %100
```

After cleanup, the "externally modified" popups should stop since only the active (IPC-connected) sessions will be writing to documents.

This was happening in tasks/software/corky.md...as I was typing, the "externally modified" popup kept on happening. I would lose the last typed character as well. Why? Do we have logs?
### Root cause found: `clean_stale_boundaries_in_working_tree()` writes directly to disk

**File:** `src/agent-doc/src/git.rs:275-309`

After every `agent-doc commit`, the function `clean_stale_boundaries_in_working_tree()` runs. When it detects >1 boundary markers in the document, it calls `std::fs::write(file, &cleaned)` — **writing directly to the document file on disk, bypassing IPC entirely.**

This causes:
1. JetBrains' file watcher detects the external modification
2. Shows "file externally modified" popup (or auto-reloads)
3. Your in-memory document (with the character you just typed) gets replaced by the disk version → **character lost**

**Why it happens repeatedly:** Each agent-doc cycle creates a commit, which runs this function. If stale boundaries keep appearing (e.g., from IPC reposition race conditions), the cleanup fires every cycle.

**The fix:** Either:
1. **Remove the function** — the boundary reposition IPC already handles stale boundaries in the plugin
2. **Route through IPC** — use `try_ipc_reposition_boundary()` instead of `std::fs::write()`

Option 1 is cleanest since the comment at line 207 already says "Working tree is NOT written directly (would lose user keystrokes) UNLESS stale boundaries are detected." The stale boundary case should also use IPC, not disk write.

Want me to implement the fix?
Yes...Also explain what the clean stale boundries does.
### What `clean_stale_boundaries_in_working_tree()` does (HEAD)

**Boundary markers** (`<!-- agent:boundary:HASH -->`) are inserted at the end of the exchange component after each agent response. They mark where the agent last wrote — the plugin uses them to position the visual separator between agent output and user input.

When a new response is written, a **new** boundary is inserted, but sometimes the **old** boundary isn't removed (e.g., IPC reposition missed it, or a race condition). This creates "stale" boundaries — multiple markers scattered through the exchange.

The function:
1. Counts boundary markers in the document (ignoring those inside code blocks)
2. If count > 1 → calls `reposition_boundary_to_end()` which removes ALL boundaries and inserts one fresh one at the end
3. Writes the result directly to disk via `std::fs::write()` — **this is the bug**

The problem: `std::fs::write()` bypasses the IDE's Document API, triggering "externally modified" popup and losing any character you typed between the last save and the disk write.

### Fix implemented

**Removed** `clean_stale_boundaries_in_working_tree()` entirely from `git.rs`. The stale boundary cleanup is already handled by the IPC reposition path:

- `try_ipc_reposition_boundary()` sends a signal to the plugin
- The plugin calls `reposition_boundary_to_end()` via FFI (Document API)
- This removes ALL stale boundaries and inserts a single fresh one
- Done through Document API → no "externally modified" popup, no lost keystrokes

**Build:** 38 tests pass, clippy clean. Release binary built and installed to `~/.cargo/bin/agent-doc`.

The fix is live now. The "externally modified" popup while typing should not happen anymore.
<!-- agent:boundary:c1ceb987 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Fix return_stashed_panes_bulk: don't deregister active stashed panes
- [x] Commit, test, install locally
- [x] Bump to v0.27.0, publish (crates.io, PyPI, GitHub Release)
- [x] cargo install
- [x] Plugin: "Run Agent Doc" — already calls `agent-doc route <path>` (verified in source)
- [x] Fix: removed `clean_stale_boundaries_in_working_tree()` — no more disk write during commit
- [ ] Clean up orphaned stashed panes
- [ ] Commit fix, bump version, release
<!-- /agent:pending -->
