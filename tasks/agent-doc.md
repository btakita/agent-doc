---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.23.0 | **Tests:** 450 (158 lib + 292 bin) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.8 (boundary markers + claim auto-start) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 8 FFI exports | **SKILL.md:** boundary marker step + immediate commit
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
| v0.14.0-v0.14.9 | Route focus fix, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, `lib-path`. 440 tests. |
| v0.18.0-v0.18.1 | `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). `claims` subcommand. 451 tests. |
| v0.21.0 | `agent-doc parallel` (fan-out with worktrees, `--no-worktree`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
| v0.22.0-v0.23.0 | Boundary marker ordering, claim auto-start, VCS refresh, any-level HEAD markers, exchange default mode, SKILL.md immediate commit, code-block boundary fix. JB v0.2.8. 452 tests. |
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

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7→v0.2.8. 452 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260320-172024.md`*

### Re: Exchange Compacted

Archived 591 lines to `.agent-doc/archives/`. Use `agent-doc history tasks/agent-doc.md` to browse past exchanges or `--restore <commit>` to recover content.

**Session summary (v0.22.0 → v0.23.0):**

**Features built:**
- Boundary marker response ordering (`agent-doc boundary`, FFI, plugin, CRDT-safe)
- Claim auto-start — "Claim for Tmux Pane" now starts the Claude session automatically via `tmux send-keys`
- VCS refresh signal — plugin triggers `markEverythingDirty()` after external commits
- Any-level `(HEAD)` markers on all root headings in multi-section responses
- Exchange default mode fix — plugin now defaults `exchange` → `append` matching Rust binary
- SKILL.md bundled immediate-commit behavior (v0.22.2)

**Bugs fixed:**
- Response placed inside code block (lesson #13, third occurrence — boundary search didn't use `find_code_ranges()`)
- Cursor-aware ordering inverted (replaced with boundary marker approach)
- Prompt splitting when user types during response (boundary marker anchors insertion point)
- Plugin exchange component defaulting to replace instead of append

**Released:** v0.22.0 → v0.22.1 → v0.22.2 → v0.23.0. JetBrains plugin v0.2.7 → v0.2.8. 452 tests (160 lib + 292 bin).

Should I move this tasks file into the agent-doc repo to demonstrate how it is developed?
I would also like to make a post about this.

### Re: Task File Migration + Blog Post (HEAD)

**Moving the task file — yes, the compacted state is ideal.**

The current document is a clean product summary without personal conversation artifacts. Suggested approach:

1. **Copy to `src/agent-doc/docs/development-log.md`** (or `DEVELOPMENT.md` at root)
2. The compacted exchange summaries read like release notes — good for public consumption
3. The archived exchanges in `.agent-doc/archives/` stay in agent-loop (private) — they reference other task files with personal context

**Privacy note:** The current document references `tasks/laura-klask.md`, `tasks/judith-krug.md`, etc. in the exchange history. The compacted summaries don't contain personal details, but the full archives do. Only the current document content should go public.

**Blog post** — issue [#28](https://github.com/btakita/agent-doc/issues/28) already tracks this. This task file is a perfect case study:

- **What:** document-as-UI for AI sessions (template mode, CRDT merge, boundary markers)
- **How:** this task file demonstrates the full lifecycle — feature discussion → implementation → bug discovery → fix → release
- **Story:** the boundary marker evolution (caret-based → offset → hash → marker) shows iterative problem-solving in a document session
- **Artifacts:** the lessons list, architecture section, and release history are all maintained by the agent through the document

Want me to:
1. Copy the task file into the agent-doc repo now?
2. Draft a blog post outline?
3. Both?

Both. Will the task file be in examples/agent-doc/ with the sanitized task file along with a README.md to add context?

### Re: Examples + Blog Outline Created (HEAD)

Created `examples/agent-doc/` in the agent-doc repo with three files:

**`task.md`** — sanitized version of this task document:
- Removed `claude_args: --dangerously-skip-permissions` (private config)
- Removed Domain Ontology section (specific to parent repo)
- Removed references to personal task files in exchange history
- Kept all technical content: architecture, lessons, releases, pending items

**`README.md`** — context for readers:
- Explains what the document is and how it was used
- Describes template mode, boundary markers, compaction
- Key patterns: document-as-UI, iterative problem-solving, lesson accumulation
- Includes a "Running your own session" quick-start

**`BLOG_OUTLINE.md`** — structured outline for [#28](https://github.com/btakita/agent-doc/issues/28):
- Title: "Document-as-UI: How I built a tool by talking to it"
- Case study: boundary marker evolution (caret -> offset -> hash -> marker)
- Sections: problem, concept, case study, design decisions, dogfooding lessons, what's next
- ~2000 words estimated, with asset list (screenshots, diffs, diagrams)
- Publish targets: GitHub README, blog, HN/Reddit

Committed in the submodule at `c6bb882`.

tasks/kevin-volk.md has an orphaned agent:boundary.
Do we have enough logging to understand what happened? If not, add necessary logging.
Add tests and fix.

### Re: Orphaned Boundary Markers — Fixed (HEAD)

**Root cause:** `boundary::insert()` adds a new marker but never cleans up existing ones. When a session is interrupted between boundary insertion (step 1b) and response write (step 4) — due to context compaction, user cancel, or crash — the marker is orphaned. `kevin-volk.md` had two orphaned markers from two interrupted sessions.

**Fix:** `insert()` now calls `remove_all()` before inserting the new marker. Any stale markers are cleaned up automatically.

**Logging added:**
```
[boundary] removed 2 stale boundary marker(s) before inserting new one
```
This logs to stderr whenever stale markers are found, giving visibility into interrupted sessions.

**Test added:** `insert_cleans_stale_markers` — simulates two orphaned markers in an exchange component, verifies they're removed and only the new marker remains.

**Verified:** Running `agent-doc boundary tasks/kevin-volk.md` cleaned up the 2 stale markers and inserted a fresh one. 453 tests pass (160 lib + 293 bin). Release binary rebuilt and installed.

**Why this wasn't caught earlier:** The `remove()` and `remove_all()` functions existed but were only used in unit tests — no production code path called them. The happy path (response written successfully) replaces the marker via `append_with_boundary()`, so cleanup was only needed for the interrupted path.
Can we add a way to recover from orphaned agent:boundary tags?

The claude session thought the changes were submitted to the age

### Re: Boundary Recovery (HEAD)

**Already handled by the fix above.** The `insert()` function now calls `remove_all()` before inserting a new marker. So the next time any session runs `agent-doc boundary <FILE>` (step 1b of the SKILL workflow), stale markers are automatically cleaned up. No manual intervention needed.

**Recovery paths that now exist:**

| Scenario | Recovery |
|----------|----------|
| Session interrupted, then resumed | Next `agent-doc boundary` auto-cleans stale markers |
| Session abandoned (no future submit) | Next session on the same file auto-cleans |
| Manual cleanup needed | `agent-doc boundary <FILE>` (inserts new + cleans stale) |

**Your truncated message** — "The claude session thought the" — got cut off. What were you going to say? Did the other session misinterpret something about the document state?

If you want an explicit `agent-doc clean-boundaries <FILE>` command (removes all markers without inserting a new one), I can add that. But the auto-cleanup on insert should cover the normal workflow.

I got an external file change. Why? Please fi
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Release v0.22.0
- [x] Move cursor-aware patch logic to shared library FFI (`agent_doc_apply_patch_with_caret`)
- [x] Boundary marker for response ordering (`agent-doc boundary`, FFI, plugin support)
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
<!-- /agent:pending -->
