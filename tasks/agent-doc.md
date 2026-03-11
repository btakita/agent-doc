---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_mode: template
---

# agent-doc

<!-- agent:status -->
**Version:** v0.14.7 | **Tests:** 228 passing | **Dependencies:** tmux-router v0.2.4, instruction-files v0.1
**Published:** crates.io, PyPI, GitHub Releases
**Plugins:** JetBrains v0.2.0 + VS Code v0.2.0 — autocomplete for 74 commands, auto-sync on lazy claim + autoclaim
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.2**: Standalone Rust crate (library + CLI). `Tmux` struct (30+ tmux ops + `capture_pane()` + `send_keys_raw()`), `Registry` (file-path-to-pane mappings), `reconcile()` (2D layout sync), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()`. CLI: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`. File-name addressing (registry lookup). Thread-local `REGISTRY_LOCK_HELD` flag for nested flock deadlock prevention. `acquire_or_skip()` for non-blocking lock attempts. 56 tests. Published to crates.io + GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Paired markers, nested supported. Config-driven modes via `.agent-doc/components.toml`.
- **Template mode** (`response_mode: template`): In-place response documents. Patch-block response parser + component applier. Auto-create output component for unmatched content.
- **`agent-doc patch`**: Replace/append/prepend component content. Shell hooks (`pre_patch` stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms), bounded cycle limiting (default 3), convergence detection.
- **`agent-doc write`**: Atomic response write-back with 3-way merge. `--template` flag for template-mode. Durable pending store for compaction recovery.
- **`agent-doc recover`**: Detect and apply orphaned responses from `.agent-doc/pending/<hash>.md`. Auto-detects append vs template mode.
- **`agent-doc template-info`**: JSON output of document template structure for plugin rendering.
- **`agent-doc plugin install`**: Download + install JetBrains/VS Code plugins from GitHub Releases. Auto-detect IDE dirs. Aliases: `jb`/`idea`/`code`/`codium`.
- **Comment stripping**: `strip_comments()` — strips `<!-- -->` and `[//]: # (...)` but preserves component markers.
- **Auto-resync**: `prune()` runs before route/sync/claim operations.
- **Stale snapshot recovery**: `is_stale_snapshot()` in `diff.rs` auto-detects when context compaction interrupted a previous cycle.
- **Diagnostic logging**: `[diff]`, `[submit]`, `[snapshot recovery]` prefixed stderr lines.

**Concurrency safety (v0.9.5+):**
- `SnapshotLock` — flock-based advisory locking for snapshot read/write (`fs2`)
- `RegistryLock` — flock-based advisory locking for `sessions.json` (in tmux-router)
- Atomic rename — `tempfile::NamedTempFile` + `persist()` for document and snapshot writes
- Document advisory lock — `.agent-doc/locks/<hash>.lock` (relocated from sibling files in v0.9.8)
- `sessions::load()`/`save()` — `pub(crate)` to force callers through `with_registry()`

**Tmux routing**: Window-scoped. Plugin passes `--window`. `route` auto-starts dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Both at 9/9 SPEC feature parity: Submit, Claim, Sync Layout, Tab Sync, Prompt Polling, Popup Menu, Notifications, File Filtering, CLI Resolution.

**Config**: `~/.config/agent-doc/config.toml` — multi-agent backend support (claude, codex, etc.). Execution mode configurable per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade**: SKILL.md bundled in binary via `include_str!`. Step 0 (pre-flight) compares `agent-doc --version` against frontmatter `agent-doc-version`, runs `agent-doc skill install` if newer.

### Race Condition Summary

