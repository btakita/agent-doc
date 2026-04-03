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
### Re: IPC reposition → FFI event (HEAD)

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
<!-- agent:boundary:e78b240a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] IPC reposition → FFI event
- [ ] Inline `max_lines=` attribute on component tags
- [ ] Restart watch daemon (after FFI reposition is done)
<!-- /agent:pending -->
