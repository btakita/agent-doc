---
agent_doc_session: 89137ef6-5f05-44dc-a60d-ef56aba98eda
tmux_session: claude
---

# tmux-router

Extracted from `tasks/agent-doc.md` — tmux pane routing crate.

## Overview

`tmux-router` is a Rust crate (library + CLI) for declaratively routing files to tmux panes. It syncs an editor's split layout to tmux pane arrangement.

## CLI API

```
tmux-router sync --col a.md,b.md --col c.md [--window @1] [--focus a.md] [--session claude]
tmux-router register <file> <pane-id> [--session claude]
tmux-router unregister <file>
tmux-router status                    # show registry + pane health
tmux-router resync                    # prune dead panes from registry
tmux-router focus <file>              # select pane for file
```

## Design Decisions

| Decision | Choice |
|----------|--------|
| Registry | Own file: `.tmux-router/registry.json` |
| File identity | File-path-as-key (no UUID dependency) |
| Library API | `tmux_router::{Tmux, Registry, reconcile()}` |
| Config | `~/.config/tmux-router/config.toml` + CLI flags override |

## Architecture

### What moves to tmux-router

From `agent-doc/src/sessions.rs`:
- `Tmux` struct (tmux command abstraction)
- `SessionEntry` / `SessionRegistry` types
- `IsolatedTmux` test infrastructure
- Pane lifecycle: `pane_alive`, `new_session`, `new_window`, `send_keys`
- Stash window: `find_stash_window`, `ensure_stash_window`, `stash_pane`
- Query methods: `pane_window`, `pane_session`, `list_window_panes`, `list_panes_ordered`

From `agent-doc/src/sync.rs`:
- `Layout` / `Column` types
- `reconcile()` algorithm (attach-first: ATTACH → SELECT → DETACH → REORDER → VERIFY)
- `find_column_pane()`, `find_best_window()`, `join_target()`, `equalize_sizes()`
- `SyncLog` structured logging
- All 25+ integration tests and 3 proptest property-based tests

### What stays in agent-doc

- Frontmatter parsing (`agent_doc_session` → file path mapping)
- Snapshot system
- Agent backends (Claude, Junie)
- Skill/upgrade system
- `claim`/`focus`/`start` commands (thin wrappers calling tmux-router)

### Boundary

agent-doc maps `agent_doc_session` UUID → file path, then calls `tmux_router::sync()` with file paths. tmux-router handles everything from file-path-as-key onward.

## Key Lessons (from 7 iterations of sync reconciliation)

1. **Never break wanted panes** from the target window — scatter-gather destroys working state
2. **No registry dependency for eviction** — break ANY unwanted pane, not just tracked ones
3. **Re-query tmux state before each operation** — cached state goes stale instantly
4. **Attach before detach** — prevents pane selection flicker
5. **All join-pane calls use `-d`** — no intermediate selection changes during reconcile
6. **Same-column-only auto-register** — cross-column donors cause wrong focus
7. **Plugin must validate windows** — stale registry entries cause cascading failures

## Testing Strategy

**Dual-layer coverage — no test deletion during extraction:**

- **tmux-router**: All 25+ reconciliation integration tests + 3 proptests + unit tests move here. These test algorithm correctness independently.
- **agent-doc**: Keep integration tests for the full flow (frontmatter → path mapping → tmux-router). Add contract tests verifying correct API usage. Edge cases discovered in agent-doc get corresponding tests in tmux-router too.

Tests are copied/adapted, not just moved. Both crates must pass independently.

## Extraction Plan

1. ~~Create `tmux-router/` crate in agent-loop workspace (or standalone repo)~~ **Done** — `btakita/tmux-router` on GitHub
2. ~~Move `Tmux`, `Registry`, `reconcile()` + types~~ **Done** — 46 tests pass
3. Update agent-doc to `use tmux_router::*` — **Pending**
4. Ensure all tests pass in both crates — **Pending**
5. Release tmux-router v0.1.0, agent-doc v0.8.0 — **Pending**

## Status

**v0.1.0 extracted** (`4cba1c8`):
- `src/tmux.rs` — `Tmux` + `IsolatedTmux` with all methods
- `src/registry.rs` — `RegistryEntry`, `Registry`, load/save/lookup/prune (parameterized path)
- `src/sync.rs` — `Layout`, `reconcile()`, `sync()` with callback-based `FileResolution`
- 46 tests pass (unit + integration + proptest)
- Added as submodule to agent-loop

**Next:** Wire agent-doc to depend on tmux-router and delegate sync/sessions to the library.

## User

