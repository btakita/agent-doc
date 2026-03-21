---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.23.3 | **Tests:** 459 (162 lib + 297 bin) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✗ (pending)
**Plugins:** JetBrains v0.2.8 (boundary markers + claim auto-start) + VS Code v0.2.3
**Library:** \`libagent_doc.so\` — 8 FFI exports | **SKILL.md:** boundary marker step + immediate commit
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
| v0.9.7-v0.9.10 | \`agent-doc write\`, template mode, compaction recovery, \`recover\`, \`template-info\`, code-span parser fix. 192 tests. |
| v0.10.0-v0.10.1 | Renamed \`response_mode\` → \`agent_doc_mode\`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared \`merge.rs\`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | \`compact\`, \`convert\`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | \`autoclaim\` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.9 | Route focus fix, \`commands\` subcommand, plugin autocomplete (v0.2.0), bidirectional \`convert\`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, \`lib-path\`. 440 tests. |
| v0.18.0-v0.18.1 | \`undo\`, \`extract\`, \`transfer\`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). \`claims\` subcommand. 451 tests. |
| v0.21.0 | \`agent-doc parallel\` (fan-out with worktrees, \`--no-worktree\`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
| v0.22.0-v0.23.0 | Boundary marker ordering, claim auto-start, VCS refresh, any-level HEAD markers, exchange default mode, SKILL.md immediate commit, code-block boundary fix. JB v0.2.8. 452 tests. |
| v0.23.1-v0.23.2 | Stale boundary cleanup, boundary-aware insertion (unmatched + explicit patches), IPC snapshot \`content_ours\` fix, (HEAD) marker accumulation fix, examples/blog outline. 459 tests. |
| v0.23.3 | Cross-platform sync pane guard (\`ps\` instead of \`/proc\`), pre-sync duplicate pane prevention, clippy fixes. 459 tests. |
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

**v0.9.7-v0.12.2:** \`agent-doc write\`, template mode, compaction recovery, \`recover\`, \`exchange\` convention, append-friendly merge, \`compact\`/\`convert\`, lazy claim.

**v0.13.0-v0.14.9:** \`autoclaim\`, git-based snapshot fallback, \`commands\`, plugin autocomplete (v0.2.0), bidirectional \`convert\`, code-range-aware \`strip_comments\`, multi-backtick.

**v0.15.0-v0.16.1:** CRDT stream mode (yrs), chain-of-thought streaming, reactive file-watching, truncation detection, native compact. 309 tests.

**v0.17.0-v0.17.30:** Format/write split. IPC write + JetBrains PatchWatcher. JNA FFI (NativeLib.kt). VS Code prepend. Immutable \`tmux_session\` binding. \`lib-path\` + \`merge_frontmatter()\` FFI export.

**v0.18.0-v0.18.1:** \`undo\`, \`extract\`, \`transfer\`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3.

**v0.19.0:** ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7).

**v0.20.0-v0.20.3:** CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs, session affinity enforcement, numeric session fix (colon suffix), \`agent-doc claims\` subcommand. 451 tests.

**v0.21.0:** \`agent-doc parallel\` — fan-out with git worktrees (\`--no-worktree\` for read-only). CRDT post-merge reorder via Yrs attribution. 461 tests.

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7→v0.2.8. 452 tests.

**v0.23.1-v0.23.2:** Stale boundary cleanup, boundary-aware insertion (unmatched content + explicit patches), IPC snapshot \`content_ours\` fix, (HEAD) marker accumulation fix, examples + blog outline. 459 tests.

**v0.23.3:** Cross-platform sync pane guard (\`ps\` replaces \`/proc\`), pre-sync duplicate pane prevention, orphaned window cleanup, clippy fixes. 459 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260321-112447.md`*

### Re: Exchange Compacted

