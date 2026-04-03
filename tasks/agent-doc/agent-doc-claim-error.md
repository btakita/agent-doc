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
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260403-194619.md`*

Go
- [ ] Inline `max_lines=` attribute on component tags
### Re: Inline `max_lines=` attribute (HEAD)

Implemented `max_lines=N` as an inline attribute on component tags. Usage: `&lt;!-- agent:exchange patch=append max_lines=50 --&gt;` trims content to the last 50 lines after patching.

**Changes:**
- `template.rs`: Added post-patch processing — checks each component for `max_lines` (inline attr > `components.toml` > unlimited). Uses `limit_lines()` to keep last N lines.
- `patch.rs`: Added `max_lines` field to `ComponentConfig` for `components.toml` support.
- Both inline attrs and config file work; inline takes precedence.

`make check` passes (clippy + all tests).
<!-- agent:boundary:afaa2e0e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] IPC reposition → FFI event
- [x] Inline `max_lines=` attribute on component tags
- [ ] Restart watch daemon (after FFI reposition is done)
<!-- /agent:pending -->
