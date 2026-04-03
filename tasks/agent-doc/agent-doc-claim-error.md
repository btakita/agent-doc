---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Sync auto-start bug fixed. Unclaimed files no longer spawn sessions on tab navigation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260403-193618.md`*

do:
> go:
> - [ ] IPC reposition → FFI event (next session)
### Re: IPC reposition → FFI event

Removed the file-based IPC fallback from `try_ipc_reposition_boundary` in `write.rs`. The function now uses socket IPC exclusively (through the FFI listener callback chain).

**What changed:**
- `write.rs:1244-1266`: Simplified from ~70 lines to ~20 lines
- Removed: JSON patch file creation, polling loop, timeout cleanup
- Kept: socket IPC via `ipc_socket::send_reposition` (delivers through FFI listener callback)
- Non-fatal on failure (same behavior as before)

**What didn't need to change:**
- `git.rs` already calls `try_ipc_reposition_boundary` correctly
- `ffi.rs` already has `agent_doc_reposition_boundary_to_end` for text transforms
- `ipc_socket.rs` socket listener already routes through FFI callback
- JetBrains plugin already handles `"reposition"` messages via the FFI callback

`make check` passes (clippy + all 608 tests).

Will this file editing workflow when we are not using a plugin?
Let's say we have an editor without a plugin with the agent document...how would it work?
### Re: No-plugin workflow

Without a plugin, the workflow still works — just with a minor cosmetic gap:

1. **`agent-doc write`** — writes directly to disk (IPC skipped when `.agent-doc/patches/` doesn't exist). Boundary marker placed correctly by write command. Editor picks up file change via native file watcher.

2. **`agent-doc commit`** — repositions boundary in snapshot (always works). `try_ipc_reposition_boundary` checks `is_listener_active()` → `false` → returns immediately. Working tree keeps old boundary position.

3. **Next cycle** — next `agent-doc write` places fresh boundary, stale one cleaned up.

**The gap:** Between commit and next write, the working-tree boundary is stale (old position). Cosmetic only — snapshot has correct position, diff uses snapshot. Gutter boundary briefly in wrong place, self-corrects on next response.

Same behavior as before removing file-based fallback — file IPC also required `.agent-doc/patches/` (plugin install). No regression for non-plugin editors.
<!-- agent:boundary:45e8b0be -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] IPC reposition → FFI event
- [ ] Inline `max_lines=` attribute on component tags
- [ ] Restart watch daemon (after FFI reposition is done)
<!-- /agent:pending -->
