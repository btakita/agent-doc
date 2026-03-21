---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.24.1 | **Tests:** 497 (162 lib + 297 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.10 (startup resync --fix) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 8 FFI exports | **SKILL.md:** heredoc examples + immediate commit
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

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7->v0.2.8. 452 tests.

**v0.23.1-v0.23.2:** Stale boundary cleanup, boundary-aware insertion (unmatched content + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples + blog outline. 459 tests.

**v0.23.3:** Cross-platform sync pane guard (`ps` replaces `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests.

**v0.24.0-v0.24.1:** `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content validation tests, heredoc examples, VCS refresh signal fix (JB v0.2.9). 497 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Audit + Compact Done

**Audit-docs:** Updated CLAUDE.md (module layout + init description), README.md (quick-start + CLI reference), SPEC.md (init two-mode + new install spec). Only remaining warning: "large table" (acceptable) + SKILL.md staleness (pre-existing).

**Exchange compacted:** Archived 534 lines.

**Session summary (v0.23.3 → v0.24.1):**

**Features:**
- `agent-doc install` — system-level setup (prereqs, editor plugin auto-detect)
- `agent-doc init` (no-arg) — project-level setup (.agent-doc/, SKILL.md, hooks)
- `agent-doc init <file>` — lazy project init when .agent-doc/ missing
- SKILL.md heredoc examples (replaced `echo` with `cat <<'RESPONSE'`)

**Bugs fixed:**
- 3-pane duplicate: sync pre-start race with `resync::prune()` — added `find_alive_pane_for_file()`
- Cross-platform process inspection: `ps` replaces `/proc`
- VCS refresh signal: JB plugin now watches `ENTRY_CREATE` + `ENTRY_MODIFY`
- Orphaned window @33 cleanups
- Escaped backticks in patch output (model habit, not SKILL.md bug)
- Clippy fixes (collapsible_if, dead_code)

**Tests:** 497 (162 lib + 297 bin + 38 CLI) — 15 new tests for install/init/skill

**Released:** v0.23.3, v0.24.0, v0.24.1. JB plugin v0.2.9.
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260321-124751.md`*

There are multiple agent-doc tmux windows in tmux session 2. How did that happen?

### Re: Multiple agent-doc Windows in Session 2

**What happened:** 6 panes are in tmux session 2, each in its own window. They should all be in session 0 (stash window).

| Window | Pane | File |
|--------|------|------|
| @37 | %19 | tasks/x.md |
| @11 | %15 | tasks/youtube.md |
| @31 | %14 | tasks/kevin-volk.md |
| @41 | %11 | tasks/lucas-noble.md |
| @35 | %2 | tasks/corky.md |
| @39 | %6 | (unknown — not in registry) |

**Root cause:** `auto_start_in_session` creates new windows when it can't find a registered pane in the target session. If the frontmatter says `tmux_session: '0'` but the pane was created in session 2 (because that was the "current" session at the time), each auto-start creates a new window in session 2 instead of session 0.

We need to create the pan in `tmux_session: '0'` as specified in the frontmatter. Even if `tmux_session: '0'` is not instantiated. The "current" session should only be used when there is no `tmux_session` defined. 

`resync` reports these as "in session '2', expected '0'" — the panes are alive but in the wrong session.

**Fix options:**
1. **`resync --fix`** — kill wrong-session panes and let them be re-created in the correct session on next route
2. **Manual cleanup** — move panes to session 0's stash window, update registry
3. **Consolidate** — join all 6 panes into session 0's stash, close empty windows in session 2

**Recommendation:** Run `resync --fix` to clean up. The panes will be killed and re-created in session 0 on next use. This is the safest approach — the Claude sessions in those panes are likely stale anyway.

Want me to run `resync --fix`?
yes. Does the JB Plugin has `resync --fix` available? I see `Sync Tmux Layout`, but that doesn't seem to perform `resync --fix`.

### Re: resync --fix Done + Plugin Action

**Cleanup results:**
- `resync --fix` fixed 7 of 11 issues — killed wrong-session panes, deregistered stale entries
- Manually killed 1 orphan pane (%6) not in the registry
- Session 2 now clean: only zsh + corky windows remain
- 6 active sessions remain (agent-doc, robert-ward, judith-krug, linkedin, benjamin-becker, browser-bridge)

**JB plugin and `resync --fix`:**

No — the JB plugin doesn't have a `resync --fix` action. "Sync Tmux Layout" runs `agent-doc sync` which arranges panes to match the editor layout. It's a different operation.

**Options to add it:**
1. **Plugin action:** Add "Resync Sessions" to the agent-doc menu that runs `agent-doc resync --fix`
2. **Automatic:** Run `resync --fix` as part of `sync` (already partially done — `sync` calls `resync::prune()` but not `--fix`)
3. **Startup hook:** Run `resync --fix` when the plugin starts (catches drift from previous sessions)

**Recommendation:** Option 3 — run `resync --fix` on plugin startup. This catches wrong-session/wrong-window issues left over from previous IDE sessions or system restarts. It's transparent to the user and prevents the kind of drift we just cleaned up.

Want me to add this to the JB plugin?

Yes.

The last run did not register the commit. I did not run `/compact`. I have since run `/compact`. Should this issue be fixed now?
![img_1.png](img_1.png)
### Re: resync --fix on Startup + Commit Issue

**Commit issue:** Fixed. The previous session compacted before the commit step could run. This session's step 0b committed it as `47c2fec`. The gutter should now be clean for that response.

**JB plugin startup resync:** Added `resync --fix` to `PluginLifecycleListener.projectOpened()`. On IDE startup:
1. Checks if `.agent-doc/` exists (skips if not an agent-doc project)
2. Runs `agent-doc resync --fix` on a daemon thread (non-blocking)
3. Logs results to `idea.log` under `#com.github.btakita.agentdoc`

This catches wrong-session panes, stale registry entries, and dead panes left from previous IDE sessions — transparently, without user action.

**File changed:** `editors/jetbrains/.../PluginLifecycleListener.kt` — added `runResyncFix()` method + companion LOG.

Build: `./gradlew buildPlugin` — BUILD SUCCESSFUL.

Ready to bump plugin version and publish?

Yes
### Re: Plugin v0.2.10 Published (HEAD)

**JB plugin v0.2.10** built and committed:
- `PluginLifecycleListener.kt` — added `runResyncFix()` on `projectOpened()`
- `gradle.properties` — bumped `pluginVersion` to `0.2.10`
- Both unsigned + signed zips built: `build/distributions/agent-doc-jetbrains-0.2.10{,-signed}.zip`
- Committed in submodule: `c886689`

Install via: IDEA → Settings → Plugins → gear → "Install Plugin from Disk..." → select the zip. Structural class changes → uninstall first → restart → install → restart.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- `agent-doc install` — system setup, prereq check, editor plugin auto-detect ([v0.24.0])
- `agent-doc init` (no-arg) — project setup, .agent-doc/, SKILL.md, hooks ([v0.24.0])
- Install script (`curl | sh`) for binary download
- Homebrew formula (`brew tap btakita/agent-doc`)
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
