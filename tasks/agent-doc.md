---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.17.30+dev | **Tests:** 446 (152 lib + 273 bin + 21 int) | **Dependencies:** tmux-router v0.2.6, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.5 (JNA FFI + Gson + IPC) + VS Code v0.2.0 (prepend fix)
**Library:** `libagent_doc.so` (1.7M) — 6 FFI exports | **New:** undo, extract, transfer commands
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.4**: Standalone Rust crate (library + CLI). `Tmux` struct, `Registry`, `reconcile()` (2D sync), `IsolatedTmux` (test infra), `FileResolution` callback, `prune()`, `acquire_or_skip()`. 56 tests. Published to crates.io + GitHub Releases.

**Core:** diff → agent → merge-safe write → snapshot → git. Components (`&lt;!-- agent:name --&gt;`), template mode, stream mode (CRDT), `agent-doc patch` (hooks), `agent-doc watch` (debounced + reactive), `agent-doc write --stream` (CRDT merge), `agent-doc recover`, `agent-doc plugin install`. Comment stripping, auto-resync, stale snapshot recovery, diagnostic logging.

**Library target (`libagent_doc`):** `[lib]` with `crate-type = ["cdylib", "lib"]`. Exports `component`, `crdt`, `ffi`, `frontmatter`, `merge`, `template` modules. C ABI via `ffi.rs` (6 functions: parse_components, apply_patch, crdt_merge, merge_frontmatter, free_string, free_state). Binary modules use `pub(crate) use agent_doc::*` re-exports. `agent-doc lib-path` prints the shared library location.

**Concurrency:** `SnapshotLock` + `RegistryLock` (flock), atomic rename (tempfile + persist), doc advisory lock, `pub(crate)` load/save.

**Routing:** Window-scoped. Plugin passes `--window`. Auto-start dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugins (JetBrains + VS Code):** 9/9 SPEC parity. `editors/` directory. `agent-doc commands` for autocomplete. JetBrains: JNA FFI bindings (`NativeLib.kt`) with Kotlin fallback — native-first patching for component patches and frontmatter merge.

**Config:** `~/.config/agent-doc/config.toml`. Multi-agent backend support. Execution mode per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade:** SKILL.md bundled via `include_str!`. Pre-flight version check → `agent-doc skill install`.

### Race Conditions

| Race | Protection |
|------|-----------|
| Registry read-modify-write | flock (RegistryLock) |
| Snapshot read-modify-write | flock (SnapshotLock) |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` |
| Watch daemon write window | Atomic rename + doc advisory lock |
| Stale snapshot (compaction) | `is_stale_snapshot()` auto-recovery |
| Compaction mid-write | Durable pending store + `recover` |

### Execution Model

| Mode | Behavior |
|------|----------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent use subagent |
| **`parallel`** | Every `/agent-doc` spawns subagent |
| **`sequential`** | Fully sequential, cheapest |

### Domain Ontology

| Term | Derives From |
|------|-------------|
| **Session** | project + story |
| **Document** | entity + context |
| **Component** | scope + abstraction |
| **Project** | system + scope |
| **Pane** | focus + scope |
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.9.6 | Window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename. 176 tests. |
| v0.9.7-v0.9.10 | `agent-doc write`, template mode, compaction recovery, `recover`, `template-info`, code-span parser fix. 192 tests. |
| v0.10.0-v0.10.1 | Renamed `response_mode` → `agent_doc_mode`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared `merge.rs`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | `compact`, `convert`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.2 | Route focus fix, claim defaults to template, `mode` CLI, skill reload (`--reload compact/restart`). |
| v0.14.3-v0.14.9 | Removed auto-prune, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range `strip_comments`, multi-backtick support. 235 tests. |
| v0.15.0 | CRDT stream mode (`yrs`), chain-of-thought streaming, watch daemon stream support, cumulative flush fix. 293 tests. |
| v0.15.1 | Claim defaults to stream mode, route.rs refactor (`send_command()`, `find_target_pane()`, `[route]` logging). |
| v0.16.0 | Reactive file-watching (zero debounce for stream), truncation detection (recheck chain), CRDT stale base fix, `docs/reference/reactive-stream.md`. 308 tests. |
| v0.16.1 | Native compact for template/stream mode (`--component`, `--message`), deterministic-in-Rust convention. 309 tests. |
| v0.17.0 | Split `agent_doc_mode` → `agent_doc_format` + `agent_doc_write`. IPC write (`--ipc` + JetBrains PatchWatcher v0.2.1). tmux layout fix (`first_pane_join_flag`). 303 + 67 tests. |
| v0.17.29 | JNA FFI integration — `agent_doc_merge_frontmatter()` export, `lib-path` command, NativeLib.kt JNA bindings, native-first patching with Kotlin fallback. JetBrains plugin v0.2.5. 440 tests. |
<!-- /agent:releases -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed twice: component.rs (v0.9.10) and diff.rs (v0.14.8). Check all `<!--` parsers when adding new ones.
14. **Immutable session binding** — `tmux_session` in frontmatter should never be silently overwritten by claim. This caused cross-session pane swapping when autoclaim or lazy-claim ran in a different session. Fix: refuse to overwrite unless `--force`. Track multi-step plans in `agent:pending` to survive context compaction.
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.9.10:** `agent-doc write` command, template mode (phases 1-4), compaction recovery, `recover`, code-span parser fix.

**v0.10.0-v0.12.2:** Renamed `response_mode` → `agent_doc_mode`, `exchange` component convention, append-friendly merge, `compact`/`convert` commands, lazy claim, exchange defaults. CWD drift lessons.

**v0.13.0-v0.13.3:** `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim integration, spare pane assignment for sole-column files (tmux-router v0.2.4).

