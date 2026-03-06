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

Is the cli interface implemented?

## Assistant

No. tmux-router is currently **library-only** — there's no `src/main.rs` or `[[bin]]` in `Cargo.toml`. The CLI API listed at the top of this document is a design spec that hasn't been built.

All tmux routing is currently done through **agent-doc**, which calls the `tmux_router` library API directly (Rust function calls, not CLI subprocess). The commands like `agent-doc sync`, `agent-doc claim`, `agent-doc focus` are thin wrappers that call `tmux_router::reconcile()`, `tmux_router::Registry`, etc.

**Current status (v0.1.3):**
- Library API: fully implemented and published to crates.io
- 48 tests (integration + proptest + unit)
- Used by agent-doc v0.9.3 as a dependency
- No standalone CLI binary

The CLI binary was listed as "Pending / Not Built" in `tasks/agent-doc.md`. It would be useful for non-Rust consumers or shell scripting, but agent-doc doesn't need it since it links directly.

Want me to implement the CLI?

## User

Yes...how would it work? Could I send shell commands?

It also looks like I can send data to the panes. What would the interaction with a program running in the pane be like in this case?

## Assistant

### CLI Design

The CLI would be a thin wrapper around the existing library API. Two categories:

**Layout management** (what agent-doc already uses via library calls):
```bash
tmux-router sync --col a.md --col b.md --session 0 --window @0
tmux-router register tasks/plan.md %5
tmux-router status          # show registry + pane health
tmux-router resync           # prune dead panes
tmux-router focus tasks/plan.md
```

**Pane interaction** (new, directly useful from shell):
```bash
tmux-router send %5 "ls -la"              # send shell command to pane
tmux-router send %5 --no-enter "partial"  # send text without pressing Enter
tmux-router capture %5                     # read pane content (stdout)
tmux-router capture %5 --lines 50         # last 50 lines of scrollback
```

### Sending Commands to Panes

`send_keys()` already exists in the library — it types text into a pane literally and presses Enter. Whatever is running in that pane receives the keystrokes as if you typed them.

**Interaction with a shell:**
```bash
tmux-router send %5 "cd /tmp && ls"
# → the shell in pane %5 runs the command
```

**Interaction with Claude Code:**
```bash
tmux-router send %5 "fix the failing test"
# → Claude Code in pane %5 receives the prompt
```

**Interaction with any TUI (vim, htop, etc.):**
```bash
tmux-router send %5 --raw ":wq"    # send to vim
tmux-router send %5 --raw "q"      # send to htop
```

### Reading Pane Output

Not yet implemented, but tmux has `capture-pane`:
```bash
tmux-router capture %5
# → returns the visible content of pane %5 (what you'd see on screen)
# Uses: tmux capture-pane -t %5 -p
```

This enables **scripted workflows**:
```bash
# Send a command, wait, read the result
tmux-router send %5 "cargo test"
sleep 5
tmux-router capture %5 | grep "test result"
```

### The Full Picture

tmux panes are **bidirectional channels**. The program running inside doesn't know or care whether keystrokes come from a human or `send-keys`. So:

| Running in pane | `send` does | `capture` returns |
|----------------|-------------|-------------------|
| bash/zsh | Executes command | Terminal output |
| Claude Code | Submits prompt | Agent response stream |
| vim/neovim | Sends keystrokes | Editor screen |
| python REPL | Evaluates expression | REPL output |
| Any TUI | Sends input | Screen buffer |

This makes tmux-router a **universal agent harness** — you can drive any CLI tool programmatically, including other AI agents, without them needing an API. agent-doc's `send_keys` already uses this for Claude Code prompt injection.

### Implementation Plan

1. Add `src/main.rs` with clap CLI (~150 lines)
2. Add `capture_pane()` to `Tmux` struct (~20 lines)
3. Wire `send`, `capture`, `sync`, `register`, `status`, `resync`, `focus` subcommands
4. Publish as dual crate (library + binary)

~200 lines total. Want me to build it?

## User


