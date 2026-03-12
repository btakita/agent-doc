---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '23'
agent_doc_mode: stream
---

# agent-doc

<!-- agent:status -->
**Version:** v0.15.0 | **Tests:** 293 passing | **Dependencies:** tmux-router v0.2.4, instruction-files v0.1
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
| v0.14.8 | Fix `strip_comments` code-range awareness, safe stash window purge |
| v0.14.9 | Multi-backtick code span support in `find_code_ranges`, 235 tests |
| v0.15.0 | CRDT stream mode, chain-of-thought streaming, watch daemon stream support, cumulative flush fix. 293 tests. |
| v0.15.1 | Claim defaults to stream mode, route.rs refactor (extract methods + `[route]` logging) |
<!-- /agent:releases -->

## Pending / Not Built

<!-- agent:pending -->
- Response diff storage for undo/extract workflows
- `agent-doc undo` — revert last response via CRDT UndoManager (or inverse diff as fallback)
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
- `agent-doc stream` — bridge terminal STDOUT/STDERR to document components in real-time
- CRDT-based merge for all components (`yrs` library) — eliminate merge conflicts, uniform model, CRDT operation log as canonical history
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
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed twice: component.rs (v0.9.10) and diff.rs (v0.14.8). Check all `<!--` parsers when adding new ones.
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

**v0.14.6-v0.14.9 session:** Auto-sync tmux layout on lazy claim + autoclaim. Bidirectional `convert` (append↔template). Fixed `strip_comments()` code-range awareness: single-backtick (v0.14.8), then multi-backtick CommonMark §6.1 (v0.14.9). Safe `purge_stash_windows()`. 235 tests.

**v0.15.0 session:** CRDT stream mode implementation (Phases 1-4). Phase 1: `crdt.rs` with `yrs` library — `CrdtDoc`, `merge()`, `compact()`. Phase 2: CRDT merge path in `merge.rs`, CRDT state persistence in `snapshot.rs`. Phase 3: `StreamingAgent` trait + `stream-json` parser. Phase 4: `agent-doc stream` command — timer-based flush loop, template patch application, CRDT state save. Chain-of-thought streaming (`thinking` + `thinking_target` in StreamConfig). Watch daemon stream support (tmux capture-pane polling). Stream flush uses replace mode for cumulative buffers. 293 tests. Designed Option A (skill-level checkpoints) and Option B (watch daemon capture) for tool-use + streaming. Implemented both. Converted this document to stream mode.

**v0.15.1 session:** Claim defaults to stream mode instead of template. Refactored `route.rs`: extracted `send_command()` and `find_target_pane()` to eliminate duplicated lazy-claim logic, added `[route]` prefix logging. Identified CRDT merge bug (frontmatter character doubling). Code review guide for recurring routing bug (auto-start race condition). Published to all targets.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange -->
*Compacted. v0.12.2 through v0.15.1 folded into history + releases.*

We are still having issues with a stale claim. See tasks/gmail.md.
Please diagnose with logs. Let's conduct another manual review.

`/agent-doc compact` got duplicated in the CRDT response.
### Re: Compacted exchange

Removed 1,478 lines of old conversation. Exchange now starts clean. History component updated with v0.15.0 and v0.15.1 session summaries.

**CRDT append mode issue:** The compact didn't work via patch block because exchange has `mode = "append"` in `components.toml`. Patch blocks respect component mode, so the compacted content was appended instead of replacing. Had to do manual deletion. For future: `agent-doc compact` should support stream/template mode documents by reading the exchange component and summarizing it.

Ok...Why are we doing append mode? I thought we default to template mode.

### Re: `/agent-doc compact` duplicated in CRDT response

Known issue. The CRDT merge in stream mode is producing duplicates — same root cause as the frontmatter character doubling bug from earlier. The `crdt::merge()` function's `compute_edit_ops()` is generating duplicate insert operations. This needs a focused debugging session on `crdt.rs`.

Should we add detailed logging? Like log each function call? From an analysis of crdt.rs, what could the causes of the duplicates be?

Should we add detailed logging? Like log each function call? From an analysis of crdt.rs, what could the causes of the duplicates be?

### Re: Stale claim — gmail.md diagnosis

Checked the registry and tmux state:

| File | Registry Pane | Pane Alive | Pane Process | Session |
|------|--------------|------------|-------------|---------|
| tasks/gmail.md | %76 | Yes | agent-doc | 23 |
| tasks/agent-doc.md | %75 | Yes | agent-doc | 23 |

