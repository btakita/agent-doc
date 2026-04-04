---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.6 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 12:43) (HEAD)

**Release: agent-doc v0.31.6→v0.31.9 + tmux-router v0.3.9**

**Bugs fixed (8 from prior + 5 this session = 13 total):**
Prior session (compacted): Console output spam, boundary marker stripping, stashed panes can't return, claim not atomic, verbose git status, window closure guard, navigation swap, pre-response cleanup.

This session:
1. Double-sync per editor event — removed redundant `sync_after_claim` and `sync::run_layout_only` from route
2. Stash-bounce loop — `return_stashed_panes_bulk` in prune() pulled stashed panes back every sync cycle; removed from auto path
3. Concurrent sync race — added `flock` on `.agent-doc/sync.lock` to serialize sync calls
4. CI path dep failure — removed `path = "../tmux-router"` from Cargo.toml (v0.31.8)
5. Clippy errors — 4 unused functions, 6 collapsible ifs (v0.31.9)

**Features implemented (11 from prior + 3 this session):**
Prior: Debounce, structured logging, lock cleanup, gc, auto-gc, cleanup runbook, tracing, source annotations, health check, route cleanup, runbook convention.

This session:
1. Diagnostic checkpoint logging in `sync_with_options`
2. `.cargo/config.toml` patch override pattern (gitignored local dev, CI uses crates.io)
3. `make release` target + CI workflow improvements (idempotent, version-sync check)

**Decisions:**
- JB plugin's `EditorTabSyncListener` is sole layout sync authority (route does pane resolution only)
- Stashed panes stay in stash until reconciler explicitly wants them (no auto-return)
- Cargo path deps: `.cargo/config.toml` with `[patch.crates-io]` (gitignored), not workspace or CI checkout
- IPC timeout → disk fallback can cause duplicate writes (known edge case, CRDT handles it)

**Releases:**
- v0.31.7: stash-bounce fix, sync lock, route sync removal, checkpoint logging
- v0.31.8: CI path dep fix
- v0.31.9: clippy fixes, `.cargo/config.toml` setup, CI workflow improvements

**Current state:** v0.31.9 released. No pending release (4 infra-only commits since tag). 634 tests pass.

**Test suite: 634 tests (193 lib + 403 bin + 38 integration)**
<!-- agent:boundary:87ea640d -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Clippy fixes pushed
- [x] No pending release (infra-only changes since v0.31.9)
<!-- /agent:pending -->
