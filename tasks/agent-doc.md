---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
---



## Status Summary (compacted 2026-03-07)

### Architecture

**tmux-router v0.2.2**: Standalone Rust crate (library + CLI). `Tmux` struct (30+ tmux ops + `capture_pane()` + `send_keys_raw()`), `Registry` (file-path-to-pane mappings), `reconcile()` (2D layout sync), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()`. CLI: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`. File-name addressing (registry lookup). Thread-local `REGISTRY_LOCK_HELD` flag for nested flock deadlock prevention. `acquire_or_skip()` for non-blocking lock attempts. 56 tests. Published to crates.io + GitHub Releases.

**agent-doc v0.9.6**: Interactive document sessions with AI agents. Depends on tmux-router 0.2.2. Published to crates.io, PyPI, GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Paired markers, nested supported. Config-driven modes via `.agent-doc/components.toml`.
- **`agent-doc patch`**: Replace/append/prepend component content. Shell hooks (`pre_patch` stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms), bounded cycle limiting (default 3), convergence detection.
- **`agent-doc plugin install`**: Download + install JetBrains/VS Code plugins from GitHub Releases. Auto-detect IDE dirs. Aliases: `jb`/`idea`/`code`/`codium`.
- **Comment stripping**: `strip_comments()` — strips `<!-- -->` and `[//]: # (...)` but preserves component markers.
- **Auto-resync**: `prune()` runs before route/sync/claim operations.
- **Stale snapshot recovery**: `is_stale_snapshot()` in `diff.rs` auto-detects when context compaction interrupted a previous cycle, syncs the snapshot without re-processing.
- **Diagnostic logging**: `[diff]`, `[submit]`, `[snapshot recovery]` prefixed stderr lines for debugging silent no-ops.

**Concurrency safety (v0.9.5+):**
- `SnapshotLock` — flock-based advisory locking for snapshot read/write (`fs2`)
- `RegistryLock` — flock-based advisory locking for `sessions.json` (in tmux-router)
- Atomic rename — `tempfile::NamedTempFile` + `persist()` for document and snapshot writes
- Document advisory lock — `<file>.md.agent-doc.lock` for agent-doc-vs-agent-doc coordination
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

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5–v0.6.6 | Window-scoped routing, 2D sync, Junie backend, LayoutDetector |
| v0.7.0–v0.7.2 | Snapshot-diff-apply sync, ATTACH-first reconciliation |
| v0.8.0 | tmux-router extraction (-3133 lines) |
| v0.9.0 | Components, patch, watch daemon. 150 tests. |
| v0.9.1 | Colons in component names |
| v0.9.2 | `plugin install/update/list`, VS Code 9/9 parity |
| v0.9.3 | tmux-router 0.1.3 (stash window fix for numeric sessions) |
| v0.9.4 | tmux-router 0.2.0 (CLI binary, capture_pane, send_keys_raw) |
| v0.9.5 | RegistryLock, SnapshotLock, flock advisory locking, stale claim auto-pruning |
| v0.9.6 | Atomic rename, doc advisory lock, stale snapshot recovery, diagnostic logging, race condition docs, 26 new tests |

### Comparison with Agent of Empires

AoE is a **session manager** (TUI dashboard, multi-agent, Docker, worktrees). agent-doc is a **document workflow** (markdown is the UI). Complementary — could run agent-doc inside AoE-managed panes.

**Ideas borrowed from AoE (priority order):**
1. Multi-agent backends (Codex, Gemini CLI via `agent/` trait) — low effort, high value
2. Git worktree isolation for `agent-doc deep` fan-out
3. `agent-doc status` TUI (dashboard + interactive modes)
4. Sub-profiles per submodule in `.agent-doc/config.toml`
5. Docker sandboxing (`agent-doc run --sandboxed`)

### Pending / Not Built