**gmail.md frontmatter says `tmux_session: '0'`** but the pane is in session `'23'`. This is a stale frontmatter value — the pane was reclaimed to session 23 but `tmux_session` wasn't updated.

However, `tmux_session` is informational — `route` uses the UUID (`agent_doc_session`) to look up the pane in the registry, not `tmux_session`. So the stale `tmux_session` shouldn't cause routing failures.

**What specific failure are you seeing with gmail.md?** Is `/agent-doc tasks/gmail.md` not reaching pane %76? Or is the pane not responding? The `[route]` logging we just added in v0.15.1 should help trace the exact failure point — can you run it and share the stderr output?

Two issues:
- The tmux panes did not refresh when run was called.
- When I switched the editor back to this file, the tmux panes refreshed. But the right side tmux pane ended up with a Claude session that did nothing. The run command was not called in the Claude session.

Here is the copy of the tmux pane

```
 ▐▛███▜▌   Claude Code v2.1.72
▝▜█████▛▘  Opus 4.6 · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop
  ⎿  SessionStart:startup says: [agent-loop] recent context, 2026-03-12 12:26am EDT
     ────────────────────────────────────────────────────────────

     Legend: session-request | 🔴 bugfix | 🟣 feature | 🔄 refactor | ✅ change | 🔵 discovery | ⚖️ decision

     Column Key
       Read: Tokens to read this observation (cost to learn it now)
       Work: Tokens spent on work that produced this record ( research, building, deciding)

     Context Index: This semantic index (titles, types, files, tokens) is usually sufficient to understand past work.

     When you need implementation details, rationale, or debugging context:
       - Fetch by ID: get_observations([IDs]) for observations visible in this index
       - Search history: Use the mem-search skill for past decisions, bugs, and deeper research
       - Trust this index over re-reading code for past decisions and learnings

     Context Economics
       Loading: 50 observations (17,179 tokens to read)
       Work investment: 174,488 tokens spent on research, building, and decisions
       Your savings: 90% reduction from reuse

     Mar 11, 2026

     src/agent-doc/src/write.rs
       #4073  3:56 PM  🔴  Fixed snapshot bug in write::run() for append mode
       #4074  3:58 PM  🔴  Fixed snapshot bug in all write.rs functions (6 code paths)

     src/agent-doc/src/submit.rs
       #4075           🔴  Fixed snapshot bug in submit::run() for agent submit workflow

     src/agent-doc/src/stream.rs
       #4076  3:59 PM  🔴  Fixed snapshot bug in stream::stream_loop() for real-time streaming

     src/agent-doc/src/write.rs
       #4078  4:05 PM  ✅  Added regression test for snapshot behavior during concurrent user edits

     src/agent-doc/src/main.rs
       #4079  4:06 PM  ✅  Streaming interval reduced from 2s to 200ms

     src/agent-doc/src/snapshot.rs
       #4080           🔴  Fixed build error by making find_project_root public

     src/agent-doc/SKILL.md
       #4081           ⚖️  Removed auto-commit to preserve git gutter visibility
       #4082           ✅  Switched all write modes to CRDT merge strategy

     src/agent-doc/src/write.rs
       #4083           🔴  Fixed snapshot to exclude concurrent user edits

     src/agent-doc/SKILL.md
       #4084  4:09 PM  ⚖️  Reverted to auto-commit after response to manage git gutter size

     tasks/stream-test.md
       #4085  4:13 PM  ✅  Committed stream-test documentation

     General
       #4086  4:23 PM  🔵  Agent-doc recover command tested on stream-test.md

     tasks/stream-test.md
       #4087           ✅  Committed agent-doc response to tasks/stream-test.md
       #4088           🔵  Agent-doc stream architecture and workflow design explored
       #4090  4:27 PM  ✅  agent-doc submodule updated with CRDT and streaming improvements
       #4091           ✅  Committed stream-test documentation using deferred workflow
       #4096  4:33 PM  ✅  Agent documentation response committed for stream-test task
       #4100  4:36 PM  🔵  agent-doc recover command handles orphaned responses
       #4104  4:39 PM  ✅  CRDT merge preserves concurrent edits in agent-doc workflow
       #4111  4:41 PM  ✅  Committed agent-doc response for stream-test task
       #4113  4:43 PM  ✅  Committed stream-test.md documentation to version control

     demo/
       #4114  4:44 PM  ✅  Created demo directory for agent-doc demonstrations

     tasks/stream-test.md
       #4116  4:49 PM  ✅  Committed agent-doc response for stream-test task
       #4117  4:51 PM  ⚖️  Stream mode as document nexus architecture
       #4118  4:57 PM  ✅  Blog post and investor pitch documentation for agent-doc CRDT collaboration
       #4119  4:59 PM  🟣  Tested agent-doc streaming write with CRDT merge

     tasks/linkedin.md
       #4120  7:27 PM  🔵  Agent-doc recover found no pending responses for linkedin.md
       #4121           ✅  Agent-doc committed 5,598 lines to linkedin.md task
       #4122  7:28 PM  🔵  LinkedIn task file contains new bookmarklet request with 1.8MB HTML structure

     #S1202 Clean up tasks/linkedin.md file and git history (Mar 11, 7:37 PM)

     tasks/linkedin.md
       #4123  7:38 PM  ✅  LinkedIn task documentation drastically simplified
       #4124           🟣  LinkedIn bookmarklet now extracts contact names for personalized exports

     .git/HEAD
       #4125  7:39 PM  ✅  Git history cleaned by squashing LinkedIn commits to remove HTML bloat

     .git/objects/
       #4126           ✅  Git repository garbage collected to remove orphaned HTML blob

     #S1203 Background task completion check for git cleanup response writing (Mar 11, 7:43 PM)

     tasks/linkedin.md
       #4127  7:48 PM  ✅  Committed agent documentation updates to tasks/linkedin.md
       #4128           🔵  LinkedIn task documentation and corky social integration workflow
       #4129  7:51 PM  ✅  Updated LinkedIn bookmarklet with date prefix and improved name extraction

     #S1204 Clean up tasks/linkedin.md documentation file and fix corrupted bookmarklet code (Mar 11, 8:48 PM)

     #S1205 Monitor background task completion for tasks/linkedin.md cleanup (Mar 11, 9:29 PM)

     #S1206 Fixed LinkedIn message extraction bookmarklet to properly extract names and timestamps (Mar 11, 9:29 PM)

     #S1209 Agent-doc stream mode configuration and routing bug investigation (Mar 11, 9:33 PM)

     tasks/linkedin.md
       #4130  9:33 PM  ✅  Committed LinkedIn bookmarklet updates to version control
       #4131           🔴  Fixed LinkedIn bookmarklet name extraction using newline splitting

     tasks/agent-doc.md
       #4135  10:10 PM  🔵  agent-doc project documentation reviewed

     src/agent-doc/src/crdt.rs
       #4136            🟣  CRDT-based stream mode implemented across 4 phases

     src/agent-doc/src/watch.rs
       #4137  10:11 PM  🟣  Watch daemon enhanced with stream mode polling (Option B)

     src/agent-doc/src/frontmatter.rs
       #4138            🟣  Chain-of-thought streaming with thinking blocks

     src/agent-doc/src/watch.rs
       #4139            🔴  Watch daemon CWD and stream flush duplication bugs fixed

     tasks/agent-doc.md
       #4140            ✅  agent-doc v0.15.0 released with CRDT stream mode

     #S1210 Refactored route.rs and debugged auto-start command routing bug (Mar 11, 10:26 PM)

     src/agent-doc/src/route.rs
       #4141  10:28 PM  🔄  Refactored route.rs to eliminate duplicate code and improve logging

     #S1211 Fixed CRDT-mangled LinkedIn bookmarklet for Mia Tomeo profile extraction (Mar 11, 11:38 PM)

     src/agent-doc/Cargo.toml
       #4142  11:41 PM  ✅  Released agent-doc v0.15.1 with claim mode change and route refactoring
       #4143  11:43 PM  🟣  Deployed agent-doc v0.15.1 to crates.io and PyPI

     Mar 12, 2026

     #S1212 Released and integrated agent-doc v0.15.1 with stream mode defaults and route refactoring (Mar 12, 12:10
     AM)

     tasks/linkedin/mia-tomeo.md
       #4144  12:13 AM  ⚖️  LinkedIn Recovery Strategy for Mia Tomeo Financial Advisor Conversation

     src/agent-doc
       #4145  12:24 AM  ✅  Updated agent-doc submodule to v0.15.1

     #S1213 Completed agent-doc v0.15.1 multi-platform release with all background tasks finished (Mar 12, 12:25 AM)

     Investigated: Background task completion status for binary installation, PyPI publishing, and release binary
     builds

     Learned: agent-doc release workflow includes parallel background tasks for binary installation, PyPI publishing,
     and GitHub release binary builds that complete asynchronously

     Completed: All v0.15.1 release tasks completed: binary installed to ~/bin/agent-doc, PyPI package published,
     GitHub release binaries built, crates.io published, and agent-loop submodule updated to v0.15.1

     Next Steps: Release workflow fully completed; v0.15.1 is live and available across all distribution channels


     Access 174k tokens of past research & decisions for just 17,179t. Use the claude-mem skill to access memories by
     ID.

     View Observations Live @ http://localhost:37777
```

