---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.13 | **Tests:** 526 (173 lib + 315 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.10, instruction-files v0.1
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
Session 3: v0.25.12 — sync swap-pane, no-wait auto-start, cross-session auto-repair, tmux_session frontmatter repair, resync --fix disabled, split in visible window, (HEAD) marker investigation, GitHub Actions CI fix, plugin v0.2.21 sync logging.
Exchange compacted. 260 lines archived.
Implement:
- [ ] Install script (`curl | sh`) for binary download ([#30](https://github.com/btakita/agent-doc/issues/30))
- [ ] Homebrew formula (`brew tap btakita/agent-doc`) ([#31](https://github.com/btakita/agent-doc/issues/31))
### Re: Implement #30 + #31

**#30 — Install script** (`install.sh`):
- Platform detection (linux/macos) + arch (x86_64/aarch64)
- Fetches latest release tag from GitHub API
- Downloads binary directly (no tar extraction)
- Installs to `~/.cargo/bin/` or `~/.local/bin/`
- Verifies with `agent-doc --version`
- PATH instructions if needed

Usage: `curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh`

**#31 — Homebrew formula** (`Formula/agent-doc.rb`):
- `on_macos` (Intel + ARM) + `on_linux` platform blocks
- Downloads prebuilt binary from GitHub Releases
- SHA256 checksums placeholder (`FIXME` — need actual checksums per release)
- Version: 0.25.12

To publish: copy to `homebrew-agent-doc` repo, fill checksums, push. Users install with `brew install btakita/agent-doc/agent-doc`.

Both files need to be committed and the install.sh needs to be included in the next release. Want me to commit and release v0.25.13?
Implement:
- [ ] Support `/btw` for instant processing ([#33](https://github.com/btakita/agent-doc/issues/33))
### Re: #33 /btw — design analysis

`/btw` for instant processing requires two levels:

**Level 1: Side-channel messaging (achievable now)**
Write urgent messages to `.agent-doc/btw/<session-hash>.txt`. The SKILL checks this during response generation (at streaming checkpoints). If a btw message exists, the agent incorporates it into the current response.

Implementation:
- `agent-doc btw <file> "message"` — writes to the btw file
- SKILL step 1 checks for btw messages before/during response
- Claims log already does something similar but only at preflight

**Level 2: True preemption (requires Claude Code support)**
Interrupt the currently running Claude process and inject a message. This would need:
- Claude Code's `/btw` slash command (if available)
- Or: `tmux send-keys` to inject text into the Claude pane's input while it's thinking
- Or: a signal mechanism that causes Claude to check the btw file mid-generation

Level 1 is implementable now. Level 2 depends on Claude Code's capabilities. Which level do you want?
Can level 2 be solved with the agent-doc binary parsing the document?
### Re: Can the binary solve level 2?

Yes — here's how:

**The binary watches the document file.** While Claude is processing, the user edits the document (adds a `/btw` line or writes in the exchange component). The binary detects the file change and:

1. `agent-doc watch` (already exists) detects the file change
2. Instead of triggering a new `/agent-doc` submission, it writes the new content to a **btw file** (`.agent-doc/btw/<hash>.txt`)
3. The **SKILL checks the btw file at streaming checkpoints** (during `agent-doc write --stream` calls)
4. If btw content exists, the agent reads it and adjusts the response

**The key insight:** The document IS the btw channel. The user types in the document while Claude is responding. The CRDT merge preserves both the user's edits and the agent's response. The agent just needs to READ the document at checkpoints to see if the user added new content.

**Simplest implementation:**
- During streaming checkpoints (step 1 in SKILL), re-read the document to check for new user edits since the baseline
- If new content was added below the boundary marker, incorporate it as a btw message
- This works because `--stream` (CRDT) already handles concurrent edits — no data loss

This means `/btw` is essentially **the document itself** — the user types while the agent is responding, and the agent sees it at the next checkpoint. No new binary feature needed — just a SKILL instruction to re-read the document at checkpoints.

The binary role: ensure `agent-doc write --stream` merges correctly (already does via CRDT).
There are many stash windows in tmux session 0.
The currently selected window is 1:stash
### Re: stash window buildup + wrong selection

**Cleaned up:** Killed 19 orphaned stash windows. Session 0 now has 2 windows: `0:agent-doc` (1p) + `1:stash` (11p). Selected window set back to `0:agent-doc`.

**Root cause:** Each failed auto-start for email.md created a pane via `split-window`, the pane died (Claude failed to start), the dead pane became a shell, and the next sync stashed it — creating a new 1-pane stash window. 20+ failed attempts = 20+ stash windows.

**The fix needed:** Auto-start should not retry indefinitely for files that can't start Claude. Add a cooldown: if a file's pane died within the last N seconds, don't auto-start again. This prevents the stash window buildup.

The stash window `1:stash` with 11 panes is from legitimately stashed sessions — those are fine.
Why where there failed attempts?
And why would the failed attempt create a new stash window?
### Re: Why failed attempts? Why new stash windows?

**Why failed attempts:** The `resync --fix` was killing the pane immediately after auto-start created it (see earlier root cause: cross-session mismatch between frontmatter `tmux_session: '1'` and the sync's target session `0`). Each plugin tab-switch triggered a sync → auto-start → pane created → resync killed it → next sync sees no pane → auto-start again. Infinite loop.

It should have "self-healed" the tmux_session in the document. I'm also wondering if it even makes sense to configure a tmux_session since we always want to use the bound tmux session for the agent-doc project.

**Why new stash windows:** When `auto_start_in_session` uses `split-window` to create a new pane, it splits alongside an existing pane. After the new pane dies, the shell from `split-window` remains briefly. The next sync reconcile sees this shell as "unwanted" and stashes it. But instead of joining the existing stash window (`1:stash`), `break_pane -d` creates a NEW detached window (which tmux names "stash" because it inherits the window-naming pattern). Each failed attempt → one orphan shell → one new stash window.

**The fix (already applied):** `resync --fix` is now disabled in sync path (`resync::run(false)`), and tmux_session is auto-repaired in frontmatter. The cycle is broken. But the stash window proliferation bug remains if panes die for other reasons. The `stash_pane` function should always target the existing stash window via `join-pane`, not `break_pane` (which creates new windows).

yes
### Re: self-heal + remove tmux_session? + stash fix

**On `tmux_session` as a frontmatter field:**
You're right — the project's `.agent-doc/` directory already defines where sessions live. The `tmux_session` field was originally for multi-session support (#17), but in practice every file in a project uses the same session. The auto-repair we just added effectively makes `tmux_session` in frontmatter redundant — the sync always overrides it with the project's session anyway.

**Proposal:** Deprecate `tmux_session` in frontmatter. Instead:
1. The project session is determined by the `--window` argument (from sync) or `current_tmux_session()` (from route)
2. Remove `tmux_session` from frontmatter entirely — the binary handles session binding at runtime
3. For the multi-session use case (#17), add a project-level config in `.agent-doc/config.toml` instead

agreed.
Update tests, SPEC.md, README.md

This simplifies things significantly. But it's a bigger refactor — for now, the auto-repair ensures it works correctly.

**On the stash fix:** The `stash_pane` function in tmux-router already uses `join-pane -dv` (line 532 of tmux.rs) to join the existing stash. The `break_pane -d` fallback only triggers when `join-pane` fails (stash window too small, join error). The orphan windows came from the `break_pane_to_stash` fallback path. I'll look at making the stash window resize more aggressive to prevent the join failure, but this is a tmux-router change. Tabling for the next session.

Implement now

Should I compact and release what we have?
### Re: Deprecate tmux_session — done

Deprecated `tmux_session` in frontmatter:
- `frontmatter.rs`: Field marked deprecated in doc comments, `set_tmux_session()` deprecated
- `README.md`: Frontmatter table shows Deprecated marker
- `SPEC.md`: Deprecation callout + renamed auto-repair section

No code behavior changes — the auto-repair and context_session override continue to work. The field is still read/written for backward compatibility. `make check` passes.

Should we have the current agent-doc release delete tmux_session in the frontmatter?

Ready to release?
### Re: Implement stash fix + strip tmux_session?

**On stripping tmux_session:** No — don't delete it yet. The sync auto-repair already writes the correct value, so existing files work. Stripping would be a breaking change for users on older binaries that still need the field. Better to just ignore it at runtime (which we already do via context_session priority) and let it decay naturally as files get rewritten.

I don't share my documents with anyone. Not many users. So it's ok to strip.

**On the stash fix:** I'll implement more aggressive stash resize in tmux-router now. The issue is `stash_pane` falls back to `break_pane_to_stash` when `join-pane` fails due to "pane too small". The fix: resize the stash window to a larger height before attempting the join.

Let me check the current stash resize logic and make it more robust.
### Re: Stash join fix — done

Changed `stash_pane` in tmux-router:
1. Resize from 200 → 1000 rows (prevents "pane too small")
2. On join failure: **kill the pane** instead of `break_pane_to_stash` (prevents orphan stash windows)

The tradeoff: if join fails, the pane is lost instead of creating an orphan window. This is the right choice — an unwanted pane that can't be stashed should be killed, not scattered into new windows.

All 526 tests pass. Ready for v0.25.13 release.
/btw Is SPEC loaded into every context?
### Re: v0.25.13 released (HEAD)

**v0.25.13 published:**
- crates.io ✓ | PyPI ✓ | GitHub Release ✓ | `cargo install` ✓

**Changes:**
1. `install.sh` — curl | sh installer (#30)
2. `Formula/agent-doc.rb` — Homebrew formula (#31)
3. `tmux_session` stripping — sync removes deprecated field from frontmatter
4. Stash join fix — kill on failure instead of orphan window creation (1000 row resize)
5. `tmux_session` deprecation — docs/comments updated
<!-- agent:boundary:c4a2b5bd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] v0.25.13 released — install.sh + formula + tmux_session strip + stash fix
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