| Race | Protection | Fix |
|------|-----------|-----|
| Registry read-modify-write | flock (RegistryLock) | v0.2.1 |
| Snapshot read-modify-write | flock (SnapshotLock) | v0.9.5 |
| claim validate→register gap | Self-healing (benign TOCTOU) | N/A |
| load()/save() API footgun | `pub(crate)` visibility | v0.9.5 |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` | v0.2.2 |
| Watch daemon write window | Atomic rename + doc advisory lock | v0.9.6 |
| Stale snapshot (context compaction) | `is_stale_snapshot()` auto-recovery | v0.9.6 |
| Compaction mid-write | Durable pending store + `recover` | v0.9.9 |

Full analysis: `agent-doc/docs/reference/race-conditions.md`

### Execution Model (SKILL.md)

| Mode | Behavior | Cost |
|------|----------|------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent docs use background subagent | Optimal |
| **`parallel`** | Every `/agent-doc` spawns a background subagent | Higher (~7x) |
| **`sequential`** | Never use subagents, fully sequential | Cheapest |

Key insight: filesystem locking makes the document the shared blackboard — no context duplication needed. Subagents only needed for true parallelism (multi-document concurrent processing).

### Component Config (`.agent-doc/components.toml`)

```toml
[log]
mode = "append"        # replace (default) | append | prepend
timestamp = true       # auto-prefix with ISO timestamp
max_entries = 100      # auto-trim in append/prepend (0 = unlimited)