### Re: CRDT duplicate root cause + fix

**Root cause:** Stale CRDT base state. When `merge(base_state, ours, theirs)` runs:
1. `base_text` is extracted from the `.yrs` state file
2. Char-level diffs computed: `base_text → ours` and `base_text → theirs`
3. If `base_text` is stale (doesn't match the actual common ancestor), both diffs contain Insert ops for the same content
4. CRDT sees two independent clients inserting the same text → duplicates

**Example:** base had old frontmatter, both ours/theirs had `'23'` → both generated Insert `'23'` → CRDT merged to `'2323'`.

**Fixes applied to `crdt.rs`:**

1. **Short-circuit:** If `ours == theirs`, skip merge entirely and return ours
2. **Stale base detection:** Compare common prefix length between base and each side. If both share < 50% with base, base is stale → use ours as base instead
3. **Base rebuild:** When stale base is detected, rebuild `base_encoded` from the new base text
4. **Logging:** `[crdt] merge: base_len=X ours_len=Y theirs_len=Z` on every merge + stale detection warnings

All 293 tests pass. Binary installed.

Tell me more about `apply_stream_from_string`. Why is it dead code? What is it supposed to be used for? I see:

> /// Used by `recover` to apply orphaned stream responses.

Is recover used to mitigate a stale claim?

Could you create a diagram of the call paths that converge to `merge`?

### Re: "What should we do with"

Your message was cut off. What were you asking about?

We need to stop getting cut off. Can we build a heuristic to where if a message is cut off, try again for the next diff?

### Re: Cut-off message heuristic

Good idea. Detection could work like:
- If the last non-whitespace character before `<!-- /agent:exchange -->` is mid-word (no sentence-ending punctuation), flag as potentially truncated
Yes...definately
- Or: check if the last line ends without a newline or period/question mark/exclamation
Don't do this unless the statement is obviously not complete. You can also check the document again after a short delay to see if more is added to that line.

**Implementation options:**
1. **In `diff.rs`** — after computing the diff, check if the last added line looks incomplete. Log a warning: `[diff] Warning: last change may be truncated`
This should cause a delayed recheck. Like a double tap. To get the rest of the sentence. If the sentence is still being typed, keep on rechecking. At the end of the sentence, recheck one last time to ensure that more content is not added to the line.
If the editing goes to the next line...Enter was pressed...present some options what can be done.
I think we should just cut off this diff at the end of the line...and save the next line(s) for the next run.
2. **In SKILL.md** — instruct Claude to detect incomplete messages and ask the user to complete them before responding
Follow the chain of delayed rechecks until we hit a terminal case...then use that diff. If agent-doc handles this entirely, agent-doc can block until the chain of rechecks are complete.
3. **In the binary** — `agent-doc diff --check-truncation` flag that returns a special exit code
With the chain of delayed rechecks, `--check-truncation` does not seem necessary. Do you agree?

Option 2 is zero-code. Want me to add it to SKILL.md, or implement option 1 in the binary? We need to stop getting cut off. Can we build a heuristic to where if a message is cut off, try again for the next diff?

We want option 1. The chain of rechecks.

There are 3 recovery bashes for this file. Why?

```
     ctrl+b ctrl+b (twice) to run in background

✻ Doodling… (43s · ↓ 66 tokens)
  ⎿  Tip: Use /btw to ask a quick side question without interrupting Claude's current work

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Background tasks
  3 active shells

  ❯ agent-doc recover tasks/agent-doc.md 2>&1; cat .agent-doc/claims.log 2>/dev/null && > .agent-… (running)
    agent-doc recover tasks/agent-doc.md && cat .agent-doc/claims.log 2>/dev/null && > .agent-doc… (running)
    agent-doc recover tasks/agent-doc.md && cat .agent-doc/claims.log 2>/dev/null && > .agent-doc… (running)

  ↑/↓ to select · Enter to view · x to stop · Esc to close
```
<!-- /agent:exchange -->
