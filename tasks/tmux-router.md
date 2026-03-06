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
# Are we not using claim?
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

Tmux already have send? What value would tmux-router have over the stock tmux send?

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



## Assistant

Done. agent-doc v0.9.4 released:

- **Cargo.toml**: `tmux-router = "0.2"`, version bumped to 0.9.4
- **pyproject.toml**: synced to 0.9.4
- **crates.io**: published
- **PyPI**: published
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.9.4 (with binary)
- **Installed locally**: `agent-doc v0.9.4`
- **agent-loop**: submodule pointer updated, pushed to main

All 150 agent-doc tests pass (129 unit + 21 integration).

## User


## Assistant

Built the CLI — 48 tests pass, clippy clean. But first, your two questions:

### "Are we not using claim?"

`claim` is an agent-doc concept — it binds a UUID-based `agent_doc_session` to a pane. In tmux-router standalone, there's no UUID system, so `register` is the equivalent (maps file-path → pane). I could add `claim` as an alias for `register` if that's more intuitive, or agent-doc could keep its own `claim` command that calls `tmux-router register` under the hood (which is what it does now via library calls).

### "What value over stock tmux send?"

Three things:

1. **Registry-aware addressing** — send by file name instead of raw pane ID:
   ```bash
   tmux-router send plan.md "cargo test"   # looks up pane from registry
   # vs
   tmux send-keys -t %5 "cargo test" Enter  # must know pane ID
   ```

2. **Literal-by-default** — tmux `send-keys` interprets special key names by default (`C-c`, `Enter`, `Space`), which mangles code/text. tmux-router defaults to literal mode (`-l`) with `--raw` as opt-in for when you actually want key interpretation.

3. **Enter handling** — `send` automatically appends Enter with a 50ms delay (TUI-safe). Stock tmux requires manually adding `Enter` as a separate argument.

So `send`/`capture` wrap tmux's primitives with saner defaults and file-name addressing. For one-off commands, `tmux send-keys` works fine. The value shows up in scripted workflows where you're driving multiple panes by name.

### What was built

- `src/main.rs` (~160 lines) — clap CLI with 8 subcommands: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`
- `capture_pane()` on `Tmux` — reads screen buffer via `tmux capture-pane -p`
- `send_keys_raw()` on `Tmux` — sends raw tmux key names (C-c, Escape, etc.)

I should also update `send` to accept file names (registry lookup) in addition to raw pane IDs. Want me to add that + release?

## User

Yes. I'm trying to figure out the use cases of tmux-router standalone.
So let's say someone is using nvim. They bind a tmux pane with a file. The tmux pane can run a program that watches the file.

In the case of agent-doc, the file is a md file that acts as a agent loop interface.

What other programs would tmux-router be instrumental in supporting?
Perhaps a log watcher?

## Assistant

### The Pattern: File → Pane → Program

The generic pattern is: a file changes, a program in a bound pane reacts to it. tmux-router manages the binding and layout. The program in the pane is whatever you want.

**Concrete use cases:**

| Use Case | File | Program in Pane | Interaction |
|----------|------|----------------|-------------|
| **Agent loop** (agent-doc) | `plan.md` | Claude Code | File edits → agent prompt → response appended |
| **Log watcher** | `app.log` | `tail -f` or `lnav` | Real-time log stream bound to a pane |
| **Test runner** | `src/main.rs` | `cargo watch -x test` | File save → tests rerun in bound pane |
| **REPL-driven dev** | `scratch.py` | `ipython` | Edit file, send snippets to REPL via `send` |
| **Build monitor** | `Cargo.toml` | `cargo build --watch` | Dependency changes → rebuild visible in pane |
| **Database console** | `queries.sql` | `psql` / `sqlite3` | Send SQL from file to db console |
| **Jupyter alternative** | `notebook.md` | `python3` REPL | Markdown cells → send code blocks to REPL, capture output |
| **Multi-agent orchestration** | `task-a.md`, `task-b.md` | Multiple Claude Code instances | Route tasks to different agents, each in their own pane |
| **Documentation preview** | `README.md` | `mdcat` or browser refresh | File watch → re-render in pane |
| **Server + client** | `server.rs`, `client.rs` | Two processes | Side-by-side panes, send commands to either |

### The nvim Workflow

A nvim user could:

```bash
# Open project — layout mirrors nvim splits
tmux-router sync --col src/main.rs --col tests/main_test.rs

