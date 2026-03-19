---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.22.0 | **Tests:** 466 (158 lib + 287 bin + 21 int) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.7 (cursor-aware FFI) + VS Code v0.2.3 (prepend fix)
**Library:** `libagent_doc.so` (1.7M) — 7 FFI exports | **Routing:** numeric session fix (colon suffix)
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
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed twice: component.rs (v0.9.10) and diff.rs (v0.14.8). Check all `<!--` parsers when adding new ones.
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
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Exchange Compacted

Archived 1439 lines to `.agent-doc/archives/`. Use `agent-doc history tasks/agent-doc.md` to browse past exchanges or `--restore <commit>` to recover content.

**Session summary (v0.21.0 → v0.22.0):**

**Features built:**
- `agent-doc terminal` — cross-platform terminal launch from editor plugins (config-first, stale session detection)
- `agent-doc parallel` — fan-out with git worktrees + `--no-worktree` for read-only tasks
- Selective commit — `git hash-object` + `git update-index` stages snapshot only, user edits stay uncommitted
- `(HEAD)` marker — visual boundary on last `### ` heading (committed vs working tree)
- Cursor-aware FFI — `agent_doc_apply_patch_with_caret()` in shared library + JNA bindings
- CRDT post-merge reorder via Yrs attribution
- First-submit snapshot fix (git matches current → treat as new)
- JetBrains plugin v0.2.7 (cursor-aware append ordering)

**Bugs fixed:**
- First-submit "no changes detected" when file was just committed
- `(HEAD)` marker only matched `### Re:` (now any `### `)
- Plugin version path was wrong (`0.1.0` → `0.2.7`)

**Process improvements:**
- Immediate commit after write (no green gutter gap between cycles)
- Pending protocol: flush to disk before responding
Compacted. Content archived.

The JB plugin does not register the commit.
The green gutter still takes up the entire response...until I refresh the file.
Then the commit is registered.
It seems the document + git view is stuck in a cached state.

### Re: VCS Refresh Signal

The issue: `agent-doc commit` runs externally (CLI), but IntelliJ caches git state. The gutter stays green until the user manually refreshes.

**Fix: IPC signal for VCS refresh.**

**Rust side (`git.rs`):** After a successful commit, writes `.agent-doc/patches/vcs-refresh.signal`.

**Plugin side (`PatchWatcher.kt`):** Already watches `.agent-doc/patches/`. When it sees `vcs-refresh.signal`:
1. Deletes the signal file
2. Calls `VcsDirtyScopeManager.getInstance(project).markEverythingDirty()` on the EDT
3. IntelliJ refreshes all git gutter annotations

**Flow:**
```
agent-doc commit → git commit succeeds → writes vcs-refresh.signal
PatchWatcher → detects signal → VcsDirtyScopeManager.markEverythingDirty()
IntelliJ → re-reads git state → gutter updates immediately
```

Release binary rebuilt. Plugin v0.2.7 rebuilt with VCS refresh support. Install the new plugin zip to get automatic gutter updates after commits.

**Plugin path:** `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip`
Can the plugin force a refresh of the git state?


### Re: Multi-Heading HEAD Marker

Fixed. `add_head_marker()` now marks ALL new `### ` headings in the agent's response, not just the last one.

Do responses always use `### `. Is it possible for a response to have a `## ` or `# ` or `#### ` or `##### ` or `###### `? If yes, can we make this rule apply to **any** root section title in the patch response.
So only the top level roots of the response...even if it's not a h3 tag.

**How it works:**
1. Gets git HEAD content for the file
2. Finds all `### ` headings in the snapshot
3. Filters to only headings that are NOT in git HEAD (= new, added by the agent)
4. Appends ` (HEAD)` to each new heading
5. Fallback: if git HEAD unavailable, marks the last heading only

For your `tasks/laura-klask.md` case with two response sections, both would now get the marker:
```
### Re: Amulet Technologies review (HEAD)    ← blue gutter
...response content...                       ← no gutter (committed)
### Re: Laura follow-up length (HEAD)        ← blue gutter
...response content...                       ← no gutter (committed)
```

Release binary rebuilt.
In tasks/laura-klask.md, I received a multi-section patch response from the agent.
However the section title uncommited response only applied to the last section. It should have applied to all sections in the patch response:
- ### Re: Amulet Technologies review
  - Unsuccessfully applied
- ### Re: Laura follow-up length
  - Successfully applied


### Re: Any-Level Heading Markers + VFS Refresh

**1. Heading level flexibility — Fixed**

`add_head_marker()` now matches ANY markdown heading level (`#` through `######`). Only the **root-level** (shallowest) headings in the new content get marked:

```
## Summary (HEAD)           ← root level, marked
### Details                 ← sub-heading, NOT marked
## Recommendations (HEAD)   ← root level, marked
### Item 1                  ← sub-heading, NOT marked
```

This works regardless of whether the response uses `###`, `##`, or any other level.

**2. VFS refresh — Fixed**

Added `LocalFileSystem.getInstance().refresh(true)` before `markEverythingDirty()`. The VFS refresh ensures IntelliJ re-reads the git state from disk before recomputing gutter annotations. Without VFS refresh, the dirty scope manager works against stale cached data.

**Note:** The plugin still needs to be reinstalled for these changes to take effect. Current plugin: `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.7.zip`
I still saw the full green gutter. The full green gutter went away after I made an edit.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Release v0.22.0
- [x] Move cursor-aware patch logic to shared library FFI (`agent_doc_apply_patch_with_caret`)
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
