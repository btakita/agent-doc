---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.24.0 | **Tests:** 497 (162 lib + 297 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
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
| v0.23.1-v0.23.2 | Stale boundary cleanup, boundary-aware insertion (unmatched + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples/blog outline. 459 tests. |
| v0.23.3 | Cross-platform sync pane guard (`ps` instead of `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests. |
| v0.24.0 | `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content validation tests. 497 tests. |
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

**v0.23.1-v0.23.2:** Stale boundary cleanup, boundary-aware insertion (unmatched content + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples + blog outline. 459 tests.

**v0.23.3:** Cross-platform sync pane guard (`ps` replaces `/proc`), pre-sync duplicate pane prevention, orphaned window cleanup, clippy fixes. 459 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260321-112447.md`*

### Re: Exchange Compacted

Archived 154 lines to `.agent-doc/archives/`.

**Session summary (v0.23.2 → v0.23.3):**

**Bugs fixed:**
- 3-pane duplicate: sync pre-start race with `resync::prune()` — added `find_alive_pane_for_file()` guard
- Orphaned window @33 — moved stray pane to stash, window auto-removed

**Improvements:**
- Cross-platform process inspection: `ps -p <pid> -o command=` replaces Linux-only `/proc`
- Clippy cleanup: collapsible_if, dead_code suppressions

**Released:** v0.23.3. 459 tests (162 lib + 297 bin).

I need to start to address new users.

Let's figure out how to make the agent-doc binary install, editor plugin install, and any other configuration seamless as possible.

### Re: New User Onboarding Plan

**Current install steps (too many):**
1. Install Rust toolchain (`rustup`)
2. `cargo install agent-doc`
3. Install tmux (system package manager)
4. Install Claude Code (`npm i -g @anthropic-ai/claude-code`)
5. Install editor plugin (`agent-doc plugin install jetbrains` or `agent-doc plugin install vscode`)
6. Install SKILL.md (`agent-doc skill install`)
7. Create a document (`agent-doc init plan.md`)
8. Claim a tmux pane (`agent-doc claim plan.md`)

**Goal: `agent-doc setup` — one command does everything.**

We have `agent-doc init` which initializes a new document.
Perhaps we should rename `agent-doc init` to `agent-doc new`. Or use a command tree to represent the document or session. `agent-doc session new` or `agent-doc document new` or `agent-doc md new`.

Then make this feature use `agent-doc init` instead of `agent-doc setup`.
This seems similar to `git init`

Steps 1-5 could also be a separate command. Which could be `agent-doc install` or `agent-doc update` or `agent-doc setup`.
Thoughts?

### Proposed `agent-doc setup` Command

```bash
agent-doc setup [--editor jetbrains|vscode] [--no-plugin] [--no-skill]
```

**What it does:**
1. **Check prerequisites** — tmux installed? Claude Code installed? Report missing deps with install instructions
2. **Install editor plugin** — auto-detect editor (IntelliJ/VS Code running?) or use `--editor` flag
3. **Install SKILL.md** — `agent-doc skill install` (if Claude Code's `~/.claude/` exists)
4. **Create `.agent-doc/` directory** — snapshots, patches, sessions.json
5. **Print quick-start guide** — "Create your first document: `agent-doc init plan.md`"

### Distribution Options

| Method | Audience | Effort |
|--------|----------|--------|
| `cargo install agent-doc` | Rust developers | Already done |
| `pip install agent-doc` (maturin) | Python developers | Already built, needs PyPI publish |
| `brew install agent-doc` | macOS users | Need Homebrew formula |
| GitHub Release binaries | Everyone | Already done (Linux x86_64) |
| `npx agent-doc` | Node developers | Need npm package wrapper |
| Install script (`curl \| sh`) | Quick start | Need to write |

**Recommended priority:**
1. **`agent-doc setup`** command (immediate — biggest UX win)
2. **Install script** (`curl -fsSL https://agent-doc.dev/install.sh \| sh`) — downloads binary + runs setup
3. **Homebrew formula** — `brew tap btakita/agent-doc && brew install agent-doc`
4. **PyPI publish** — already have maturin, just need to publish
5. **npm wrapper** — later (low priority)

### SKILL.md Auto-Install

Currently the skill must be manually installed. Options:
- **SessionStart hook** — add `agent-doc skill install` to Claude Code hooks (already partially done with autoclaim)
- **Plugin-triggered** — JetBrains/VS Code plugin runs `agent-doc skill install` on first launch
- **`setup` command** — installs the hook automatically

### Quick-Start README

The README should have a 3-line getting started:
```bash
cargo install agent-doc
agent-doc setup
agent-doc init plan.md --mode template
```

Want me to start implementing `agent-doc setup`?

### Re: Command Naming — init vs new vs setup

Agree with the `git init` analogy. Here's a proposed command structure:

**Renamed commands:**

| Current | New | Analogy |
|---------|-----|---------|
| `agent-doc init plan.md` | `agent-doc new plan.md` | Create a new document |
| (none) | `agent-doc init` | Initialize project (`.agent-doc/`, plugins, skill, hooks) |

**`agent-doc init`** = project-level setup (like `git init`):
- Creates `.agent-doc/` directory (snapshots, patches, sessions.json)
- Checks prerequisites (tmux, claude)
- Installs editor plugin (auto-detect or `--editor`)
- Installs SKILL.md
- Installs SessionStart hook for autoclaim
- Prints quick-start guide

**`agent-doc new <file>`** = create a document:
- Same as current `agent-doc init <file>`
- Scaffolds frontmatter + components

**For steps 1-5 (external deps):**
Those are pre-`agent-doc` steps — the user needs the binary before they can run `agent-doc init`. The install script (`curl | sh`) handles the binary download. Then `agent-doc init` handles everything else.

**Flow for a new user:**
```bash
# 1. Install binary (one of these)
cargo install agent-doc          # Rust users
curl -fsSL .../install.sh | sh   # Everyone else

# 2. Initialize project
cd my-project
agent-doc init                   # creates .agent-doc/, installs plugin + skill

# 3. Create first document
agent-doc new plan.md --mode template

# 4. Start working
# Open plan.md in editor → plugin auto-starts tmux session
```

We had an issue here. This text is in escaped ```
The inner h1 `#` has taken precedence over the patch response h3s `###`.

**Backward compat:** Keep `agent-doc init <file>` working as an alias for `agent-doc new <file>` (with deprecation warning). If `init` is called with no args, it does the project setup.

Should we just keep `agent-doc init <file>` as a separate mode as the canonical command to init a new agent document? Rather than having `agent-doc new <file>` with an alias `agent-doc init <file>`?

**`agent-doc update`** could be a separate command that:
- Updates the binary itself (`cargo install agent-doc` or re-download)
- Updates editor plugins
- Updates SKILL.md

This keeps concerns cleanly separated: `init` = first time, `update` = keeping current.

Want me to implement this restructuring?

I was thinking that `agent-doc init` would be on the project level. Mean project configuration such as `SKILL.md`
With `agent-doc install` being on the system level. Meaning binaries such as the `agent-doc` binary.
Thoughts?

### Re: Escaped Code Block Bug + Command Hierarchy

**1. Code block heading bug:** When I write a response containing fenced code blocks with `#` headings inside them, the `#` lines get interpreted as markdown headings rather than code. This is a SKILL.md issue — I need to properly escape code blocks in patch responses. I'll be more careful with code block content in future responses.

**2. Keep `agent-doc init <file>`** — agreed. No rename needed. `init` with a file arg creates a document, `init` without args initializes the project. Same command, two modes:

```
agent-doc init              # project-level: .agent-doc/, skill, hooks
agent-doc init <file>       # document-level: scaffold a new session doc
```

**3. Three-tier command hierarchy — this is clean:**

| Command | Scope | What it does |
|---------|-------|-------------|
| `agent-doc install` | **System** | Install/update the binary, editor plugins |
| `agent-doc init` | **Project** | Create `.agent-doc/`, install SKILL.md, configure hooks |
| `agent-doc init <file>` | **Document** | Scaffold a new session document |

**`agent-doc install`** (system-level):
- Self-update binary (check latest GitHub Release, download if newer)
- Install/update editor plugins (`--editor jetbrains\|vscode`)
- Check prerequisites (tmux, claude) and report missing deps
- Idempotent — safe to re-run

**`agent-doc init`** (project-level):
- Create `.agent-doc/` directory structure
- Install SKILL.md to `~/.claude/skills/agent-doc/`
- Add SessionStart hook for autoclaim (if not already present)
- Print project-specific quick-start

`agent-doc init` should lazily run `agent-doc install`

**New user flow:**
```
cargo install agent-doc      # get the binary
agent-doc install            # system setup (plugins, prereq check)
cd my-project
agent-doc init               # project setup (.agent-doc/, skill, hooks)
agent-doc init plan.md       # create first document
```

Want me to implement this hierarchy?

Yes

### Re: install + init Implemented

**`agent-doc install`** (system-level):
- Checks prerequisites: tmux, claude — prints status + install hints
- Auto-detects editors (JetBrains dirs, VS Code/Cursor/Codium binaries)
- Installs editor plugins via `plugin::install()`
- `--editor`, `--skip-prereqs`, `--skip-plugins` flags

**`agent-doc init`** (no args = project-level):
- Lazily calls `install::check_prereqs()`
- Creates `.agent-doc/snapshots/` and `.agent-doc/patches/`
- Installs SKILL.md
- Prints quick-start guide

**`agent-doc init <file>`** (document-level):
- Same as before (scaffolds session document)
- Lazily runs project init first if `.agent-doc/` doesn't exist

**Files:** `install.rs` (new), `init.rs` (updated), `main.rs` (updated), `test_cli.rs` (updated)

459 tests pass. Release binary installed.

Add automated tests. Preferably functional tests that test real behavior.

### Re: Functional Tests Added

9 new CLI integration tests in `tests/test_cli.rs`:

**install (3):**
- `test_cli_install_help` — verifies help output shows `skip-prereqs` and `skip-plugins` flags
- `test_cli_install_skip_all` — `--skip-prereqs --skip-plugins` succeeds without errors
- `test_cli_install_checks_prereqs` — stderr contains `tmux` and `claude` prereq checks

**init project-level (3):**
- `test_cli_init_creates_agent_doc_dir` — creates `.agent-doc/snapshots/` and `.agent-doc/patches/`
- `test_cli_init_idempotent` — running twice doesn't fail
- `test_cli_init_prints_quickstart` — output contains quick-start hints

**init document-level (3):**
- `test_cli_init_file_creates_document` — creates file with `agent_doc_session:` frontmatter
- `test_cli_init_file_with_mode` — `--mode template` creates template doc with `agent:exchange` components
- `test_cli_init_file_lazy_project_init` — creating a document in a fresh dir also creates `.agent-doc/`

489 tests pass (162 lib + 297 bin + 30 CLI).

Can we have tests for SKILL.md behavior?

### Re: SKILL.md Tests Added

6 new CLI tests for skill behavior:

- `test_cli_skill_install_help` — help text exits 0
- `test_cli_skill_check_help` — help text exits 0
- `test_cli_skill_install_creates_file` — creates `.claude/skills/agent-doc/SKILL.md` with `agent-doc-version:` frontmatter
- `test_cli_skill_check_after_install` — check passes after fresh install (version match)
- `test_cli_skill_install_idempotent` — two consecutive installs both succeed
- `test_cli_skill_install_reload_compact` — `--reload compact` outputs `SKILL_RELOAD=compact` or "already up to date"

Tests use CWD isolation (temp dir) since skill path resolution is CWD-relative.

495 tests pass (162 lib + 297 bin + 36 CLI).

Can we validate the behavior of the SKILL.md file?
Or is the best validation dog-fooding agent-doc?

### Re: SKILL.md Validation Strategy

**Both.** SKILL.md validation has two layers:

**1. Structural validation (testable):**
- SKILL.md contains required sections (Workflow steps 0-5, Document Format, Snapshot Storage)
- Frontmatter has `agent-doc-version:` matching the binary
- All referenced commands exist (`agent-doc boundary`, `agent-doc write --stream`, `agent-doc commit`, `agent-doc recover`)
- The `instruction-files` audit crate already does this — `agent-doc audit-docs` checks that SKILL.md references valid subcommands and file paths

**2. Behavioral validation (dog-fooding only):**
- Does the skill produce correct diff responses?
- Does it handle template mode vs append mode correctly?
- Does it properly escape code blocks in patch responses? (we found a bug with this earlier)
- Does it checkpoint at the right moments?
- Does it handle boundary markers correctly?

**This second layer can't be automated** — it requires LLM interpretation of the skill instructions. Dog-fooding IS the test suite for behavioral correctness. Every time you run `/agent-doc` on this document, you're running a behavioral test.

**What we CAN add to automated tests:**
- `test_skill_md_contains_required_steps` — parse SKILL.md, verify steps 0-5 are present
yes
- `test_skill_md_references_valid_commands` — extract all `agent-doc <cmd>` references, verify each is a valid subcommand
Do this.
- `test_skill_md_version_matches_binary` — already covered by `test_cli_skill_check_after_install`

**`agent-doc audit-docs`** already does most of this. The audit checks SKILL.md, CLAUDE.md, and SPEC.md for stale references. Running `make precommit` (which includes audit-docs) before release catches structural drift.

**Bottom line:** Dog-fooding is the primary validation for SKILL.md behavior. The audit + skill check tests catch structural issues. The gap is behavioral regression testing — which would require running Claude in CI (expensive, non-deterministic). For now, this document session IS the behavioral test suite.

Want me to release v0.24.0 with the install/init commands and new tests?
yes

### Re: SKILL.md Content Tests + v0.24.0 Released

**2 new SKILL.md content tests:**
- `test_skill_md_contains_required_steps` — verifies all 8 workflow step headings (0, 0b, 1, 1b, 2, 3, 4, 5)
- `test_skill_md_references_valid_commands` — extracts all `agent-doc <cmd>` references, validates against actual subcommand list from `--help`

**v0.24.0 released:**

| Target | Status |
|--------|--------|
| crates.io | `agent-doc v0.24.0` ✓ |
| GitHub Release | [v0.24.0](https://github.com/btakita/agent-doc/releases/tag/v0.24.0) ✓ |
| cargo install | v0.24.0 ✓ |

**v0.24.0 changelog (since v0.23.3):**
- `agent-doc install` — system-level setup (prereqs, editor plugin auto-detect)
- `agent-doc init` (no args) — project-level setup (.agent-doc/, SKILL.md, hooks)
- `agent-doc init <file>` — lazy project init when .agent-doc/ missing
- Cross-platform sync pane guard (`ps` instead of `/proc`)
- 15 new CLI tests (install, init, skill content validation)
- 497 tests (162 lib + 297 bin + 38 CLI)
`agent:pending` has not been updated recently.
How can we ensure that `agent:pending` is always actively updated?

### Re: Keeping agent:pending Updated

The SKILL.md already instructs the agent to manage pending items — the "Pending protocol" memory says: "When user approves work, add to `agent:pending` and flush BEFORE responding." But in practice, I've been adding items to the exchange instead of promoting them to pending.

**Options to enforce active pending updates:**

**1. SKILL.md enforcement (cheapest):**
Add a step to the SKILL.md workflow: "After each release or feature completion, update `agent:pending` — mark completed items `[x]` and add new items from the exchange discussion."

**2. Audit-docs check:**
Add a rule to `agent-doc audit-docs` that warns when `agent:pending` hasn't been modified in the last N commits but the exchange has new content. This catches drift.

**3. Compact-time reconciliation:**
When compacting the exchange, automatically scan for completed items (mentioned as "done", "released", "implemented" in the exchange) and mark them `[x]` in pending. Also scan for new items discussed but not added.

**My recommendation: option 1 + 3.** Add the SKILL.md instruction (free) and have compact auto-reconcile (catches everything). Option 2 adds noise for a problem that compact already solves.

**Immediate fix — let me update pending now:**

The completed items you removed were correct. Items that should be added from this session's discussion:
- `agent-doc install` (system setup) — done in v0.24.0
- `agent-doc init` (project setup, no-arg mode) — done in v0.24.0
- Orphaned window stash consolidation in `resync --fix` — discussed but not yet built
- Install script (`curl | sh`) — discussed but not yet built
- Homebrew formula — discussed but not yet built

Why are you escaping **all** backticks in the patch output?

### Re: Escaped Backticks Bug

You're right — I've been unnecessarily escaping backticks with `\`` in patch content piped through `cat <<'PATCH'`. Since the heredoc uses `'PATCH'` (single-quoted delimiter), shell expansion is already disabled. The `\`` is written literally to the file as `\``, which renders as escaped backticks in your editor.

**Fix:** Stop escaping backticks in patch output. Raw backticks work fine inside `<<'PATCH'` heredocs.

Let me fix the pending component now with raw backticks.
Why did this happen? SKILL.md or agent context issue?

### Re: Root Cause of Escaped Backticks

**Agent context issue, not SKILL.md.** The SKILL.md doesn't instruct me to escape backticks — it shows raw backticks in all examples. This is a model-level habit.

When I generate content that will be piped through `echo "..." | agent-doc write`, I instinctively escape backticks because in `echo "string"` (double-quoted), backticks would trigger command substitution. But the SKILL.md examples use `cat <<'PATCH'` heredocs where escaping is unnecessary.

**The confusion:** The SKILL.md step 4 shows `echo "<your response>"` with double quotes in its examples, but the actual implementation uses `cat <<'PATCH'` heredocs. My training conflates the two — I escape for the `echo` case even when using heredocs.

**Fix options:**
1. **Update SKILL.md** to explicitly note: "Do NOT escape backticks in heredoc patch content — `<<'PATCH'` disables shell expansion"
2. **Change SKILL.md examples** from `echo "..."` to `cat <<'PATCH'` to match actual usage
3. **Just remember** — this is now in auto-memory as a feedback correction

Option 2 is the right fix — align the SKILL.md examples with actual practice. But since SKILL.md is bundled in the binary (`include_str!`), it requires a release to update. For now, I'll just stop escaping.

Let's use Option 2 and release.

### Re: SKILL.md Heredoc Examples + v0.24.1 Released

Updated all 3 `echo` examples in SKILL.md to use `cat <<'RESPONSE'` heredocs:
- Step 3 (streaming checkpoints): `echo '<partial...'` → `cat <<'RESPONSE'`
- Step 4a (append mode): `echo "<your response>"` → `cat <<'RESPONSE'`
- Step 4b (template mode): `echo "<your patch response>"` → `cat <<'RESPONSE'`

**v0.24.1 released:**

| Target | Status |
|--------|--------|
| crates.io | `agent-doc v0.24.1` |
| GitHub Release | [v0.24.1](https://github.com/btakita/agent-doc/releases/tag/v0.24.1) |
| cargo install | v0.24.1 |
| SKILL.md | v0.24.1 |

497 tests pass. SKILL.md updated via `agent-doc skill install`.
Does `agent:boundary` have an ideal match with the component behavior?
Are there alternative names to consider?

### Re: agent:boundary Naming Analysis

**What it does:** Marks the insertion point in an append-mode component where the next response should be placed. It separates "what the user typed before submit" from "what the user types while waiting."

**Current name: `agent:boundary`**
- Pros: Clearly describes the function — it's a boundary between the agent's response territory and the user's new input
- Cons: Could be confused with component boundaries (`&lt;!-- agent:name --&gt;` / `&lt;!-- /agent:name --&gt;`)

**Alternative names considered:**

| Name | Meaning | Fit |
|------|---------|-----|
| `agent:boundary` | Dividing line between prompt and response | Current — good but overloaded with "component boundary" |
| `agent:cursor` | Where the agent will write next | Intuitive but conflicts with editor cursor concept |
| `agent:insertion-point` | Explicit about purpose | Verbose |
| `agent:anchor` | Anchor for response placement | Clean, distinct from other agent concepts |
| `agent:mark` | A marker for insertion | Too generic |
| `agent:reply-at` | Where the reply goes | Descriptive but awkward |
| `agent:pending` | Where pending response goes | Already used for the pending component |

**My assessment:** `agent:boundary` is a good name. It accurately describes the semantic — it's a boundary between the user's prompt and where the response will be inserted. The "component boundary" confusion is minimal because component markers use paired open/close tags while boundary markers are single unpaired markers.

The self-closing syntax discussion from earlier (`<!-- agent:boundary:UUID / -->`) would further distinguish them visually, but it's not necessary for correctness.

**Verdict:** Keep `agent:boundary`. It's established, accurate, and the alternative names don't offer enough improvement to justify a migration.

The JB plugin shows a green sidebar (new git content).
When I run the **Refresh** command (Ctrl+Alt+Y in this editor session), the commit is recognized with the patch header having a blue sidebar (modified git content).

Can we have the plugin refresh the document after the git commit via IPC?
So the agent-doc can send a refresh file command to the IPC mechanism?
Would this logic be implemented in the FFI plugin where each editor plugin receives an event?

### Re: VCS Refresh After Commit

**The mechanism already exists** — `agent-doc commit` writes `vcs-refresh.signal` to `.agent-doc/patches/`, and the JB plugin's `PatchWatcher` watches for it:

1. `git.rs:signal_vcs_refresh()` writes `.agent-doc/patches/vcs-refresh.signal`
2. `PatchWatcher.kt` polls every 500ms via `WatchService`
3. When signal detected: deletes file, calls `refreshVcs()` → `markEverythingDirty()`

**The bug:** The signal file exists right now and hasn't been consumed:
```
-rw-r--r-- 1 brian brian 7 Mar 21 08:24 vcs-refresh.signal
```

This means either:
1. **PatchWatcher isn't running** — it may have crashed or not started
2. **WatchService missed the event** — Java's inotify can miss events if the file is written and deleted before the next poll
3. **Path mismatch** — the watcher is watching a different `.agent-doc/patches/` directory

**Most likely cause:** The PatchWatcher IS running (since IPC patches work — we see "IPC patch consumed by plugin" in every write). But `signal_vcs_refresh()` creates the file, and the WatchService correctly fires a CREATE event. The PatchWatcher checks `filename == "vcs-refresh.signal"` and deletes + refreshes.

Let me check if the PatchWatcher's `refreshVcs()` is actually calling `markEverythingDirty()` on the right thread. IntelliJ requires VCS operations on the EDT (Event Dispatch Thread). If `refreshVcs()` runs on the watcher thread, the `markEverythingDirty()` call may silently fail.

**Fix:** Wrap `refreshVcs()` in `ApplicationManager.getApplication().invokeLater { ... }` to ensure it runs on EDT.

Want me to check the plugin source and fix it?

Yes

### Re: VCS Refresh Fix — Cross-Platform

**Root cause found:** `PatchWatcher` only watched for `ENTRY_CREATE` events. When the signal file already exists (from an unprocessed previous signal), `std::fs::write()` produces `ENTRY_MODIFY` — which was never caught.

**JetBrains fix (PatchWatcher.kt):**
1. Added `ENTRY_MODIFY` to `WatchService` registration — catches both create and overwrite
2. Added signal file processing in `processPendingPatches()` — handles signals that exist on plugin startup

**VS Code fix (extension.ts):**
1. Added `signalWatcher` — watches for `vcs-refresh.signal` (both create and change events)
2. Added `processVcsRefreshSignal()` — deletes signal file and runs `git.refresh` command
3. Added startup signal processing
4. Added cleanup in dispose

**Both plugins now:**
- Watch for `vcs-refresh.signal` create AND modify events
- Process existing signals on startup
- Delete the signal file after consuming
- Trigger VCS/git refresh so gutter annotations update immediately

JB plugin builds successfully. Restart IntelliJ to load the new plugin.

Do I need to reinstall the JB plugin to test?

### Re: Plugin Reinstall

Yes — the PatchWatcher changes are in compiled Kotlin bytecode, so you need to reinstall the plugin zip.

**Steps:**
1. IDEA Settings > Plugins > gear icon > "Install Plugin from Disk..."
2. Select: `file:///home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.8.zip`
3. Restart IDEA

Since this is a behavioral change (adding `ENTRY_MODIFY` to an existing watcher + signal processing in startup), reinstalling over the existing v0.2.8 should work without uninstall first.

After restart, the VCS gutter should update automatically when `agent-doc commit` runs — no more manual Ctrl+Alt+Y.

**To verify:** After reinstall, check that `.agent-doc/patches/vcs-refresh.signal` gets deleted after a commit.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- `agent-doc install` — system setup, prereq check, editor plugin auto-detect ([v0.24.0])
- `agent-doc init` (no-arg) — project setup, .agent-doc/, SKILL.md, hooks ([v0.24.0])
- Install script (`curl | sh`) for binary download
- Homebrew formula (`brew tap btakita/agent-doc`)
- Orphaned window stash consolidation in `resync --fix`
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
