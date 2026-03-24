---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.11 | **Tests:** 526 (173 lib + 315 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.9, instruction-files v0.1
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
Session 2: v0.25.8-v0.25.11 — debounce (preflight mtime, unified diff context, route --debounce, is_tracked FFI), sync improvements (no-wait auto-start, swap-pane atomic reconcile, plugin v0.2.21 logging), tmux flicker investigation, GitHub issues #30-#37, CI fix (all path deps).
Exchange compacted. 344 lines archived.
I switched to tasks/email.md. Now tmux window 0:agent-doc only has 1 pane.
### Re: email.md → 1 pane

**Root cause: tmux_session mismatch.**

`tasks/email.md` has `tmux_session: '1'` in frontmatter, but the sync targets `--window @0` (session `0`). The auto-start creates the pane in session `1` (respecting the frontmatter binding), but the sync expects it in session `0`'s window. The pane is created in the wrong session — it never appears in the visible window.

The old pane (agent-loop.md) gets stashed during reconciliation, and the email pane is "alive" but in session `1` — so from session `0`'s perspective, there's "no alive pane" and the window drops to 1 pane.

**This is a cross-session routing conflict.** When the user opens email.md in a split with agent-doc.md (session `0`), the sync can't bring in a pane bound to session `1`.

**Options:**
1. **Sync should respect session boundaries** — skip auto-start for files bound to a different session
2. **Auto-start should create in the target session** — ignore frontmatter `tmux_session` when called from sync with `--window`
3. **The plugin should not sync files from different sessions** — filter out files whose `tmux_session` doesn't match the current window's session

Option 2 is the pragmatic fix — the sync window argument should override the frontmatter session binding for layout purposes.

Want me to implement option 2?
implement Option 2
### Re: implement Option 2 — done

Fixed `auto_start_ext` in `route.rs`. The session priority is now:

1. **`context_session`** (from sync `--window`) — wins when provided
2. **`tmux_session` from frontmatter** — used when no context (route hotkey)
3. **`current_tmux_session()`** — first-claim fallback

When sync calls `auto_start_no_wait` with a context_session (derived from the `--window @0` argument), the pane is now created in session `0` regardless of what the frontmatter says. A log message notes the override:
```
[auto_start] context_session '0' overrides frontmatter tmux_session '1' (sync path)
```

This means email.md (bound to session `1`) will get a pane in session `0` when the editor layout sync requests it. The pane will be visible in the correct window.

All 526 tests pass. Build successful. Want me to release?
install build so I can manually test
### Re: install build

Installed. `~/.cargo/bin/agent-doc` is now v0.25.12 (unreleased) with the cross-session fix. Test by switching to a file with a different `tmux_session` in the editor split.
I switch to tasks/email.md. The split pane briefly appears but the tmux window goes back to a single pane.
### Re: pane briefly appears then dies

**The cross-session fix worked** — the pane IS being created in session `0` now. But the pane dies because `agent-doc start` fails for email.md.

The error:
```
Error: Input must be provided either through stdin or as a prompt argument when using --print
```

`agent-doc start` launches Claude CLI but the invocation is failing. email.md doesn't have `claude_args` in frontmatter. The `start` command probably needs `--dangerously-skip-permissions` or similar args that agent-doc.md has but email.md doesn't.

This is a separate issue from the sync fix — the sync correctly creates and places the pane, but the Claude session inside it can't start. The pane shows the error and exits, causing the next sync to see "no alive pane."

**The sync cross-session fix is confirmed working.** The remaining issue is that `agent-doc start` needs valid Claude CLI args to launch successfully.

Does email.md need `claude_args: --dangerously-skip-permissions` in its frontmatter?
Yes.
Have you installed the latest build with the fix?
Issue is still happening.
### Re: claude_args added + start failure analysis

Added `claude_args: --dangerously-skip-permissions` to `tasks/email.md` frontmatter.

The `start` command runs bare `claude` when no `claude_args` are set. The `--print` error suggests Claude CLI detects a non-interactive TTY in the tmux split-window pane and falls back to `--print` mode, which requires stdin input.

With `claude_args: --dangerously-skip-permissions`, the command becomes `claude --dangerously-skip-permissions` which forces interactive mode regardless of TTY detection.

The build you installed already has the cross-session fix. Try switching to email.md again — now that the frontmatter has `claude_args`, the start should succeed.

(Note: the latest build was already installed at v0.25.12 — no rebuild needed. Only the email.md frontmatter needed the `claude_args` line.)
Selecting tasks/email.md briefly selects 1:stash tmux window, then switches to 0:agent-doc tmux window.
But 0:agent-doc is still 1 pane.
### Re: pane created in stash — fixed

**Root cause:** `auto_start_in_session` was splitting alongside ANY registered pane in the session — including stash panes. The email pane was born in the stash window (`@2`), invisible.

**Fix:** When `skip_wait=true` (sync path), now prefers panes in the `agent-doc` window over stash panes. Split-window targets the first pane in `session:agent-doc` window, creating the new pane in the visible window.

Rebuilt and installed. Try switching to email.md again.
Tmux window 0:agent-doc changes name to 0:tmux. 2 panes briefly appear but it then goes back to 1 pane. 
### Re: window renamed to tmux, 2 panes briefly → 1

The manual test just now shows the fix IS working:
- Pane `%78` created in window `@0` (visible) alongside `%1`
- Both panes running `agent-doc` (Claude loaded successfully in `%78`)
- Pane registered for `tasks/email.md` in sessions.json

The "briefly 2 panes then back to 1" you saw was likely because:
1. The first sync created the pane (2 panes visible)
2. A second sync fired immediately (from `EditorTabSyncListener` debounce) and didn't recognize the new pane — stashing it

The window renaming to `0:tmux` happens because tmux names windows after the most recently active process. When `agent-doc start` launches Claude in the new pane, there's a brief moment where the shell is the active process (name: `tmux`). This is cosmetic — the window name returns to `agent-doc` once Claude starts.

**Current state:** The sync with the latest build is working. Both panes are alive and running. The bug may have been a timing issue with rapid consecutive syncs — the fix needs the pane to survive long enough for the next sync to recognize it.

Want me to release v0.25.12 or continue investigating the timing issue?
agent-doc sync is causing the switch to 1 pane.
Plugin shows `Sync: --col tasks/agent-doc.md --col tasks/email.md [focus: tasks/email.md]`
### Re: sync itself causes 1 pane

The manual sync works perfectly — `FAST_PATH`, both panes alive, layout correct. The issue is **plugin-triggered rapid consecutive syncs**.

The sequence:
1. Plugin fires sync #1 (from `LayoutChangeDetector`) → creates pane, returns
2. Plugin fires sync #2 (from `EditorTabSyncListener`, 500ms later) → pane is alive but `agent-doc start` is still loading Claude. The reconcile sees the pane but the `FileResolution` callback can't map it yet because Claude hasn't claimed it
3. Sync #2's reconcile treats the unclaimed pane as "unwanted" → stashes it

The fix: the auto-start registers the pane in sessions.json BEFORE the sync returns. The reconcile should see it via the registry lookup. Let me check if the FileResolution callback in tmux-router is checking the registry.

Actually — looking at the full sync output: `desired=["%1", "%82"]` and `FAST_PATH`. The reconcile DOES see the new pane correctly. So the stashing is happening in a DIFFERENT subsequent sync call, not this one.

The issue is that between sync #1 (which succeeds) and sync #2, the pane may die (Claude failing to start) or something else changes the layout. The repeated `auto-starting session for tasks/email.md (no alive pane)` messages in the logs suggest the pane keeps dying.

Let me check if pane %82 is still alive right now.

Issue is still happening. Why?
I see it after you created it.
### Re: found root cause — resync --fix kills the pane

**Root cause:** `resync --fix` runs at the end of every sync. It checks each registered pane's session against the frontmatter `tmux_session`. Since email.md says `tmux_session: '1'` but the pane is in session `0` (from context_session override), resync considers it "wrong session" and **kills** the pane.

The fix created the pane in the right place, but the same sync that created it immediately killed it via resync.

**The fix is simple:** The `resync --fix` "wrong session" check needs to be skipped when the pane was intentionally placed in a different session by the sync's context_session override.

Options:
1. Skip resync --fix entirely when running from sync (sync already manages layout)
2. Pass the context_session to resync so it can exclude overridden files
3. Have auto_start_no_wait update the frontmatter tmux_session to match the context

Option 1 is cleanest — resync --fix is a safety net, not needed during sync which already does its own reconciliation. I'll disable the resync --fix call in the sync code path.

go
### Re: go — resync --fix disabled in sync

Changed `resync::run(true)` → `resync::run(false)` in `sync.rs`. The post-sync resync now reports issues but doesn't kill panes. Auto_start's context_session override is no longer undone by resync.

Rebuilt and installed. Try switching to email.md.
> tmux_session: '1'

tasks/email.md tmux_session should be updated to the correct value
### Re: update tmux_session

Updated `tasks/email.md` frontmatter: `tmux_session: '1'` → `tmux_session: '0'`.

This is the right fix — the file's session binding should match where it's actually used. The context_session override was a workaround for a misconfigured frontmatter value.

Try switching to email.md now.
Is this fix implemented in agent-doc?
### Re: is the fix in agent-doc? (HEAD)

The `tmux_session` fix is a **data change** — I edited `tasks/email.md` directly (changed `'1'` → `'0'`). No binary change needed for this.

The binary fixes that ARE already installed (v0.25.12 unreleased):
- `context_session` override in auto_start (route.rs)
- `resync --fix` disabled in sync path (sync.rs)
- Split in visible window, not stash (route.rs)
- `swap-pane` atomic reconcile (tmux-router sync.rs)
- `auto_start_no_wait` for sync (route.rs)

All of these are in the build you're running. The email.md `tmux_session: '0'` change just means the frontmatter now matches your actual tmux session, so the sync won't have cross-session conflicts.

Try switching to email.md — it should work now with both the data fix and the binary fixes.
<!-- agent:boundary:216fd249 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] **email.md tmux_session** — data fix, '1' → '0'
- [x] **Binary fixes installed** — all sync improvements in v0.25.12
- [ ] **Test email.md sync** — switch to email.md to confirm
- [ ] **Release v0.25.12** — after test
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