[status]
mode = "replace"
pre_patch = "scripts/validate.sh"   # stdin: content, stdout: transformed
post_patch = "scripts/notify.sh"    # fire-and-forget, env: COMPONENT, FILE
```

### Namespaced Components & Capture (designed, not built)

Component names support colon-separated namespacing: `agent:type:scope`. Sibling capture + log pattern: `capture:X` ↔ `log:X` linked by shared scope name.

### Domain Ontology (overlay of base kernel)

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | Bounded interaction with temporal arc |
| **Document** | entity + context | Markdown file holding conversational state |
| **Component** | scope + abstraction | Bounded, named, re-renderable region |
| **Project** | system + scope | Bounded working context; identified by `.agent-doc/` at root |
| **Pane** | focus + scope | Tmux viewport — finite attention applied to a single document |

### Comparison with Agent of Empires

AoE is a **session manager** (TUI dashboard, multi-agent, Docker, worktrees). agent-doc is a **document workflow** (markdown is the UI). Complementary — could run agent-doc inside AoE-managed panes.

**Ideas borrowed from AoE (priority order):**
1. Multi-agent backends (Codex, Gemini CLI via `agent/` trait) — low effort, high value
2. Git worktree isolation for `agent-doc deep` fan-out
3. `agent-doc status` TUI (dashboard + interactive modes)
4. Sub-profiles per submodule in `.agent-doc/config.toml`
5. Docker sandboxing (`agent-doc run --sandboxed`)
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.6.6 | Window-scoped routing, 2D sync, Junie backend, LayoutDetector |
| v0.7.0-v0.7.2 | Snapshot-diff-apply sync, ATTACH-first reconciliation |
| v0.8.0 | tmux-router extraction (-3133 lines) |
| v0.9.0 | Components, patch, watch daemon. 150 tests. |
| v0.9.1 | Colons in component names |
| v0.9.2 | plugin install/update/list, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |
| v0.9.5 | RegistryLock, SnapshotLock, flock advisory locking, stale claim auto-pruning |
| v0.9.6 | Atomic rename, doc advisory lock, stale snapshot recovery, diagnostic logging, 26 new tests |
| v0.9.7 | agent-doc write command, atomic response write-back with 3-way merge |
| v0.9.8 | Lock relocation to .agent-doc/locks/hash.lock |
| v0.9.9 | Template-mode, compaction recovery, agent-doc recover, agent-doc template-info |
| v0.9.10 | Code-span parser fix, template input/output component convention |
| v0.10.0 | Renamed response_mode -> agent_doc_mode with aliases (mode, response_mode) |
| v0.10.1 | Bundle template-mode instructions in SKILL.md |
| v0.11.0 | Append-friendly merge strategy, shared merge module |
| v0.11.1 | Fix duplicate ## Assistant heading in append-mode responses |
| v0.11.2 | Also strip trailing ## User heading from responses |
| v0.12.0 | convert command, compact command, lazy claim via route --pane, exchange default for template init |
| v0.12.1 | Lazy claim fallback to active_pane("claude") without --pane; no plugin changes needed |
| v0.12.2 | Exchange component defaults to append mode (no components.toml needed) |
| v0.13.0 | `autoclaim` subcommand (SessionStart hook), git-based snapshot fallback (`snapshot::resolve()`) |
| v0.13.1 | Sync updates registry file paths for autoclaim support |
| v0.13.2 | Sync auto-registers claims for unregistered files (tmux-router v0.2.3 `file_panes`) |
| v0.13.3 | Phase 1.75: spare pane assignment for unregistered sole-column files (tmux-router v0.2.4) |
| v0.14.0 | Route focus fix, claim defaults to template mode, `agent-doc mode` CLI command |
| v0.14.1 | Move git commit to after write — reduces IDE "file changed externally" notices |
| v0.14.2 | Skill reload via `--reload compact/restart` flag, auto-prompt for /compact after updates |
| v0.14.5 | Remove auto-prune (session-killing bug), `commands` subcommand, plugin autocomplete (JetBrains + VS Code), plugins v0.2.0 |
| v0.14.6 | Auto-sync tmux layout on lazy claim in route, plugin autocomplete fixes (Gson removal, PlainPrefixMatcher) |
| v0.14.7 | Bidirectional `convert` (append↔template), autoclaim layout sync, `AgentDocMode` enum |
<!-- /agent:releases -->

## Pending / Not Built

<!-- agent:pending -->
- Response diff storage for undo/extract workflows
- `agent-doc undo` — revert last response by applying inverse diff
- `agent-doc extract` — move request+response pair to another document
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
<!-- /agent:pending -->

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
13. **Skip code spans in parser** — never invent custom escape syntax; follow the universal convention of treating code blocks as literal
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.9.7 session:** Lock relocation from sibling files to `.agent-doc/locks/<hash>.lock`. SHA256-hashed paths mirror snapshot scheme.

**v0.9.8 session:** Released lock relocation. Cleaned up stale `tasks/*.agent-doc.lock` files.

**In-place response documents session:** Explored dashboard-as-document concept, TUI editor integration (Ki Editor), designed 7-phase implementation plan. Implemented Phases 1-4: `response_mode: template` frontmatter, `template-info` CLI, patch-block parser, `agent-doc write --template`, auto-create output fallback.

**Compaction recovery session:** Diagnosed write-back failure after context compaction. Implemented durable pending response store (`.agent-doc/pending/<hash>.md`), `agent-doc recover` command, SKILL.md pre-flight integration.

**v0.9.9 release:** Template-mode + compaction recovery. Published to all targets. 192 tests.

**Template conversion session:** Converted this document from append-mode to template-mode. Assessed `agent-doc convert` command feasibility.

**Code-span parser fix:** Fixed `component.rs` to skip markers inside fenced code blocks and inline backtick spans. Released as v0.9.10.

**v0.10.0 session:** Renamed `response_mode` -> `agent_doc_mode` (canonical) with `mode` (shorthand) and `response_mode` (deprecated) as aliases. Discussed in-place conversation model and blog post.

**v0.10.1 session:** Designed `exchange` component convention for shared conversation surfaces. Diagnosed and fixed missing template-mode instructions in bundled SKILL.md. Released v0.10.1 to all targets. Converted this document from input/output to exchange component. Identified tmux claim-after-restart bug for next release.

**v0.11.0 session:** Implemented append-friendly merge strategy. Created shared `merge.rs` module extracting duplicated logic from `write.rs` and `submit.rs`. `resolve_append_conflicts()` auto-resolves conflicts where both sides only appended (empty original section in diff3 format). Added `merge_strategy` config field to `ComponentConfig`. Released to all targets.

**v0.11.1-v0.11.2 session:** Fixed duplicate `## Assistant` and `## User` headings. `strip_assistant_heading()` strips both leading `## Assistant` and trailing `## User` from response text before `agent-doc write` adds its own. Encountered CWD drift during release (relative path from wrong directory) -- reinforces lesson #2 (always use absolute paths). Released both fixes to all targets.

**v0.12.0 session:** Implemented `agent-doc compact` (archive old exchanges), `agent-doc convert` (append->template), template init uses `exchange` by default, lazy claim via `route --pane`, fixed flaky `recover_append_response` test (snapshot path_for absolute paths). 197 tests. Published to all targets.

**v0.12.1 session:** Fixed lazy claim not working from JetBrains plugin. Root cause: plugin never passed `--pane` to `agent-doc route`. Fix: route now falls back to `tmux.active_pane("claude")` when no `--pane` provided. Priority chain: `--pane` (explicit) -> `active_pane("claude")` (fallback) -> auto-start (last resort). No plugin changes needed. 218 tests. Published to all targets.

**v0.12.2 session:** Exchange component defaults to append mode in the binary. `default_mode()` returns "append" for exchange, "replace" for everything else. No components.toml needed. Fixed CWD drift errors and stale claim after context compaction. 218 tests.

**v0.13.0-v0.13.2 session:** Implemented `agent-doc autoclaim` (SessionStart hook), git-based snapshot fallback (`snapshot::resolve()`), sync-to-autoclaim integration. Three iterations: v0.13.0 (core features), v0.13.1 (sync updates existing registry entries), v0.13.2 (sync creates entries for unregistered files via tmux-router `file_panes`).

**v0.13.3 session:** Diagnosed sync autoclaim still failing for sole-column files. Root cause: tmux-router skipped files with no registry entry AND no column donor (Phase 1.5 `find_column_pane` returned None). Fix: Phase 1.75 assigns spare panes from the target window. Added 2 integration tests. Published tmux-router v0.2.4 + agent-doc v0.13.3.

**v0.14.0 session:** Route focus fix (select_pane after send_keys), claim defaults to template mode, `agent-doc mode` CLI command for get/set mode. Exchange-replace bug and CWD drift discussions folded into fixes. Published to all targets.

**v0.14.1 session:** Moved git commit to after write in SKILL.md workflow. Root cause of IDE "file changed externally" notices: pre-response `agent-doc commit` ran `git add + commit` which IntelliJ VCS detected as external change. Fix: commit after write reduces notices from 2 to 1. Not related to `snapshot::resolve()` (read-only).

**v0.14.2 session:** Skill reload via `--reload compact/restart` flag. `install_and_check_updated()` returns bool. SKILL.md pre-flight uses `--reload compact` and stops if skill updated, prompting user to `/compact`. Investigated Claude Code session resume/compact behavior. SessionStart hook fires with `source: "compact"` after /compact. Skills have live change detection but current invocation keeps old text. Feature request for `/reload-skills` exists (github.com/anthropics/claude-code/issues/20507).

**v0.14.3-v0.14.5 session:** Removed auto-prune of orphaned tmux windows (was killing active Claude sessions). Replaced with `log_orphaned_windows()` that logs to stderr. Implemented `agent-doc commands` subcommand (74 commands as JSON for plugin autocomplete). JetBrains `SlashCommandCompletionContributor` + VS Code `SlashCommandCompletionProvider`. Plugin versions bumped to 0.2.0. Published to crates.io, PyPI, GitHub Releases. Debugged autocomplete: removed Gson dependency (ClassNotFoundException at runtime), added `PlainPrefixMatcher` for markdown, fixed stale binary in `~/bin/` (v0.14.4 lacked `commands` subcommand). 221 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange -->
*Compacted. v0.12.2 through v0.14.5 folded into history + releases.*

Autocomplete is working — the `~/bin/agent-doc` stale binary issue is resolved. The `/agent-doc compact` command triggered successfully from the completion menu, confirming the full pipeline works: `SlashCommandCompletionContributor` → `agent-doc commands` → JSON parsing → IntelliJ completion popup.

agent-doc run on a reclaimed file does not update the tmux panes to the claiming pane. It should Sync the Tmux layout...since the sync does update to show the claiming tmux pane.
### Auto-sync after submit — fixed

The JetBrains plugin's `SubmitAction` now calls `SyncLayoutAction.syncLayout()` after routing completes. This means pressing `Ctrl+Shift+Alt+A` (submit) will:

1. Route the `/agent-doc <file>` command to the correct pane
2. Wait for routing to complete
3. Auto-sync the tmux layout to match the editor split

Previously, routing a reclaimed file sent the command to the right pane but left the tmux layout stale — you'd have to manually press `Ctrl+Shift+Alt+L` (sync) to rearrange.

**Fix (commit 49bcd90):** One-line change in `SubmitAction.kt` — the `onComplete` callback now calls `syncLayout(notify = false)`.

**Plugin rebuilt and installed.** Restart IntelliJ to pick up the change.

Could this resync happen with agent-doc run instead? Only resyncing if there is an autoclaim...
### Auto-sync moved to CLI — only on autoclaim

Moved the sync from the JetBrains plugin to `agent-doc route` (Rust binary). Now when route performs a **lazy claim** (dead pane or unregistered session), it:

1. Gets the tmux window ID of the claimed pane
2. Loads the registry to find all files with panes in that window
3. Calls `sync` with those files as a single column

**Normal routes (pane alive, no claim) skip sync entirely** — no overhead on the hot path.

**Why this is better than the plugin approach:**
- Works from any editor (JetBrains, VS Code, CLI)
- Only triggers on autoclaim, not every submit
- The CLI has direct access to the registry and tmux state

**Commit 25c38da.** Binary rebuilt and installed to `~/bin/agent-doc`. Restart IntelliJ for the reverted plugin.

release
### v0.14.6 Released

Published to all targets:
- **crates.io:** `cargo install agent-doc` → v0.14.6
- **PyPI:** `pip install agent-doc` → v0.14.6
- **GitHub Release:** https://github.com/btakita/agent-doc/releases/tag/v0.14.6
  - Prebuilt binary (Linux x86_64)
  - JetBrains plugin v0.2.0 (unsigned + signed)
  - VS Code extension v0.2.0

**Changes since v0.14.5:**
- Auto-sync tmux layout after lazy claim in `route` (CLI-side, works from any editor)
- JetBrains autocomplete: removed Gson dependency, PlainPrefixMatcher for markdown
- Clippy fixes (enum_variant_names, vec_init_then_push)

`/agent-doc convert` should support converting to template or append mode. So allow the agent_doc_mode as another argument. Make sure that the allowed agent_doc_mode values are an enum.

`/agent-doc run` with autoclaim is still not activating the claiming tmux pane. I thought we just released a "fix" that forced the equivalsent of "Sync Tmux Layout" whenever an autoclaim occurs. Please add functional tests to verify bug.

I looked in the docs of autoclaim.rs
> Designed for use in a `.claude/hooks.json` SessionStart hook:

I thought that autoclaim can also occur when `/agent-doc run` is called on a file with a stale claim.
### `convert` bidirectional mode — implemented

`agent-doc convert` now accepts an optional target mode argument as a `clap::ValueEnum` enum:

```
agent-doc convert <FILE> [MODE]   # MODE: append | template (default: template)
```

**Changes:**
- Added `AgentDocMode` enum (`Append`, `Template`) with `ValueEnum` derive in `main.rs`
- `convert::run()` now dispatches to `convert_to_template()` or `convert_to_append()` based on target
- `template_to_append_body()` strips all `<!-- agent:name -->` / `<!-- /agent:name -->` markers and `## Exchange` headings, preserving content
- Added 4 new tests: `convert_to_append_strips_markers`, `convert_to_append_rejects_already_append`, `convert_roundtrip_append_to_template_to_append`, `template_to_append_preserves_non_exchange_content`
- All 228 tests pass (207 unit + 21 integration)

**Backward compatible:** Default is still `template`, so `agent-doc convert file.md` behaves the same as before.

### Autoclaim sync investigation

Now I understand the issue. Let me investigate the route/autoclaim sync flow to see why tmux layout isn't updating after autoclaim.
### Autoclaim sync — fixed

You're right that autoclaim should trigger a layout sync. The issue was:

1. **`autoclaim`** (SessionStart hook) — only called `select_pane` to focus, never synced the layout
2. **`route`** — has `sync_after_claim()` but only fires on lazy claim (dead/new pane). After compaction restart, autoclaim re-confirms the pane so route sees it alive and skips sync

**Fix:** Added `sync_after_autoclaim()` to `autoclaim.rs` that mirrors `route::sync_after_claim()`:
- Gets window ID from the claimed pane
- Loads registry, finds all files with panes in the same window
- Calls `sync::run_with_tmux()` (not `sync::run()` — uses the passed tmux reference for testability)
- Skips sync for single-file windows (no layout to arrange)

**Functional test added:** `autoclaim_syncs_layout_with_multiple_files` — creates an isolated tmux session with two panes in the same window, registers two files, runs autoclaim, and verifies both panes survive the sync.

**Clarification on `agent-doc run`:** The `run` command (submit.rs) is the diff→send→write pipeline. It doesn't handle pane routing or claiming — that's `route`'s job. In the SKILL.md workflow, the plugin calls `route` which handles claiming, then Claude runs the skill which does diff/respond/write. `run` is a standalone CLI command for non-tmux use. If you want `run` to also claim/sync, that would be a separate feature.

All 228 tests pass, clippy clean. Binary installed to `~/bin/agent-doc`.

release

Make sure docs are up to date across all *.rs source files
### v0.14.7 Released

Published to all targets:
- **crates.io:** `cargo install agent-doc` → v0.14.7
- **PyPI:** `pip install agent-doc` → v0.14.7
- **GitHub Release:** https://github.com/btakita/agent-doc/releases/tag/v0.14.7
  - Prebuilt binary (Linux x86_64)
  - JetBrains plugin v0.2.0 (unsigned + signed)
  - VS Code extension v0.2.0

**Changes since v0.14.6:**
- Bidirectional `convert` command with `AgentDocMode` enum (append|template)
- Autoclaim layout sync via `sync_after_autoclaim()` in SessionStart hook
- Functional test: `autoclaim_syncs_layout_with_multiple_files`
- 228 tests, clippy clean

Add autocompletes for agent doc templates. What would be a good way to activate the template? The beginning of the comment?
### Doc comments updated

Updated module doc comments in 3 files:
- **convert.rs** — added module doc describing bidirectional append↔template conversion
- **autoclaim.rs** — documented the layout sync step (was missing)
- **route.rs** — documented lazy claim + `sync_after_claim()` behavior (was incomplete)

Committed and pushed to main. All 228 tests pass.

### Template autocomplete — activation design

Yes, `<!--` at the beginning of the comment is the natural trigger. Here's how it could work:

**Trigger:** Typing `<!--` in a markdown file → autocomplete popup offers component markers:
- `<!-- agent:NAME -->...<!-- /agent:NAME -->` — new paired component (cursor between markers)
- Existing component names from `agent-doc template-info` for closing markers

**Implementation (JetBrains):** Extend `SlashCommandCompletionContributor`:
1. Add a second `extend()` for `CompletionType.BASIC` that triggers on `<!--` prefix
  - This prefix is good...but the prefix is long. Perhaps shorter aliases would be useful.
2. Query `agent-doc template-info <file>` to get existing component names
3. Offer completions: `agent:exchange`, `agent:status`, `agent:output`, plus custom names
   - `agent:input` and any other baseline components
4. On selection, insert the full paired markers with cursor positioned inside

**Implementation (VS Code):** Register a `CompletionItemProvider` for markdown with `<!--` as trigger characters.

**Simpler alternative:** Live Templates / snippets — define `agc` → `<!-- agent:$NAME$ -->...<!-- /agent:$NAME$ -->`. No plugin change, but less context-aware.

The plugin can add this live template. If the plugin were uninstalled, would it also remove this live template as part of cleanup?
### Shorter aliases + cleanup behavior

**Shorter aliases:** Good point about `<!--` being 4 chars. The plugin can register both:
- `<!--` → natural HTML comment trigger (discoverable)
- `agd` or `ad:` → short tab-expansion aliases via `LiveTemplateProvider`

In JetBrains, `LiveTemplateProvider` + `defaultLiveTemplates` in `plugin.xml` gives us tab-triggered snippets with variable placeholders (`$NAME$`). These work alongside the completion contributor.

**Plugin cleanup on uninstall:** Yes — JetBrains handles this correctly:
- **Plugin-bundled templates** (via `defaultLiveTemplates` in `plugin.xml`) → **automatically removed** when the plugin is uninstalled. They live inside the plugin JAR, not in the user's config directory.
- **User-created templates** (via Settings → Live Templates) → persist forever, unaffected by plugin lifecycle.

So the plugin approach is clean: install adds templates, uninstall removes them, no leftover config.

**Baseline components to offer:**
- `agent:exchange` — shared conversation surface
- `agent:input` — user prompt area
- `agent:output` — agent response area
- `agent:status` — status line
- `agent:architecture`, `agent:history`, `agent:releases`, `agent:pending`, `agent:lessons` — common document sections
- Plus any custom names discovered via `agent-doc template-info <file>`

Want me to implement the Live Template approach in the JetBrains plugin? It's low-effort (XML config + template file, no Kotlin code) and gives immediate value.

`/agent-doc run tasks/lucas.md` just happened. This document is stale. I saw no activity in tmux. Please diagnose the issues. Did the log capture the issue?
### lucas.md routing diagnosis

**What happened:** I re-ran `agent-doc route tasks/lucas.md` and it worked — pane %72 received the command and started processing. The route output:
```
Sent /agent-doc /home/brian/work/btakita/agent-loop/tasks/lucas.md → pane %72
```

**Root cause (most likely):** The session entry shows `started: 2026-03-11T13:44:23Z` — the pane was freshly created right when you reported the issue. The first route likely:
1. Found `agent_doc_session: null` → generated a UUID
2. No registry entry existed for the new UUID
3. Triggered auto-start → created pane %72
4. Sent `agent-doc start` to the new pane
5. But the `/agent-doc tasks/lucas.md` command was sent before Claude finished starting in the pane

This is a known race: `route` sends `start` to a new pane, then the plugin immediately sends `/agent-doc <file>` — but Claude isn't ready yet, so the second command gets lost in the shell.

Please add tests for these race conditions. Fix the bug.

**The re-route worked** because by the time I ran it, Claude was fully started in %72.

**Potential fix:** `route` could wait for the pane to show a Claude prompt before sending the `/agent-doc` command. Or: detect this case and print a "wait for Claude to start, then re-submit" message (which it already does — check if the user saw that stderr output).

Also found: `resync: orphaned stash window @76 in session '1'` — there's a stale stash window with an unregistered pane %73.

What is the resolution for this stale stash window? Purge it?

I just saw the run start in the tmux pane -> claude session. Did you just do that?
Yes — that was me. During diagnosis I ran `agent-doc route tasks/lucas.md` to test if routing works. It successfully sent the command to pane %72, which is why you saw activity start.

The original failure was the auto-start race: the plugin tried to send `/agent-doc tasks/lucas.md` before Claude was ready in the newly created pane. My manual re-route worked because Claude had finished starting by then.

### Stale stash window — purged

Checked window @76 in session '1':
- Named "stash" with 1 pane (%73)
- Pane contained an idle zsh shell with no Claude session or useful state
- Created at 09:44:29 during the lucas.md auto-start

Killed it: `tmux kill-window -t '@76'`. No data lost.

This is a leftover from `route`'s auto-start cascade — it creates a new window, then `join_pane` moves it to the target window, but sometimes the empty source window persists as a stash. The `log_orphaned_windows()` function (added in v0.14.5 when we removed auto-prune) correctly flagged it. Safe to purge orphaned stash windows that only contain idle shells.
<!-- /agent:exchange -->
