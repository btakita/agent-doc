---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_mode: template
---

# agent-doc

<!-- agent:status -->
**Version:** v0.12.0 | **Tests:** 197 passing | **Dependencies:** tmux-router v0.2.2, instruction-files v0.1
**Published:** crates.io, PyPI, GitHub Releases
**Plugins:** JetBrains + VS Code at 9/9 SPEC feature parity
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
| v0.9.2 | `plugin install/update/list`, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |
| v0.9.5 | RegistryLock, SnapshotLock, flock advisory locking, stale claim auto-pruning |
| v0.9.6 | Atomic rename, doc advisory lock, stale snapshot recovery, diagnostic logging, 26 new tests |
| v0.9.7 | `agent-doc write` command, atomic response write-back with 3-way merge |
| v0.9.8 | Lock relocation to `.agent-doc/locks/<hash>.lock` |
| v0.9.9 | Template-mode, compaction recovery, `agent-doc recover`, `agent-doc template-info` |
| v0.9.10 | Code-span parser fix, template input/output component convention |
| v0.10.0 | Renamed `response_mode` → `agent_doc_mode` with aliases (mode, response_mode) |
| v0.10.1 | Bundle template-mode instructions in SKILL.md |
| v0.11.0 | Append-friendly merge strategy, shared merge module |
| v0.11.1 | Fix duplicate `## Assistant` heading in append-mode responses |
| v0.11.2 | Also strip trailing `## User` heading from responses |
| v0.12.0 | `convert` command, `compact` command, lazy claim via `route --pane`, exchange default for template init |
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

**v0.10.0 session:** Renamed `response_mode` → `agent_doc_mode` (canonical) with `mode` (shorthand) and `response_mode` (deprecated) as aliases. Discussed in-place conversation model and blog post.

**v0.10.1 session:** Designed `exchange` component convention for shared conversation surfaces. Diagnosed and fixed missing template-mode instructions in bundled SKILL.md. Released v0.10.1 to all targets. Converted this document from input/output to exchange component. Identified tmux claim-after-restart bug for next release.

**v0.11.0 session:** Implemented append-friendly merge strategy. Created shared `merge.rs` module extracting duplicated logic from `write.rs` and `submit.rs`. `resolve_append_conflicts()` auto-resolves conflicts where both sides only appended (empty original section in diff3 format). Added `merge_strategy` config field to `ComponentConfig`. Released to all targets.

**v0.11.1–v0.11.2 session:** Fixed duplicate `## Assistant` and `## User` headings. `strip_assistant_heading()` strips both leading `## Assistant` and trailing `## User` from response text before `agent-doc write` adds its own. Encountered CWD drift during release (relative path from wrong directory) — reinforces lesson #2 (always use absolute paths). Released both fixes to all targets.

**v0.12.0 session:** Implemented `agent-doc compact` (archive old exchanges), `agent-doc convert` (append→template), template init uses `exchange` by default, lazy claim via `route --pane`, fixed flaky `recover_append_response` test (snapshot path_for absolute paths). 197 tests. Published to all targets.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange -->
*Compacted. Resolved items (v0.11.0 merge strategy, v0.11.1–v0.11.2 heading fixes, CWD drift) folded into history.*

Is there an /agent-doc compact command?

### No `compact` command yet

Not built. It's in the pending list as `agent-doc compact — auto-summarize old exchanges`. For now, compaction is manual: ask the agent to compact, and it rewrites the exchange with resolved items folded into history summaries.

Implement agent-doc compact. What else is pending?

### `agent-doc compact` — Implemented

**New command:** `agent-doc compact <file> [--keep N]`

Archives old User/Assistant exchanges from append-mode documents to `.agent-doc/archives/<hash>-<timestamp>.md`, keeping only the most recent N exchanges (default: 2). A summary marker replaces the archived content.

**Features:**
- Parses exchanges by `## User` / `## Assistant` headings (respects code blocks)
- Only archives complete exchanges (trailing User block preserved as active)
- Archive file includes frontmatter with session ID, timestamp, exchange count
- Atomic write + snapshot update
- Template-mode documents rejected (components are already bounded)

