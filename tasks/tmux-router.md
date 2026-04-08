---
agent_doc_session: 89137ef6-5f05-44dc-a60d-ef56aba98eda
tmux_session: '0'
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
tmux-router send <target> "command"   # send to pane by file name or %id
tmux-router capture <target>          # read pane content
```

## Design Decisions

| Decision | Choice |
|----------|--------|
| Registry | Own file: `.tmux-router/registry.json` |
| File identity | File-path-as-key (no UUID dependency) |
| Library API | `tmux_router::{Tmux, Registry, reconcile()}` |
| Config | `~/.config/tmux-router/config.toml` + CLI flags override |
| Concurrency | `flock`-based advisory locking via `fs2` crate (cross-platform) |
| `register` vs `claim` | `register` is tmux-router's file-path-based binding; `claim` is agent-doc's UUID-based concept that calls `register` under the hood |

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
3. ~~Update agent-doc to `use tmux_router::*`~~ **Done** — agent-doc v0.9.4+
4. ~~Ensure all tests pass in both crates~~ **Done** — 48 tmux-router, 150 agent-doc
5. ~~Release tmux-router v0.1.0, agent-doc v0.8.0~~ **Done** — see releases below

## Session Log (compacted)

### CLI Implementation (v0.2.0)

tmux-router was library-only. CLI built with clap providing 8 subcommands: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`.

**Value over stock `tmux send-keys`**: (1) Registry-aware addressing -- send by file name, not pane ID, (2) Literal-by-default -- tmux interprets special keys; tmux-router defaults to `-l`, (3) Auto Enter with 50ms delay.

**File-name addressing**: `resolve_target()` checks if target starts with `%` (raw pane ID) or looks up in registry. Library additions: `capture_pane()` (screen buffer via `tmux capture-pane -p`), `send_keys_raw()` (raw tmux key names).

**Core pattern**: File changes -> program in bound pane reacts. tmux-router manages the binding and layout. Use cases: agent loops, test runners, log watchers, REPL-driven dev, database consoles, multi-agent orchestration, build monitors. README documented with 6 use cases.

**tmux-router vs raw tmux**: Semantic addressing (file names not pane IDs), layout reconciliation, registry persistence, health management (dead pane pruning).

### Bug: Lost Write-back on Concurrent Documents

When two `/agent-doc` invocations arrived in quick succession, the first document's write-back was lost due to context-switching. **Skill execution bug**, not a crate bug.

**Fix**: Subagent delegation model in SKILL.md. Every `/agent-doc <FILE>` spawns a subagent via the Agent tool that owns its document cycle end-to-end. No locks for same-document -- Claude handles interruptions via re-read-before-write. Independent documents run as parallel subagents.

### Registry Concurrency: flock Advisory Locking (v0.2.1)

TOCTOU race on registry JSON solved with `RegistryLock` RAII guard using `fs2` crate (`flock` on Linux/macOS, `LockFileEx` on Windows). `with_registry(path, closure)` holds lock across entire read-modify-write. All write call sites hold the lock; read-only callers don't need it.

### Active Marker Merged into Claim

Instead of separate `.agent-doc/active/<hash>.json` files, the registry entry carries `active`/`active_since` fields. Single source of truth. `resync` prunes both dead panes and stale active flags. Re-claim behavior: last-write-wins (file is the important artifact, not the pane).

### Stale Claims After Machine Restart

After reboot, tmux pane IDs are reassigned but registry still has old IDs. Fix: `validate_file_claim()` in `claim.rs` runs after `resync::prune()`, checks if claimed pane is alive, logs `stale claim: <file> was bound to dead pane %X, replacing`, removes stale entry.

### Releases

| Version | What |
|---------|------|
| tmux-router v0.1.0 | Initial extraction: `Tmux`, `Registry`, `reconcile()`, 46 tests |
| tmux-router v0.2.0 | CLI binary, file-name addressing, `capture_pane()`, `send_keys_raw()` |
| tmux-router v0.2.1 | `RegistryLock` RAII guard, `flock`-based advisory locking, `fs2` dependency |
| agent-doc v0.9.4 | Updated to `tmux-router = "0.2"` |
| agent-doc v0.9.5 | Registry locking, stale claim auto-pruning, tmux-router 0.2.1 |

### Pending: Skill Creator Eval Suite

10 test cases designed for agent-doc skill (single-submit, empty-submit, inline-annotation, comment-ignored, concurrent-submit, git-commit, snapshot-persistence, cwd-resilience, claim-workflow, diff-accuracy). Plan: use Anthropic Skill Creator to validate subagent architecture.

## User