Archived 154 lines to \`.agent-doc/archives/\`.

**Session summary (v0.23.2 → v0.23.3):**

**Bugs fixed:**
- 3-pane duplicate: sync pre-start race with \`resync::prune()\` — added \`find_alive_pane_for_file()\` guard
- Orphaned window @33 — moved stray pane to stash, window auto-removed

**Improvements:**
- Cross-platform process inspection: \`ps -p <pid> -o command=\` replaces Linux-only \`/proc\`
- Clippy cleanup: collapsible_if, dead_code suppressions

**Released:** v0.23.3. 459 tests (162 lib + 297 bin).

I need to start to address new users.

Let's figure out how to make the agent-doc binary install, editor plugin install, and any other configuration seamless as possible.

### Re: New User Onboarding Plan

**Current install steps (too many):**
1. Install Rust toolchain (\`rustup\`)
2. \`cargo install agent-doc\`
3. Install tmux (system package manager)
4. Install Claude Code (\`npm i -g @anthropic-ai/claude-code\`)
5. Install editor plugin (\`agent-doc plugin install jetbrains\` or \`agent-doc plugin install vscode\`)
6. Install SKILL.md (\`agent-doc skill install\`)
7. Create a document (\`agent-doc init plan.md\`)
8. Claim a tmux pane (\`agent-doc claim plan.md\`)

**Goal: \`agent-doc setup\` — one command does everything.**

We have `agent-doc init` which initializes a new document.
Perhaps we should rename `agent-doc init` to `agent-doc new`. Or use a command tree to represent the document or session. `agent-doc session new` or `agent-doc document new` or `agent-doc md new`.

Then make this feature use `agent-doc init` instead of `agent-doc setup`.
This seems similar to `git init`

Steps 1-5 could also be a separate command. Which could be `agent-doc install` or `agent-doc update` or `agent-doc setup`.
Thoughts?

### Proposed \`agent-doc setup\` Command

\`\`\`bash
agent-doc setup [--editor jetbrains|vscode] [--no-plugin] [--no-skill]
\`\`\`

**What it does:**
1. **Check prerequisites** — tmux installed? Claude Code installed? Report missing deps with install instructions
2. **Install editor plugin** — auto-detect editor (IntelliJ/VS Code running?) or use \`--editor\` flag
3. **Install SKILL.md** — \`agent-doc skill install\` (if Claude Code's \`~/.claude/\` exists)
4. **Create \`.agent-doc/\` directory** — snapshots, patches, sessions.json
5. **Print quick-start guide** — "Create your first document: \`agent-doc init plan.md\`"

### Distribution Options

| Method | Audience | Effort |
|--------|----------|--------|
| \`cargo install agent-doc\` | Rust developers | Already done |
| \`pip install agent-doc\` (maturin) | Python developers | Already built, needs PyPI publish |
| \`brew install agent-doc\` | macOS users | Need Homebrew formula |
| GitHub Release binaries | Everyone | Already done (Linux x86_64) |
| \`npx agent-doc\` | Node developers | Need npm package wrapper |
| Install script (\`curl \| sh\`) | Quick start | Need to write |

**Recommended priority:**
1. **\`agent-doc setup\`** command (immediate — biggest UX win)
2. **Install script** (\`curl -fsSL https://agent-doc.dev/install.sh \| sh\`) — downloads binary + runs setup
3. **Homebrew formula** — \`brew tap btakita/agent-doc && brew install agent-doc\`
4. **PyPI publish** — already have maturin, just need to publish
5. **npm wrapper** — later (low priority)

### SKILL.md Auto-Install

Currently the skill must be manually installed. Options:
- **SessionStart hook** — add \`agent-doc skill install\` to Claude Code hooks (already partially done with autoclaim)
- **Plugin-triggered** — JetBrains/VS Code plugin runs \`agent-doc skill install\` on first launch
- **\`setup\` command** — installs the hook automatically

### Quick-Start README

The README should have a 3-line getting started:
\`\`\`bash
cargo install agent-doc
agent-doc setup
agent-doc init plan.md --mode template
\`\`\`

Want me to start implementing \`agent-doc setup\`?

### Re: Command Naming — init vs new vs setup

Agree with the \`git init\` analogy. Here's a proposed command structure:

**Renamed commands:**

| Current | New | Analogy |
|---------|-----|---------|
| \`agent-doc init plan.md\` | \`agent-doc new plan.md\` | Create a new document |
| (none) | \`agent-doc init\` | Initialize project (\`.agent-doc/\`, plugins, skill, hooks) |

**\`agent-doc init\`** = project-level setup (like \`git init\`):
- Creates \`.agent-doc/\` directory (snapshots, patches, sessions.json)
- Checks prerequisites (tmux, claude)
- Installs editor plugin (auto-detect or \`--editor\`)
- Installs SKILL.md
- Installs SessionStart hook for autoclaim
- Prints quick-start guide

**\`agent-doc new <file>\`** = create a document:
- Same as current \`agent-doc init <file>\`
- Scaffolds frontmatter + components

**For steps 1-5 (external deps):**
Those are pre-\`agent-doc\` steps — the user needs the binary before they can run \`agent-doc init\`. The install script (\`curl | sh\`) handles the binary download. Then \`agent-doc init\` handles everything else.

**Flow for a new user:**
\`\`\`bash
# 1. Install binary (one of these) (HEAD)
cargo install agent-doc          # Rust users
curl -fsSL .../install.sh | sh   # Everyone else

# 2. Initialize project (HEAD)
cd my-project
agent-doc init                   # creates .agent-doc/, installs plugin + skill

# 3. Create first document (HEAD)
agent-doc new plan.md --mode template

# 4. Start working (HEAD)
# Open plan.md in editor → plugin auto-starts tmux session (HEAD)
\`\`\`

**Backward compat:** Keep \`agent-doc init <file>\` working as an alias for \`agent-doc new <file>\` (with deprecation warning). If \`init\` is called with no args, it does the project setup.

**\`agent-doc update\`** could be a separate command that:
- Updates the binary itself (\`cargo install agent-doc\` or re-download)
- Updates editor plugins
- Updates SKILL.md

This keeps concerns cleanly separated: \`init\` = first time, \`update\` = keeping current.

Want me to implement this restructuring?
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
