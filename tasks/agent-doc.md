---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.3 | **Tests:** 521 (167 lib + 316 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✓
**Plugins:** JetBrains v0.2.12 (boundary reposition + end-of-exchange) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 8 FFI exports | **SKILL.md:** preflight + IPC boundary reposition
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
| v0.10.0-v0.10.1 | Renamed `response_mode` -> `agent_doc_mode`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared `merge.rs`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | `compact`, `convert`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.9 | Route focus fix, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, `lib-path`. 440 tests. |
| v0.18.0-v0.18.1 | `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). `claims` subcommand. 451 tests. |
| v0.21.0 | `agent-doc parallel` (fan-out with worktrees, `--no-worktree`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
| v0.22.0-v0.23.0 | Boundary marker ordering, claim auto-start, VCS refresh, any-level HEAD markers, exchange default mode, SKILL.md immediate commit, code-block boundary fix. JB v0.2.8. 452 tests. |
| v0.23.1-v0.23.2 | Stale boundary cleanup, boundary-aware insertion (unmatched + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples/blog outline. 459 tests. |
| v0.23.3 | Cross-platform sync pane guard (`ps` instead of `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests. |
| v0.24.0-v0.24.1 | `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content tests + heredoc examples, VCS refresh signal fix (JB v0.2.9). 497 tests. |
| v0.24.2 | SKILL.md step 3b (mandatory pending), `plugin install --local`, JB v0.2.10 (startup resync --fix). 497 tests. |
| v0.24.3 | Context session for auto_start (wrong-session fix), post-sync resync --fix. 497 tests. |
| v0.24.4 | Deterministic boundary re-insertion in apply_patches (end of exchange). 497 tests. |
<!-- /agent:releases -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity. This applies to ALL tmux `-t` targeting, not just stash. `new_window("-t", "0")` interprets "0" as window index; `new_window("-t", "0:")` forces session name. Root cause of persistent session bleedover bug fixed in v0.2.8.
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed THREE times: component.rs (v0.9.10), diff.rs (v0.14.8), boundary search in component.rs + write.rs (v0.23.0). Check all `<!--` parsers when adding new ones.
14. **Immutable session binding** — `tmux_session` in frontmatter should never be silently overwritten by claim. This caused cross-session pane swapping when autoclaim or lazy-claim ran in a different session. Fix: refuse to overwrite unless `--force`. Track multi-step plans in `agent:pending` to survive context compaction.
15. **No Gson in JetBrains plugin** — Gson causes `ClassNotFoundException` at runtime in some IntelliJ builds. Use hand-written JSON parsing. This bug was found and fixed twice (SlashCommandCompletionContributor Mar 11, PatchWatcher Mar 18) — check git history before introducing "bundled" dependencies.
16. **Never fall back to current_tmux_session() when tmux_session is set** — `display-message #{session_name}` returns whichever session the user's terminal is viewing. If the user switches to session 1, all routes using `current_tmux_session()` as fallback will create panes in session 1. Fix: bail with error instead of fallback. Only use `current_tmux_session()` for first-claim (no frontmatter binding yet).
17. **Deterministic operations belong in the binary, not the skill** — when adding a feature, ask "will ALL write paths need this?" If yes, implement in the binary. Boundary insertion was initially a SKILL step (1b) but caused stale boundary bugs when non-SKILL paths (run, stream, watch) bypassed it. Moved to `apply_patches()` in v0.24.4. The binary-vs-agent test: "can it be unit-tested with fixed inputs?" If yes → binary.
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.12.2:** `agent-doc write`, template mode, compaction recovery, `recover`, `exchange` convention, append-friendly merge, `compact`/`convert`, lazy claim.

**v0.13.0-v0.14.9:** `autoclaim`, git-based snapshot fallback, `commands`, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick.

**v0.15.0-v0.16.1:** CRDT stream mode (yrs), chain-of-thought streaming, reactive file-watching, truncation detection, native compact. 309 tests.

**v0.17.0-v0.17.30:** Format/write split. IPC write + JetBrains PatchWatcher. JNA FFI (NativeLib.kt). VS Code prepend. Immutable `tmux_session` binding. `lib-path` + `merge_frontmatter()` FFI export.

**v0.18.0-v0.18.1:** `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3.

**v0.19.0:** ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7).

**v0.20.0-v0.20.3:** CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs, session affinity enforcement, numeric session fix (colon suffix), `agent-doc claims` subcommand. 451 tests.

**v0.21.0:** `agent-doc parallel` — fan-out with git worktrees (`--no-worktree` for read-only). CRDT post-merge reorder via Yrs attribution. 461 tests.

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7->v0.2.8. 452 tests.

**v0.23.1-v0.23.2:** Stale boundary cleanup, boundary-aware insertion (unmatched content + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples + blog outline. 459 tests.

**v0.23.3:** Cross-platform sync pane guard (`ps` replaces `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests.

**v0.24.0-v0.24.1:** `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content validation tests, heredoc examples, VCS refresh signal fix (JB v0.2.9). 497 tests.

**v0.24.2:** SKILL.md step 3b (mandatory pending updates), `plugin install --local`, JB plugin v0.2.10 (startup resync --fix). 497 tests.

**v0.24.3:** Context session for auto_start (eliminates wrong-session pane creation), post-sync resync --fix (auto-catches drift). 497 tests.

**v0.24.4:** Deterministic boundary re-insertion in `apply_patches()` — re-inserts at END of exchange after checkpoint writes. Boundary lifecycle: consumed by write, re-inserted by binary at correct position, cleaned up next cycle. 497 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260322-162909.md`*
Why did you compact the exchange? I did not ask.

### Re: Why did you compact the exchange?

Apologies — I saw `compact exchange` in the previous diff as a new line you added and interpreted it as a command. Your content is preserved in the archive file:
`/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260322-162909.md`

You can restore it with:
```bash
agent-doc undo tasks/agent-doc.md
```

Or if you want to keep the compact but reference the history, the archive has the full exchange.

### Re: JB plugin moving prompt after response patch

**Root cause found and fixed.** The IPC write path was using the boundary position from disk (stale — above the user's new prompt), while the disk write path repositions the boundary to end-of-exchange before applying patches.

**The bug:**
1. After commit, boundary is at end of exchange: `[content] [boundary] [close]`
2. User types prompt below boundary: `[content] [boundary] [prompt] [close]`
3. IPC reads boundary from disk → sends old position to plugin
4. Plugin inserts response at boundary (above prompt): `[content] [response] [boundary] [prompt]`
5. Prompt appears to "move" after the response

**The fix:** All three IPC paths in `write.rs` now call `template::reposition_boundary_to_end()` before extracting boundary IDs. This matches the pre-patch step in `apply_patches_with_overrides()`: remove stale boundaries → insert fresh one at end of exchange → extract that boundary_id for the IPC patch.

**Files changed:**
- `src/template.rs` — extracted `pub fn reposition_boundary_to_end()` from the pre-patch logic
- `src/write.rs` — all 3 IPC paths (`try_ipc`, `run_stream` primary, `run_stream` timeout fallback) now use repositioned boundary

521 tests pass (167 lib + 316 bin + 38 CLI). Binary installed.
Update SPEC.md
audit docs
release

### Re: Update SPEC / audit docs / release

**v0.25.3 released** — IPC boundary reposition fix for prompt ordering.

**SPEC.md updates:**
- §7.22: Documented that all IPC write paths call `reposition_boundary_to_end()` before extracting `boundary_id` values
- §8.4: Added step 1 to the boundary reposition lifecycle (in-memory reposition before IPC JSON construction), making it a 4-step lifecycle

**README.md:** Version bumped to v0.25.3, test count updated to 521.

**CLAUDE.md:** Already up to date — boundary lifecycle is in the binary responsibility table, `template.rs` module description covers boundary lifecycle.

**Published:** crates.io ✓ | PyPI ✓ | GitHub Release ✓ | binary installed ✓

521 tests pass (167 lib + 316 bin + 38 CLI).
tmux session 0 window 0 right pane had an error loading tasks/toby-hill.md, which was moved to task/employment/toby-hill.md in the editor.

We need a way to recover from the error. Should we retry the autoclaim? How can we tell this was from a rename?

Content is:
```
~/work/btakita/agent-loop main ⇡422 +14 !33 ?9                                                                17:14:40
❯ /home/brian/.cargo/bin/agent-doc start tasks/toby-hill.md
Error: file not found: tasks/toby-hill.md
~/work/btakita/agent-loop main ⇡422 +14 !33 ?9                                                                17:14:40
❯ /agent-doc tasks/toby-hill.md
zsh: no such file or directory: /agent-doc
~/work/btakita/agent-loop main ⇡422 +14 !33 ?9
~/work/btakita/agent-loop main ⇡422 +14 !33 ?9                                                                17:14:40
```

### Re: Autoclaim error after file rename

The autoclaim reads the registry (`sessions.json`) which stores the old path. When the file is renamed in the editor, the registry entry becomes stale. Options:

**1. Validate file exists in autoclaim** (simplest) — Before outputting claim commands, check if the file still exists. If not, log a warning and remove the stale entry from the registry. This handles renames, deletes, and any path drift.

**2. Registry rename hook** — The editor plugin could update the registry when a file is renamed. JetBrains has `VirtualFileListener.beforePropertyChange()` for renames. But this couples the plugin to the registry format.

**3. Resync on autoclaim** — Run `resync --fix` logic as part of autoclaim to prune dead entries. This already exists in `resync.rs`.

**Recommendation:** Option 1 (validate + prune in autoclaim). It's a 5-line fix in `autoclaim.rs`: check `Path::new(&entry.file).exists()`, if not, remove from registry and log. No editor plugin changes needed.

### Re: ClawTeam investigation

**What it is:** Agent swarm orchestration (Python/CLI). Leader agent spawns workers via `clawteam spawn`, each getting a git worktree + tmux window + identity. Workers communicate via file-based transport or ZeroMQ. Uses a "board" for task assignment/status.

**Overlap with agent-doc:**
- Git worktrees for isolation (agent-doc has `parallel.rs` + `worktree.rs`)
- Tmux pane management (agent-doc has tmux-router)
- Multi-agent coordination (agent-doc has session registry + routing)

**Differences:**
- ClawTeam is a general swarm orchestrator (any CLI agent) — agent-doc is document-focused
- ClawTeam uses Python; agent-doc is Rust
- ClawTeam's "board" is a centralized task queue; agent-doc's documents ARE the coordination surface
- ClawTeam targets ML experiments and full-stack dev as primary use cases

**Relevance to agent-doc:** Low direct overlap. agent-doc's parallel fan-out already covers the worktree + tmux spawning. ClawTeam's "swarm self-organization" and cross-pollination patterns are interesting for inspiration but not directly applicable — agent-doc's document model is fundamentally different from a task queue model.

Worth bookmarking for the swarm coordination patterns, but not a dependency or integration target.

### Re: Autoclaim error on renamed file

**Implemented: file existence validation in autoclaim.** When a claimed file no longer exists (renamed/deleted), the stale entry is:
1. Logged as a warning: `[autoclaim] Pruning stale claim: tasks/toby-hill.md (file no longer exists)`
2. Removed from the session registry
3. Skipped for claim output

If ALL claims for a pane are stale, the pane gets a clean message instead of an error. The registry is saved after pruning so the stale entries don't reappear.

This handles the `tasks/toby-hill.md → tasks/employment/toby-hill.md` rename case. The pane will no longer try to `agent-doc start` on the old path. The user can re-claim the file at its new path with `/agent-doc claim tasks/employment/toby-hill.md`.

Binary installed. 521 tests pass.
<!-- agent:boundary:18c154d1-9334-43eb-9cf4-ac4afb2b91dd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] **Autoclaim: validate file exists** — prune stale entries when file is renamed/deleted
<!-- /agent:pending -->

<!-- agent:todo -->
- Install script (`curl | sh`) for binary download
- Homebrew formula (`brew tap btakita/agent-doc`)
- [ ] **lazily-rs DocContext** -- cache find_code_ranges() (sub-ms, not needed yet)
- Support `/btw` for instant processing
  - This may involve using /btw for agent-doc if /btw is the only way to prempt the loop if a task is running in the main thread
- Investigate https://github.com/HKUDS/ClawTeam to see if it would help agent-doc
- Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))
- Plugin phases 5-7 — component folding, gutter icons, structure view ([#19](https://github.com/btakita/agent-doc/issues/19))
- Multi-agent backends — codex, gemini ([#20](https://github.com/btakita/agent-doc/issues/20))
- `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))
- `agent-doc status` — Ratatui TUI dashboard ([#22](https://github.com/btakita/agent-doc/issues/22))
- Sub-profiles for submodule workflows ([#23](https://github.com/btakita/agent-doc/issues/23))
- Docker sandboxing (`--sandboxed`) ([#24](https://github.com/btakita/agent-doc/issues/24))
- `agent/api.rs` — direct HTTP API backend ([#25](https://github.com/btakita/agent-doc/issues/25))
- `agent-doc init --dashboard` ([#26](https://github.com/btakita/agent-doc/issues/26))
- Capture/log skill workflow ([#27](https://github.com/btakita/agent-doc/issues/27))
- Blog post: document modes with screencast ([#28](https://github.com/btakita/agent-doc/issues/28))
- `agent-doc stream` — bridge terminal to components ([#29](https://github.com/btakita/agent-doc/issues/29))
<!-- /agent:todo -->