# Pane 1: cargo watch (auto-test on save)
# Pane 2: tail -f logs/app.log

# From nvim, send the current file's test to run:
tmux-router send tests/main_test.rs "cargo test test_specific_case"

# Read the test output:
tmux-router capture tests/main_test.rs --lines 20
```

The key insight: **nvim already has terminals, but they're inside nvim's process**. tmux panes are independent — they survive nvim restarts, can be shared across editors, and can be driven by scripts without nvim running.

### What Makes tmux-router Different From Just Using tmux?

Raw tmux gives you panes but no **semantic addressing**. You have to track pane IDs (`%5`, `%12`) yourself. tmux-router adds:

1. **File-to-pane binding** — address panes by the file they serve
2. **Layout reconciliation** — declare desired layout, tmux-router figures out the joins/breaks/reorders
3. **Registry persistence** — bindings survive across sessions
4. **Health management** — dead panes get pruned, stale bindings cleaned up

It's the difference between "I have 6 tmux panes" and "I have a pane for my test runner, a pane for my log watcher, and a pane for my REPL."

### Also: File-name addressing is now live

Updated `send` and `capture` to accept file names from the registry:

```bash
tmux-router send plan.md "cargo test"      # resolves plan.md → %5 from registry
tmux-router capture plan.md --lines 20     # same
tmux-router send %5 "cargo test"           # raw pane IDs still work
```

## User

Implement. Also document use cases.

## Assistant

Done. Two things shipped:

### 1. File-name addressing in `send` and `capture`

`resolve_target()` checks if the target starts with `%` (raw pane ID) or looks it up in the registry. Works for both `send` and `capture`:

```bash
tmux-router send plan.md "cargo test"     # registry lookup
tmux-router send %5 "cargo test"          # raw pane ID
tmux-router capture plan.md --lines 20    # registry lookup
```

### 2. README with documented use cases

Updated `tmux-router/README.md` with full CLI reference and 6 use cases:

- **Test runner** — `cargo watch` in a bound pane, capture results
- **REPL-driven dev** — send code to IPython from your editor
- **Log watcher** — `tail -f` bound to a log file
- **Database console** — send SQL to `psql` by file name
- **Multi-agent orchestration** — multiple Claude Code instances, each bound to a task file
- **Scripted workflows** — bash scripts using `send` + `capture` for automation

All 48 tests pass, clippy clean. Ready to bump version and release when you want.

## User

Release tmux-router and agent-loop

## Assistant

Released both.

### tmux-router v0.2.0

- **crates.io**: published (`cargo install tmux-router` now gets the CLI)
- **GitHub Release**: https://github.com/btakita/tmux-router/releases/tag/v0.2.0 (with prebuilt binary)
- **Tag**: `v0.2.0` pushed

Version bumped from 0.1.3 → 0.2.0 (new feature: CLI binary).

### agent-loop

- Submodule pointer updated to tmux-router `v0.2.0` (`f2d3490`)
- Pushed to `main`

Note: you said "agent-doc" but I released agent-loop (the parent repo with submodule pointer). agent-doc's `Cargo.toml` still points to `tmux-router = { path = "../tmux-router", version = "0.1" }` — it will need a version constraint bump to `"0.2"` and a new release if you want it to use the CLI features. Want me to update agent-doc too?

Yes

## User

I did not see the follow up response in this document.
Is this a bug with tmux-router or agent-doc?

Looks l
