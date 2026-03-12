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

### Re: `/agent-doc compact` duplicated in CRDT response

Known issue. The CRDT merge in stream mode is producing duplicates — same root cause as the frontmatter character doubling bug from earlier. The `crdt::merge()` function's `compute_edit_ops()` is generating duplicate insert operations. This needs a focused debugging session on `crdt.rs`.

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

### Re: "What should we do with"

Your message was cut off. What were you asking about? We need to stop getting cut off. Can we build a heuristic to where if a message is cut off, try again for the next diff?
<!-- /agent:exchange -->