- Multi-agent backends (`agent/codex.rs`, `agent/gemini.rs`)
- `agent-doc deep` — fan-out parallel subagents with git worktrees, review checkpoint for fan-in
- `agent-doc status` — Ratatui TUI with dashboard + interactive pane attachment
- Sub-profiles for submodule workflows in `.agent-doc/config.toml`
- Docker sandboxing (`--sandboxed`)
- `TmuxBatch` — command batching to reduce flicker
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend
- `agent-doc init --dashboard` — scaffold template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`
- Execution mode in `config.toml` (currently only in SKILL.md frontmatter)

### Key Lessons

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

## User

Could you help me with why we have tasks/contabo.md.agent-doc.lock being located next to the file?

I don't want this file comitted into the git repo. Is it possible to have it in the .agent-doc directory while keeping the file path uniqueness? Should .agent-doc have a database/datafile of locks instead?
## Assistant

### Why lock files are next to the document

The document advisory lock (`<file>.md.agent-doc.lock`) was placed adjacent to the document for simplicity — `path.with_extension("md.agent-doc.lock")` is a one-liner. But you're right, it creates two problems:
1. Pollutes the working tree (shows up in `git status`)
2. Risk of accidental commit

### Recommended: Move locks to `.agent-doc/locks/`

Use the same SHA256-of-canonical-path scheme as snapshots:

```
.agent-doc/locks/<sha256_hash>.lock
```

This mirrors the existing snapshot pattern exactly:
- Snapshots: `.agent-doc/snapshots/<hash>.md`
- Locks: `.agent-doc/locks/<hash>.lock`

The change is small — two files need updating:
- `src/write.rs:86` — `let lock_path = path.with_extension("md.agent-doc.lock");`
- `src/submit.rs:140` — `let lock_path = path.with_extension("md.agent-doc.lock");`

Both would call a shared `lock_path_for(doc_path)` function that resolves the project root, computes the hash, and returns `.agent-doc/locks/<hash>.lock`.

**A database/datafile is overkill** — `flock()` operates on file descriptors, not database rows. One lock file per document is the right granularity. The hash-based naming preserves uniqueness without the file system clutter.

**Quick mitigation now**: Add `*.agent-doc.lock` to `.gitignore` so existing lock files don't get committed while we implement the proper fix.

Want me to implement the lock relocation in `agent-doc`?

## User

Yes...hashed lock relocation.
## Assistant

### Implementation Complete

Relocated document advisory locks from sibling files to `.agent-doc/locks/<hash>.lock`:

**Changes (3 files):**

1. **`snapshot.rs`** — Added `doc_hash()` (extracts SHA256 hashing), `lock_path_for()` (returns `.agent-doc/locks/<hash>.lock`), and `find_project_root()` (walks up to find `.agent-doc/` directory). Refactored `path_for()` to use `doc_hash()`.

2. **`submit.rs:139-140`** — `acquire_doc_lock` now calls `crate::snapshot::lock_path_for(path)` instead of `path.with_extension("md.agent-doc.lock")`.

3. **`write.rs:85-86`** — Same change as submit.rs.

**Cleanup:**
- Removed stale `tasks/*.agent-doc.lock` files
- Added `*.agent-doc.lock` to `.gitignore` (safety net for old binaries)

**How `lock_path_for` works:**
1. Compute SHA256 of canonical document path (via `doc_hash()`)
2. Walk up from document to find `.agent-doc/` project root
3. Return `<project_root>/.agent-doc/locks/<hash>.lock`
4. Falls back to document's parent dir if no project root found

All 177 tests pass. Ready to release as v0.9.7.

## User

release
## Assistant

### Released v0.9.8

Version bumped from 0.9.7 to 0.9.8 (0.9.7 was already published for the `agent-doc write` command).

**Published to all targets:**
- crates.io: `agent-doc v0.9.8`
- PyPI: `agent_doc-0.9.8`
- GitHub Release: https://github.com/btakita/agent-doc/releases/tag/v0.9.8
- Local binary: `~/.cargo/bin/agent-doc` (v0.9.8)

**What's in v0.9.8:**
- Relocate doc advisory locks to `.agent-doc/locks/<hash>.lock`
- `doc_hash()`, `lock_path_for()`, `find_project_root()` in `snapshot.rs`
- 177 tests passing

## User

Can we clean up the existing tasks/*.agent-doc.lock files?
## Assistant

Done — removed 2 stale lock files:
- `tasks/agent-doc.md.agent-doc.lock`
- `tasks/contabo.md.agent-doc.lock`

These were recreated by the old v0.9.7 binary earlier in this session (before the v0.9.8 install). Now that v0.9.8 is installed, new locks go to `.agent-doc/locks/` and won't appear in the working tree. The `.gitignore` entry (`*.agent-doc.lock`) also prevents any stragglers from being committed.

## User

I want to explore the idea of having responses use the dashboard-as-document templates.

Currently, the responses default to append at the end of the document.

Some documents may be better having inline annotation and responses.
Where the annotations and responses occur at the templated location.

What would the UX look like:
- to being one of these documents?
- to transition a default append-only document to an in-place response document?

How can the user maintain order in the document? Preventing content from spilling over?
Can code folding with the editor be used?

Would it be possible to have a dashboard-like or IDE-like interface within the document?

## Assistant

### In-Place Response Documents (Dashboard-as-Document)

This builds on the existing component model. Components (`<!-- agent:name -->...<!-- /agent:name -->`) already support replace/append/prepend. The idea extends this to make the entire document a template where responses target specific regions.

#### Two Document Modes

**Append mode (current default):** Linear `## User` / `## Assistant` conversation. Good for open-ended sessions.

**Template mode (proposed):**
```markdown
---
response_mode: template
---

# Project Dashboard

## Status
<!-- agent:status -->
Build passing. 3 open PRs.
<!-- /agent:status -->

## Recent Activity
<!-- agent:activity mode="append" max_entries="10" -->
- 2026-03-07: Released v0.9.8
<!-- /agent:activity -->

## Questions
<!-- agent:questions -->
User annotates inline; agent responds in-place.
<!-- /agent:questions -->
```

Agent responds by patching specific components instead of appending `## Assistant` blocks.

#### UX: Starting a Template Document

- **Frontmatter flag:** `response_mode: template` — explicit opt-in
- **Scaffold:** `agent-doc init --template dashboard` for quick setup
- **Auto-detect fallback:** If document has `<!-- agent:* -->` components and no `## User`/`## Assistant` structure, infer template mode

#### UX: Transitioning Append → Template

1. Add `response_mode: template` to frontmatter
2. Wrap sections in component markers
3. Next `/agent-doc` submit detects template mode
4. Old `## User`/`## Assistant` blocks remain as read-only history/context

Incremental — can mix append history at top with template regions below.

#### Preventing Spillover

**Component boundaries are the guard rails.** Agent can only modify within its assigned components. User content outside is untouchable.

Additional controls via `components.toml`:
- `max_entries` — cap on append-mode items (already designed)
- `max_lines` — hard limit on component content length
- `frozen` — read-only component (agent reads but can't write)

#### Code Folding

Component markers are natural fold points. Both IDEs support this:

```
▶ <!-- agent:status -->     [collapsed]
▶ <!-- agent:activity -->   [collapsed]
▼ <!-- agent:questions -->  [expanded — user is working here]
```

**JetBrains:** Register `FoldingBuilder` detecting `<!-- agent:* -->` pairs
**VS Code:** Custom folding provider for component regions

Gives a collapsed dashboard view — user sees section headers, expands to work.

#### Dashboard/IDE Interface

With folding + template mode:
- **Gutter icons** — fold/unfold, refresh per component
- **Structure view** — component tree in IDE sidebar
- **Quick-nav popup** — jump between components
- User navigates by expanding/collapsing. Agent updates on each submit.

#### Implementation Path

1. `response_mode: template | append` in frontmatter (default: append)
2. Template-mode submit: extract user annotations from components, respond via `patch` per component
3. `FoldingBuilder` (JetBrains) / folding provider (VS Code)
4. `max_lines` and `frozen` in component config

Builds entirely on existing component infrastructure. Main new work is the template-mode submit path.

## User

Would another TUI "editor" be useful? How would some recent editors such as ki work in this flow?
## Assistant

### TUI Editors in the Agent-Doc Flow

**Ki Editor** ([ki-editor.org](https://ki-editor.org)) is a multi-cursor combinatoric modal TUI editor written in Rust. Its name means "tree" (木) in Japanese — fitting, since it uses Tree-sitter for first-class structural editing.

**What makes Ki interesting here:**
- **Unified component model** — every UI element (prompt, completion dropdown, file explorer) is a full editor with the same keybindings. Mirrors agent-doc's "everything is a document" philosophy.
- **Selection mode → Movement → Action** — more composable than Vim (`Action → Movement`) or Helix (`Movement → Action`).
- **Positional keybindings** — layout-agnostic (QWERTY, Dvorak, Colemak). No mnemonics.
- **Tree-sitter structural editing** — one key to select the enclosing syntax node. Markdown component markers (`<!-- agent:name -->`) could be registered as structural units.
- **Auto-reload** — detects external file changes without manual refresh.

**Two integration models:**

**1. TUI as editing surface (works today)**
Ki (or Helix, Neovim) as the editor. `agent-doc watch` detects saves, submits to agent, patches the document. Ki's auto-reload shows updates live. Zero Ki-specific code needed.

**2. Purpose-built dashboard TUI (future `agent-doc status`)**
A Ratatui-based interactive viewer for template-mode documents:
- Component regions as foldable panes (like tmux splits in one terminal)
- Expand a component → edit inline → submit
- Agent responses patch specific components, TUI re-renders
- Built into agent-doc, not dependent on external editor

**The Ki-specific opportunity:**
Ki's "everything is an editor" architecture means a plugin could treat `<!-- agent:name -->` component regions as Tree-sitter structural units. Pressing `d` (select enclosing node) inside a component selects the entire body. Multi-cursor lets you edit multiple components simultaneously, then submit once.

**Pragmatic path:** Use any TUI editor as the editing surface + `agent-doc watch` for agent interaction. Build the dashboard TUI separately as `agent-doc status` when the need justifies it. Ki's structural editing is a bonus but not a prerequisite — the flow works with any editor that saves files.

## User

What are the next steps to support In-Place Response Documents for Jetbrains and VSCode?
Please put all common logic into the agent-doc binary.
## Assistant

### Implementation Plan: In-Place Response Documents

All common logic lives in the `agent-doc` binary. Plugins are thin UI layers that call CLI commands.

#### Phase 1 — Binary: Template-mode detection

**Files:** `frontmatter.rs`, `submit.rs`

Add `response_mode: Option<String>` to `Frontmatter`:
```yaml
---
response_mode: template   # "template" | "append" (default)
---
```

In `submit.rs`, after computing the diff, branch based on `response_mode`:
- **append** (current): `## Assistant` block appended
- **template**: patch targeted components instead

If we have template, but there is not template field for the output, should we add an output field at the end of the document by default?

#### Phase 2 — Binary: `agent-doc template-info <FILE>`

New subcommand that outputs JSON describing the document's template structure:
```json
{
  "template_mode": true,
  "components": [
    { "name": "status", "mode": "replace", "content": "Build passing.", "line": 12 },
    { "name": "activity", "mode": "append", "max_entries": 10, "content": "...", "line": 18 }
  ]
}
```

Single CLI call gives plugins everything they need for rendering (folding ranges, gutter icons, structure view). No parsing logic in plugins.

#### Phase 3 — Binary: Template-mode submit path

When `response_mode: template`:
1. **Prompt includes component structure** — names, modes, current content (not `## User`/`## Assistant` history)
2. **Agent responds with patch blocks** using `<!-- patch:name -->` markers:
   ```markdown
   <!-- patch:status -->
   Build passing. 4 open PRs.
   <!-- /patch:status -->

   <!-- patch:activity -->
   - 2026-03-07: Implemented template mode
   <!-- /patch:activity -->
   ```
3. **New `apply_template_patches()`** in `submit.rs` parses `<!-- patch:name -->` blocks from response, calls existing `patch.rs` logic per component
4. Same atomic write + 3-way merge + snapshot save

The `<!-- patch:name -->` convention reuses marker syntax the agent already knows. Distinct prefix (`patch:` vs `agent:`) prevents confusion between document components and response directives.

#### Phase 4 — Binary: `agent-doc write --template`

Extend `write.rs` to accept template-mode responses from stdin:
```bash
echo "<structured response>" | agent-doc write plan.md --template --baseline-file /tmp/baseline.md
```

Parses `<!-- patch:name -->` blocks, patches components, atomic write. This is what the SKILL.md workflow calls for template-mode documents.

#### Phase 5 — JetBrains plugin: Component folding

- Register `FoldingBuilder` detecting `<!-- agent:name -->...<!-- /agent:name -->` pairs
- Collapsed placeholder shows component name + mode: `[agent:status (replace)]`
- Call `agent-doc template-info` for fold range data (no markdown parsing in Kotlin)

#### Phase 6 — VS Code extension: Component folding

- Custom `FoldingRangeProvider` for the same markers
- Call `agent-doc template-info` for structure (no markdown parsing in TypeScript)

#### Phase 7 — Both plugins: Gutter icons + structure view

- Gutter icons: fold/unfold, submit single component
- Structure view: component tree in IDE sidebar
- Quick-nav popup: jump between components

#### Dependency chain

```
Phase 1 (frontmatter) → Phase 3 (submit path) → Phase 4 (write --template)
Phase 2 (template-info) → Phase 5+6 (folding) → Phase 7 (gutter/structure)
```

Phases 1-4 (binary) and Phase 2 (template-info) can proceed in parallel. Plugin work (5-7) depends on Phase 2 shipping first.

#### What already exists

- `component.rs` — full parser with nesting, `replace_content()`, `is_agent_marker()`
- `patch.rs` — replace/append/prepend engine with `components.toml` config, shell hooks
- `write.rs` — 3-way merge, atomic write, snapshot save
- `submit.rs` — diff → send → write flow with doc advisory locking

Template mode is a new submit path, not a rewrite. ~200-300 lines of new Rust code for Phases 1-4.

## User

Let's go forward with the plan. Note my question above.
