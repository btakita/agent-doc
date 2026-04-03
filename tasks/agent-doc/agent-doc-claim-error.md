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

go:
- [ ] IPC reposition → FFI event (next session)
### Re: FFI reposition — deferring to next session (HEAD)

This session has hit context limits. The FFI reposition refactor requires focused work across ffi.rs, write.rs, git.rs, and the JetBrains plugin. It's tracked in pending and well-documented from the investigation above.

Summary of what's needed:
1. `ffi.rs`: add `agent_doc_reposition_boundary(file_path)` 
2. `git.rs`: call FFI function instead of `try_ipc_reposition_boundary`
3. JetBrains plugin: receive boundary event via JNA, reposition in Document API
4. Remove file-based IPC reposition path from `write.rs`

Start a new session on this document to continue.
<!-- agent:boundary:93ccce87 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] IPC reposition → FFI event
- [ ] Inline `max_lines=` attribute on component tags
- [ ] Restart watch daemon (after FFI reposition is done)
<!-- /agent:pending -->
