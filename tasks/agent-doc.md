---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.18.0 | **Tests:** 446 (152 lib + 273 bin + 21 int) | **Dependencies:** tmux-router v0.2.6, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.6 (JNA FFI + Gson + IPC) + VS Code v0.2.3 (prepend fix)
**Library:** `libagent_doc.so` (1.7M) — 6 FFI exports | **New:** undo, extract, transfer
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
| v0.17.29-v0.17.30 | JNA FFI integration, `merge_frontmatter()` export, `lib-path` command, Gson parser, VS Code prepend fix, immutable session binding. 440 tests. |
| v0.18.0 | `undo` (pre-response restore), `extract` (move last exchange entry), `transfer` (move component content). Pre-response snapshot storage. JetBrains v0.2.6, VS Code v0.2.3. 446 tests. |
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
15. **No Gson in JetBrains plugin** — Gson causes `ClassNotFoundException` at runtime in some IntelliJ builds. Use hand-written JSON parsing. This bug was found and fixed twice (SlashCommandCompletionContributor Mar 11, PatchWatcher Mar 18) — check git history before introducing "bundled" dependencies.
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

**v0.17.0-v0.17.30:** Split `agent_doc_mode` into `agent_doc_format` + `agent_doc_write`. IPC write + JetBrains PatchWatcher. JNA FFI bindings (NativeLib.kt) with Kotlin fallback. Gson JSON parser. VS Code prepend mode fix. Immutable `tmux_session` binding (`--force`). `lib-path` command. `merge_frontmatter()` FFI export.

**v0.18.0:** `undo` (pre-response snapshot restore), `extract` (move last exchange entry between docs), `transfer` (move full component between docs). Pre-response snapshot storage in all write paths. JetBrains v0.2.6, VS Code v0.2.3. 446 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: H2 Headers Before Components (Canonical Pattern)

Done. Updated two places:

**1. SKILL.md** — added canonical pattern documentation in the "Template document conventions" section:
```markdown
**Canonical pattern: h2 header before each component.** Every component 
should be preceded by an `##` heading. This enables IDE code folding — 
users can collapse sections to focus on the active area.
```

**2. `claim.rs` scaffolding** — default components now include h2 headers:
```markdown
## Status
&lt;!-- agent:status patch=replace --&gt;
&lt;!-- /agent:status --&gt;

## Exchange
&lt;!-- agent:exchange patch=append --&gt;
&lt;!-- /agent:exchange --&gt;

## Pending / Not Built
&lt;!-- agent:pending patch=replace --&gt;
&lt;!-- /agent:pending --&gt;
```

Previously the scaffolding had bare components without headers. New claims will get the h2 headers automatically.

`agent:exchange` should use `patch=append` by default.

Bug: The JB Plugin does not show the choice modal.
Is this a regression?
What happened?

### Re: Gson Regression + Exchange Default

**JetBrains plugin bug found and fixed.** Root cause: Gson `ClassNotFoundException` at runtime.

This was already discovered and fixed on March 11 (commit 908b743) when it was removed from `SlashCommandCompletionContributor`. I re-introduced it in `PatchWatcher` on March 18 (commit f968386) without checking the prior removal. The `ClassNotFoundException` during `PatchWatcher` initialization silently broke the entire action chain, preventing the popup from appearing.

**Fix:** Reverted `parsePatchJson()` back to the hand-written parser. Committed and pushed. Plugin v0.2.6 rebuilt (unsigned + signed zips).

**Lesson: Gson is not reliably available at runtime in all IntelliJ builds.** The `build.gradle.kts` depends on `intellijIdeaCommunity` which may not bundle Gson in all configurations. Always use hand-written JSON parsing in the plugin.

**Exchange default:** The `patch=append` attribute is already the default behavior via `components.toml`. Adding it to the inline tag is cosmetic but makes the intent explicit. The claim scaffolding already uses `patch=append` in the tag.

Please reinstall the plugin from `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.6.zip` and verify the popup appears.

Should we release the crate package as well, or will this replace the distribution on the github packages page?

btw, there are some agent-doc windows in tmux session 1. This should not have happened. Why were they moved to tmux session 1?

### Re: Release + Tmux Session 1 Panes

**Release:** The Gson revert only affects the JetBrains plugin (Kotlin code), not the Rust binary. crates.io v0.18.0 is fine — the binary never used Gson. However, we should release v0.18.1 to include the claim scaffolding h2 headers fix and keep the tag pointing to the correct commit. The GitHub Release binary would then include all fixes.

Want me to proceed with v0.18.1?

**Tmux session 1 panes:** Found 5 panes in session `1` that all have `tmux_session: 0` in frontmatter:

| File | Pane | Actual Session | Frontmatter |
|------|------|---------------|-------------|
| corky-public-google-app.md | %86 | 1 | 0 |
| ripple.md | %71 | 1 | 0 |
| agent-doc-bugs.md | %24 | 1 | 0 |
| agent-loop.md | %84 | 1 | 0 |
| corky.md | %85 | 1 | 0 |

The immutable binding fix (v0.17.30) prevents `claim.rs` from overwriting `tmux_session` in frontmatter. But it doesn't prevent the panes themselves from being moved at the tmux level (manual `move-pane`, `join-pane` targeting wrong session, or `auto_start` creating panes in the wrong session before the validation was added).

**Root cause:** These panes were likely created or moved before the v0.17.30 fix was installed. The fix prevents *future* frontmatter rewrites, but existing misplaced panes need manual correction or `resync --fix`.

**To fix now:** `agent-doc resync --fix` should detect and kill the wrong-session panes. On next route, they'll be auto-started in the correct session (0).

**Design gap:** `route.rs` already validates session on route (lines 66-87), but `auto_start` may still create panes in the fallback session if the document's tmux_session didn't exist at auto-start time. The fallback logic should be tighter — refuse to start rather than start in the wrong session.

Will this update help recover existing tmux panes in tmux session 1? Should I just manually close those panes? Or should I move those panes into tmux session 0?

release
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
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