**Usage:**
```bash
agent-doc compact tasks/plan.md           # keep 2 recent exchanges
agent-doc compact tasks/plan.md --keep 5  # keep 5 recent exchanges
```

**Tests:** 6 new tests (parse, code blocks, trailing user, archive format, compacted format, timestamp).
**Total:** 194 passing + 21 CLI tests. Clippy clean.

Removed `agent-doc compact` from the pending list.

**Other pending items (prioritized):**
1. Fix flaky `recover_append_response` test (parallel race condition)
2. Fix tmux claim-after-restart (plugin re-resolve pane binding)
3. Multi-agent backends (`agent/codex.rs`, `agent/gemini.rs`)
4. `agent-doc deep` — fan-out with git worktrees
5. `agent-doc convert` — append-to-template conversion
6. Plugin phases 5-7 (folding, gutter icons, structure view)
7. `agent-doc status` — Ratatui TUI
8. Response diff storage / undo / extract
9. Docker sandboxing, TmuxBatch, sub-profiles
10. Blog post on briantakita.me

In tasks/cogov.me.md, I asked agent-doc to use `agent_doc_mode: template`. It did not configure an exchange. Instead, it configured an input/output. It should use exchange by default.

Let's do pending 1 & 2.
Also agent-doc convert

### Fixes Applied

**Fix 1: Template init uses `exchange` by default**
- `init.rs`: Template mode now scaffolds `<!-- agent:exchange -->` instead of input/output
- `template.rs`: Auto-create fallback uses `exchange` component; existing component lookup checks for both `exchange` and `output` (backwards compatible)
- All tests updated and passing

**Fix 2: Flaky `recover_append_response` test**
- Root cause: `snapshot::path_for()` returned relative paths while `lock_path_for()` and `pending_path_for()` returned absolute paths. Parallel test threads with different CWDs caused relative path resolution to fail.
- Fix: `path_for()` now uses `find_project_root()` for absolute path resolution, consistent with the other path functions. Falls back to relative path only when no `.agent-doc/` directory exists.

**Fix 3: `agent-doc convert`** — not yet started. This was mentioned alongside pending 1 & 2.

Implement

**Fix 4: tmux claim-after-restart** — not yet started. What's the expected behavior? The plugin should detect when IDEA restarts and re-run `agent-doc claim` for open documents?

It would be better if we could do this claim lazily. Where calling agent-doc run would claim if the registered claim is no longer valid.

### Implemented: `agent-doc convert` + Lazy Claim

**`agent-doc convert <file>`** — Converts append-mode documents to template mode.

- Updates frontmatter: sets `agent_doc_mode: template`
- Finds first `## User` heading (skipping code blocks) and wraps from there to end in `<!-- agent:exchange -->...<!-- /agent:exchange -->`
- Content before `## User` (title, preamble) preserved outside the exchange component
- Adds `## Exchange` section heading
- Rejects documents already in template mode
- Atomic write + snapshot update
- 5 tests (user blocks, no blocks, rejection, frontmatter update, code block preservation)

```bash
agent-doc convert tasks/plan.md
```

**Lazy claim via `agent-doc route --pane`** — Auto-re-claims when existing claim is stale.

- Added `--pane` option to `agent-doc route`
- When the registered pane is dead and `--pane` is provided: re-registers the session to the new pane, then routes to it (skips auto-start)
- When no existing registration and `--pane` provided: registers and routes immediately
- When no `--pane`: falls back to auto-start as before
- Plugin can pass `--pane %<id>` on every route call, making explicit `claim` calls unnecessary after restart

`make check` passes: 197 tests + 21 CLI tests, clippy clean.

Ready to release. Suggest v0.12.0 since `convert` is a new feature and lazy claim changes the route API.

release 0.12.0

### v0.12.0 Released

Published to all targets:
- **crates.io**: https://crates.io/crates/agent-doc/0.12.0
- **PyPI**: agent-doc 0.12.0
- **GitHub**: https://github.com/btakita/agent-doc/releases/tag/v0.12.0
- **Binary installed**: `agent-doc 0.12.0`
- **SKILL.md updated**: v0.12.0

197 unit tests + 21 CLI tests passing.

compact this file
<!-- /agent:exchange -->
