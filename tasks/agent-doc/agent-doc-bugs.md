---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.10 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 15:00) (HEAD)

**Releases: agent-doc v0.31.6→v0.31.10 + tmux-router v0.3.9**

**Bugs fixed (13 from prior + 1 this session = 14 total):**
Prior (compacted): Console output spam, boundary marker stripping, stashed panes can't return, claim not atomic, verbose git status, window closure guard, navigation swap, pre-response cleanup, double-sync, stash-bounce loop, concurrent sync race, CI path dep, clippy errors.

This session:
1. Untracked files after import — granola import didn't commit; added `ensure_initialized()` to claim + preflight

**Features (14 from prior + 4 this session = 18 total):**
Prior: Debounce, structured logging, lock cleanup, gc, auto-gc, cleanup runbook, tracing, source annotations, health check, route cleanup, runbook convention, diagnostic checkpoints, cargo patch override, make release + CI improvements.

This session:
1. `ensure_initialized()` — shared auto-init for new documents (snapshot + git add + commit)
2. Cross-process typing detection (FFI: `is_typing_via_file`, `await_idle_via_file`)
3. Diff stability fix (stable_count across outer iterations)
4. IPC error propagation (no more swallowed failures)

**Granola skill updates:**
- Created `runbooks/enrich-from-transcript.md` — manual workflow for extracting Granola's hidden transcript context via text-selection popup
- Updated SKILL.md with runbook index

**Decisions:**
- Auto-init lives in existing entrypoints (claim, preflight), not a standalone `init` command
- Sync doesn't need init — it operates on layout/panes, not individual files
- Granola transcript popup is the primary way to access hidden meeting context

**Releases:**
- v0.31.7: stash-bounce fix, sync lock, route sync removal
- v0.31.8: CI path dep fix
- v0.31.9: clippy, cargo config, CI workflows
- v0.31.10: auto-init, cross-process typing, diff stability, IPC errors

**Current state:** v0.31.10 released (crates.io + GitHub + installed). All pending items complete. 637 tests pass.
<!-- agent:boundary:b4b6f655 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Granola enrich-from-transcript runbook — created
- [x] Auto-init: `ensure_initialized()` added to claim + preflight
- [x] Release v0.31.10 — published
<!-- /agent:pending -->
