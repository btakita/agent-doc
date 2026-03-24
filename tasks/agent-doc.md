---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.10 | **Tests:** 526 (173 lib + 315 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✓
**Plugins:** JetBrains v0.2.21 (sync logging + is_tracked) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 12 FFI exports | **SKILL.md:** preflight + IPC boundary reposition
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
Session: debounce improvements, unified diff, release v0.25.8+v0.25.9, plugin v0.2.20, tmux flicker investigation, GitHub issues #30-#37 created with architecture context.
Exchange compacted. 1337 lines archived. Summary preserved above.
On the right editor pane, I switched to tasks/resume.md. The tmux session 0 switched to tmux window 1:stash for a few seconds, then back to tmux window 0:agent-doc.
### Re: tmux flicker when switching to resume.md

**What happened:** At 23:15:04, `LayoutChangeDetector` detected the right pane change: `[[agent-doc.md], [justin-caird.md]]` → `[[agent-doc.md], [resume.md]]`. This triggered a sync that needed to stash the justin-caird pane and bring in (or create) a resume pane.

**Log gap:** There's no STDERR output from the sync command between 23:15:04 and 23:16:31. The `EditorTabSyncListener` has a 500ms debounce, so the sync should have started around 23:15:05 — but the output is missing from idea.log. This could mean:
1. The sync process output went to a different log stream
2. The sync ran but crashed/errored silently
3. The 500ms debounce was superseded by another event

**When the next logged sync happened** (23:16:31), it showed `[sync:FAST_PATH] layout already correct` with 2 panes in the agent-doc window (`%1` + `%31`). So by that point the layout was already resolved.

**The flicker itself** — the brief switch to stash — most likely happened during the unlogged sync at ~23:15:05. During that sync, the old justin-caird pane was stashed (moved to @2:stash) and the resume pane was attached. Even with `-d` flags, this transition can produce a visual flicker as discussed earlier.

**Next step:** Add logging to confirm the sync command actually runs after LayoutChangeDetector events. The EditorTabSyncListener delegates to `agent-doc sync` via ProcessBuilder — if the process fails to start or outputs to a different stream, we'd miss it.
continue
### Re: continue — missing sync logs found and fixed

**Root cause:** `SyncLayoutAction.syncLayout()` captured process output but **never logged it on success**. It only logged on failure (`exitCode != 0`). When called from `LayoutChangeDetector`, `notify = false`, so even failure output was swallowed.

Additionally, `EditorTabSyncListener.log()` used `LOG.debug()` — invisible unless debug logging is explicitly enabled.

**Fixes (plugin v0.2.21):**

1. **`SyncLayoutAction`:** Added `LOG.info("[sync] exit=... cmd=...")` and first 500 chars of output on every sync call — success or failure.

2. **`EditorTabSyncListener`:** Changed `LOG.debug()` → `LOG.info("[layout-sync] ...")` so tab-switch events are always visible in idea.log.

Now idea.log will show:
```
[layout] change detected via containerAdd: ... → ...     ← LayoutChangeDetector
[layout-sync] selectionChanged: ...                       ← EditorTabSyncListener
[sync] exit=0 cmd=agent-doc sync --col ...                ← SyncLayoutAction
[sync] output: [sync:GLOBAL] [sync-start] ...             ← process output
```

**Plugin v0.2.21** built. Install to see sync logs after the next tab switch.
Switching to tasks/linkedin.md on the right editor pane.
The right tmux pane split into 3 panes
### Re: linkedin.md → 3 panes

The new logging reveals the issue clearly.

**Timeline (23:31:29-31:40):**
1. `23:31:29` — You switched from resume.md to justin-caird.md (right pane)
2. `23:31:31` — You switched from justin-caird.md to linkedin.md
3. `23:31:31` — `LayoutChangeDetector` fires: `justin-caird → linkedin`
4. `23:31:31` — `EditorTabSyncListener` starts sync: `agent-doc sync --col agent-doc.md --col linkedin.md`
5. `23:31:39` — Sync output: `auto-starting session for tasks/linkedin.md (no alive pane)`
6. `23:31:40` — Sync completes (8 seconds — waiting for Claude to start in the new pane)

**Why 3 panes:** The sync auto-started a new Claude session for linkedin.md by running `split-window` (creates the 3rd pane), then waited for Claude to become ready (~8s). During the auto-start, the window briefly had 3 panes visible. The old pane should have been stashed after the new one was attached — but the auto-start creates the new pane first (ATTACH before DETACH, lesson #3).

**The flicker:** The 8-second delay was the `wait_for_claude_ready` poll waiting for Claude to start in the new pane. During this time the window had 3 panes.

**This is actually working correctly** — the auto-start for a new file requires creating a pane, starting Claude, and waiting for it to be ready. The 3-pane state is transient. The stash window isn't involved here.

The real question is whether the auto-start should happen in the stash window (hidden) and then join to the visible window after Claude is ready, to avoid the visual disruption.
We should only show the tmux pane associated with the currently selected file.
Why do we wait for Claude to become ready at all?
These should be deterministic tmux operations from the agent-doc binary.
### Re: sync should be deterministic, no waiting

You're right on all three points. The current design conflates two concerns:

**Sync (layout)** — deterministic tmux operations: stash unwanted panes, join wanted panes, select focused pane. Should be instant.

**Auto-start (session lifecycle)** — starting Claude in a new pane: `split-window`, send `agent-doc start`, wait for ready. This is slow and non-deterministic.

Currently `sync` calls `auto_start` inline when it encounters a file with no alive pane. This blocks the layout operation for 8+ seconds and shows intermediate states.

Why would there be intermediate states?
If there is no active tmux pane for the document, then start and show the tmux pane with the command that starts the claude session.
There should be no flicker. Just the tmux pane with claude starting.

If another document is selected while claude is loading, then show the tmux pane for the newly selected document...start the tmux pane + claude session if necessary.

**The fix:**

1. **Sync should only manage panes that already exist.** If a file has no pane, sync should:
   - Use a placeholder pane (empty shell) or skip that column
   - Trigger auto-start in the background (stash window, invisible)
   - When auto-start completes, the next sync cycle picks it up naturally

2. **Auto-start moves to background.** `agent-doc sync` returns immediately after layout. A background process starts Claude in the stash, registers the pane, and the next sync (from `EditorTabSyncListener` poll or layout change) joins it.

3. **Only the focused file's pane is guaranteed visible.** Non-focused columns can show a "starting..." placeholder or the existing stash pane until the real one is ready.

This separates the fast path (layout sync, <100ms) from the slow path (session start, 5-30s) and eliminates visual disruption.

Want me to implement this? It's a change in `sync.rs` — remove the inline `auto_start` call and replace with a background spawn.
### Re: simpler — just show the right pane

You're right, I overcomplicated it. The desired behavior is straightforward:

**Select file → show its pane.** If no pane exists, create one (Claude starts loading in it — that's fine). If another file is selected mid-load, show that file's pane instead.

**The 3-pane bug:** The current code creates the new pane via `split-window` in the visible window BEFORE stashing the old pane. The correct sequence should be:

1. Stash the old pane (now window has 1 pane)
2. Create/join the new pane (now window has 2 panes)

Or even simpler: `split-window` in stash, then `join-pane` to the visible window — atomic swap.

**Why it's currently 3 panes:** The sync follows ATTACH-before-DETACH (lesson #3), which makes sense for *existing* panes (join first to avoid losing focus). But for *auto-start* (creating new panes), the order should be reversed — stash first, then create. The ATTACH-before-DETACH rule applies to pane *movement*, not pane *creation*.

Is it better to have a true atomic swap with tmux? 2 -> 2 panes?
If not...It's better to have 2 -> 3 -> 2 than 2 -> 1 -> 2 panes.

This should be quick sequence of running the tmux commands. No waiting for Claude to start.

**The actual fix is small:**
In `sync.rs`, when `auto_start` is needed, stash the unwanted pane first, then split-window in the visible window. No background process needed. No waiting needed. Claude loading in the pane is visible to the user as expected.

Want me to implement this? It's a reorder of operations in the sync reconciliation path.
### Re: 2→3→2 is fine, just no waiting

You're right — tmux doesn't have an atomic swap. The choices are:

- **2→3→2** (current ATTACH-before-DETACH): split-window creates pane 3, then stash old pane. Brief 3-pane flash.
- **2→1→2** (DETACH-before-ATTACH): stash old pane first, then split-window. Brief 1-pane flash.

Both are fine if the sequence is fast (~50ms of tmux commands). The real problem is the **8-second `wait_for_claude_ready`** blocking between the split-window and the stash.

**The fix: remove `wait_for_claude_ready` from the sync path.**

Currently `auto_start_in_session()` in `route.rs` does:
1. `split-window` (create pane)
2. Send `agent-doc start <file>` (launches Claude)
3. `wait_for_claude_ready` ← **8 seconds blocking**
4. Return pane ID

Steps 1-2 are instant. Step 3 is the blocker. The sync path doesn't need to wait — it just needs the pane to exist with Claude starting in it. The user sees Claude loading, which is expected.

I'll remove the wait from the sync-initiated auto-start path. The `route` command (plugin hotkey) can keep the wait because it needs to send `/agent-doc` after Claude is ready.
continue
### Re: continue — implemented

Done. Added `auto_start_no_wait()` to `route.rs` and switched `sync.rs` to use it.

**What changed:**
- `route.rs`: Split `auto_start_in_session` into two paths via `skip_wait` flag
  - `auto_start()` — full path with wait + send command (route hotkey)
  - `auto_start_no_wait()` — pane creation + Claude start only (sync)
- `sync.rs`: Changed `route::auto_start()` → `route::auto_start_no_wait()`

**What the user sees now when sync auto-starts:**
1. Sync stashes unwanted pane + creates new pane (fast tmux ops, ~50ms)
2. New pane shows Claude loading prompt
3. Sync returns immediately — no 8-second wait
4. User sees Claude initializing in the pane

All 526 tests pass. Release build successful. Want me to release v0.25.10?
update SPEC.md, README.md
audit docs
release
### Re: update docs + audit + release — v0.25.10 done

**v0.25.10 published to all targets:**
- crates.io ✓ | PyPI ✓ | GitHub Release ✓ | `cargo install` ✓

**Docs updated:**
- README.md: version line → v0.25.10 / JetBrains v0.2.21, added 5 feature bullets
- SPEC.md: sections 7.27-7.31 for new features
- Audit docs: clean

**Changes in v0.25.10:**
1. Preflight mtime debounce (500ms idle gate)
2. Unified diff with 5-line context radius
3. `--debounce` flag on route
4. `is_tracked()` FFI export + untracked file debounce fix
5. `auto_start_no_wait()` — sync no longer blocks on Claude ready
6. Plugin v0.2.21 — sync logging at INFO level
7. `looks_truncated` improvements + recheck interval doubled
On the right editor pane:
I switched from tasks/resume.md to tasks/calendar.md

The tmux session briefly switched to window 1:stash then back to 0:agent-doc.
Why?
### Re: stash flicker on calendar switch — still happening but faster

The `auto_start_no_wait` fix IS working — the calendar sync completed in **~600ms** (vs 8 seconds before). But the stash flicker still occurs during the sync reconciliation itself.

**What the logs show:**
```
00:19:17.857  [layout] resume → calendar detected
00:19:18.069  [layout-sync] exec: sync --col agent-doc.md --col calendar.md
00:19:18.680  [sync] exit=0, output: auto-starting calendar.md
00:19:18.883  [layout-sync] result: exit=0
```

~600ms total, no 8-second block. But during those 600ms, the sync does:
1. Stash the resume pane → `-d` flag, but tmux briefly renders stash
2. Auto-start calendar pane (split-window + send start)
3. Reconcile layout

The flicker happens during step 1. Even with `-d` on `stash_pane`/`break_pane`, tmux renders the stash window for a frame before the next command selects the agent-doc window back.

**This is the tmux rendering race we identified earlier.** The code is correct — `-d` flags prevent focus *changes* but not the rendering of intermediate states between commands.

**Current sync output is truncated at 500 chars.** I should increase that limit to capture the full DETACH/ATTACH logs for definitive diagnosis. But the root cause is clear: tmux has no batch-rendering mode.

**Realistic options:**
1. **Accept ~100ms flicker** during pane swaps (current state)
2. **Investigate `swap-pane`** — tmux has a `swap-pane` command that might do an atomic swap without intermediate states
Do:
- [ ] **Investigate tmux `swap-pane`** — might eliminate flicker by atomic swap instead of stash+join
### Re: swap-pane investigation (HEAD)

**`swap-pane` is the right solution.** It's a single tmux command that atomically swaps two panes, even across windows.

```
tmux swap-pane -s %stash_pane -t %visible_pane -d
```

`-d` prevents focus change. The old pane goes to the stash position, the new pane appears in the visible window. No intermediate 1-pane or 3-pane state.

**Current flow (ATTACH then DETACH):**
```
join_pane(stash→visible)  → 3 panes briefly
stash_pane(old→stash)     → 2 panes
```

**With swap-pane:**
```
swap_pane(stash↔visible)  → always 2 panes
```

**Where it fits in tmux-router:**
The `reconcile()` function in `sync.rs` does ATTACH → DETACH in sequence. For the common case of 1:1 replacement (one pane out, one pane in), it can use `swap-pane` instead. Multi-pane changes (2→3 columns) would still use the current join+stash approach.

**Implementation plan:**
1. Add `swap_pane(&self, src: &str, dst: &str) -> Result<()>` to `Tmux` struct
2. In `reconcile()`, detect 1:1 swap case: exactly one pane to ATTACH and one to DETACH
3. Use `swap-pane -d` for the 1:1 case, fall back to join+stash for complex changes

This change is in `tmux-router` (the library crate), not `agent-doc`. Want me to implement it?
<!-- agent:boundary:21e45818 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [ ] **Implement swap-pane in tmux-router** — atomic pane swap for 1:1 replacements, eliminates flicker
<!-- /agent:pending -->

<!-- agent:todo -->
- [ ] Install script (`curl | sh`) for binary download ([#30](https://github.com/btakita/agent-doc/issues/30))
- [ ] Homebrew formula (`brew tap btakita/agent-doc`) ([#31](https://github.com/btakita/agent-doc/issues/31))
- [ ] **lazily-rs DocContext** -- cache find_code_ranges() ([#32](https://github.com/btakita/agent-doc/issues/32))
- [ ] Support `/btw` for instant processing ([#33](https://github.com/btakita/agent-doc/issues/33))
- [ ] Investigate ClawTeam for agent-doc integration ([#34](https://github.com/btakita/agent-doc/issues/34))
- [ ] Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- [ ] Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))
- [ ] Plugin phases 5-7 — component folding, gutter icons, structure view ([#19](https://github.com/btakita/agent-doc/issues/19))
- [ ] Multi-agent backends — codex, gemini ([#20](https://github.com/btakita/agent-doc/issues/20))
- [ ] `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))
- [ ] `agent-doc status` — Ratatui TUI dashboard ([#22](https://github.com/btakita/agent-doc/issues/22))
- [ ] Sub-profiles for submodule workflows ([#23](https://github.com/btakita/agent-doc/issues/23))
- [ ] Docker sandboxing (`--sandboxed`) ([#24](https://github.com/btakita/agent-doc/issues/24))
- [ ] `agent/api.rs` — direct HTTP API backend ([#25](https://github.com/btakita/agent-doc/issues/25))
- [ ] `agent-doc init --dashboard` ([#26](https://github.com/btakita/agent-doc/issues/26))
- [ ] Capture/log skill workflow ([#27](https://github.com/btakita/agent-doc/issues/27))
- [ ] Blog post: document modes with screencast ([#28](https://github.com/btakita/agent-doc/issues/28))
- [ ] `agent-doc stream` — bridge terminal to components ([#29](https://github.com/btakita/agent-doc/issues/29))
- [ ] **MDX support** — `.mdx` file format with JSX component parsing ([#35](https://github.com/btakita/agent-doc/issues/35))
- [ ] **Jupyter notebook support** — `.ipynb` cell-level sessions ([#36](https://github.com/btakita/agent-doc/issues/36))
- [ ] **Notion integration** — sync agent-doc sessions with Notion pages ([#37](https://github.com/btakita/agent-doc/issues/37))
<!-- /agent:todo -->