**v0.14.0-v0.14.9:** Route focus fix, skill reload, removed auto-prune (was killing sessions), `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick CommonMark §6.1. 235 tests.

**v0.15.0:** CRDT stream mode (4 phases). `crdt.rs` (`yrs`), CRDT merge path, `StreamingAgent` trait, `agent-doc stream` command. Chain-of-thought streaming. Watch daemon stream support. Converted this document to stream mode. 293 tests.

**v0.15.1:** Claim defaults to stream mode. Refactored `route.rs` (extracted methods, `[route]` logging). Identified CRDT merge bug + auto-start race condition.

**v0.15.1-v0.16.0:** Fixed CRDT stale base detection (common-prefix <50% → use ours as base). Truncation detection: `looks_truncated()` cascade + delayed recheck chain (200ms x 25). Proposed `agent_doc_write`/`agent_doc_format` config split. Reactive stream mode: zero debounce for stream docs, `HashSet<PathBuf>` reactive tracking. Docs: `reactive-stream.md` with diagrams. Diagnosed PATH shadowing (`.bin/agent-doc` symlink). Added release checklist to `CLAUDE.local.md`. Published v0.16.0. 308 tests.

**v0.16.1:** Native compact for template/stream mode. Published v0.16.1.

**v0.17.0:** Split `agent_doc_mode` into `agent_doc_format` (append | template) + `agent_doc_write` (merge | crdt). `resolve_mode()` with backward-compatible deprecation. CRDT as default write strategy. Convert CLI extended with `--agent-doc-format`/`--agent-doc-write` flags. Refactored 12 files, removed dead code. 303 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange -->

### Re: Response Diff Storage, Undo, Extract, Transfer

All four features implemented:

**1. Response diff storage** (`snapshot.rs`):
- New `.agent-doc/pre-response/` directory stores the document state before each agent write
- `save_pre_response(doc, content)` called in all write paths (inline, template, stream + IPC)
- One-deep undo: latest pre-response only (overwritten each write)

**2. `agent-doc undo`** (`undo.rs`):
- Restores from `.agent-doc/pre-response/<hash>.md`
- Updates main snapshot to match restored state
- Deletes pre-response file (consumed) to prevent double-undo
- No-op with message if no pre-response exists

**3. `agent-doc extract`** (`extract.rs`):
- `agent-doc extract <source> <target> [--component exchange]`
- Extracts the last `### Re:` block from source's exchange component
- Appends to target's matching component
- Updates both snapshots atomically
- 3 unit tests for split_last_entry

**4. `agent-doc transfer`** (`extract.rs::transfer`):
- `agent-doc transfer <source> <target> <component>`
- Moves entire component content from source to target
- Clears source component, appends to target component
- Useful for splitting documents or archiving sections

**Tests:** 446 (152 lib + 273 bin + 21 int) — 6 new tests. Clippy clean.

**Files changed:**
- `src/snapshot.rs` — pre-response save/load/delete/path functions
- `src/write.rs` — save_pre_response calls in all write paths
- `src/undo.rs` — new module
- `src/extract.rs` — new module (extract + transfer)
- `src/main.rs` — wired up Undo, Extract, Transfer commands

bump plugin versions and build
I want to test the agent-doc version + plugins before releasing
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Release v0.17.30 with immutable session binding fix
- [x] Phase 2 step 3: FFI is primary path, Kotlin fallbacks retained as safety net
- [x] Phase 2 step 4: Replace parsePatchJson hand-rolled parser with Gson
- [x] VS Code: Fix missing prepend mode (falls through to replace)
- [x] Response diff storage — save pre-response snapshot before each write
- [x] `agent-doc undo` — restore pre-response snapshot
- [x] `agent-doc extract` — move request+response pair to another document
- [x] `agent-doc transfer` — move context between documents by component
- Document binary-vs-agent responsibility split in README/CLAUDE.md (and AGENTS.md)
- Plugin phases 5-7 — component folding, gutter icons, structure view (depends on `template-info`)
- Multi-agent backends (`agent/codex.rs`, `agent/gemini.rs`)
- `agent-doc deep` — fan-out parallel subagents with git worktrees, review checkpoint for fan-in
- `agent-doc status` — Ratatui TUI with dashboard + interactive pane attachment
- Sub-profiles for submodule workflows in `.agent-doc/config.toml`
- Docker sandboxing (`--sandboxed`)
- `TmuxBatch` — command batching to reduce flicker
- `agent/api.rs` — direct HTTP API backend
- `agent-doc init --dashboard` — scaffold template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`
- Execution mode in `config.toml` (currently only in SKILL.md frontmatter)
- Blog post on briantakita.me: document modes (append vs template vs exchange) with screencast
- `agent-doc stream` — bridge terminal STDOUT/STDERR to document components in real-time
- CRDT conservative dedup ([#15](https://github.com/btakita/agent-doc/issues/15)) — post-merge pass for identical adjacent blocks
- CRDT frontmatter patches ([#16](https://github.com/btakita/agent-doc/issues/16)) — `patch:frontmatter` target for YAML updates
<!-- /agent:pending -->
