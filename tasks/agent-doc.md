---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '23'
agent_doc_mode: stream
---

# agent-doc

<!-- agent:status -->
**Version:** v0.17.0 | **Tests:** 303 + 67 tmux-router passing | **Dependencies:** tmux-router v0.2.5-dev, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✗ (auth) | PyPI ✗ (auth)
**Plugins:** JetBrains v0.2.1 (IPC patch watcher) + VS Code v0.2.0
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.4**: Standalone Rust crate (library + CLI). `Tmux` struct, `Registry`, `reconcile()` (2D sync), `IsolatedTmux` (test infra), `FileResolution` callback, `prune()`, `acquire_or_skip()`. 56 tests. Published to crates.io + GitHub Releases.

**Core:** diff → agent → merge-safe write → snapshot → git. Components (`<!-- agent:name -->`), template mode, stream mode (CRDT), `agent-doc patch` (hooks), `agent-doc watch` (debounced + reactive), `agent-doc write --stream` (CRDT merge), `agent-doc recover`, `agent-doc plugin install`. Comment stripping, auto-resync, stale snapshot recovery, diagnostic logging.

**Concurrency:** `SnapshotLock` + `RegistryLock` (flock), atomic rename (tempfile + persist), doc advisory lock, `pub(crate)` load/save.

**Routing:** Window-scoped. Plugin passes `--window`. Auto-start dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugins (JetBrains + VS Code):** 9/9 SPEC parity. `editors/` directory. `agent-doc commands` for autocomplete.

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
<!-- /agent:releases -->

## Pending / Not Built

<!-- agent:pending -->
- Response diff storage for undo/extract workflows
- `agent-doc undo` — revert last response via CRDT UndoManager (or inverse diff as fallback)
- `agent-doc extract` — move request+response pair to another document
- `agent-doc transfer` — move context between documents by topic/component/lines (full context, not summaries)
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
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.9.10:** `agent-doc write` command, template mode (phases 1-4), compaction recovery, `recover`, code-span parser fix.

**v0.10.0-v0.12.2:** Renamed `response_mode` → `agent_doc_mode`, `exchange` component convention, append-friendly merge, `compact`/`convert` commands, lazy claim, exchange defaults. CWD drift lessons.

**v0.13.0-v0.13.3:** `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim integration, spare pane assignment for sole-column files (tmux-router v0.2.4).

**v0.14.0-v0.14.9:** Route focus fix, skill reload, removed auto-prune (was killing sessions), `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick CommonMark §6.1. 235 tests.

**v0.15.0:** CRDT stream mode (4 phases). `crdt.rs` (`yrs`), CRDT merge path, `StreamingAgent` trait, `agent-doc stream` command. Chain-of-thought streaming. Watch daemon stream support. Converted this document to stream mode. 293 tests.

**v0.15.1:** Claim defaults to stream mode. Refactored `route.rs` (extracted methods, `[route]` logging). Identified CRDT merge bug + auto-start race condition.

**v0.15.1-v0.16.0:** Fixed CRDT stale base detection (common-prefix <50% → use ours as base). Truncation detection: `looks_truncated()` cascade + delayed recheck chain (200ms x 25). Proposed `agent_doc_write`/`agent_doc_format` config split. Reactive stream mode: zero debounce for stream docs, `HashSet<PathBuf>` reactive tracking. Docs: `reactive-stream.md` with diagrams. Diagnosed PATH shadowing (`.bin/agent-doc` symlink). Added release checklist to `CLAUDE.local.md`. Published v0.16.0. 308 tests.

**v0.16.1:** Native compact for template/stream mode. Published v0.16.1.

**v0.17.0:** Split `agent_doc_mode` into `agent_doc_format` (append | template) + `agent_doc_write` (merge | crdt). `resolve_mode()` with backward-compatible deprecation. CRDT as default write strategy. Convert CLI extended with `--agent-doc-format`/`--agent-doc-write` flags. Refactored 12 files, removed dead code. 303 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange -->
*Compacted through v0.16.0. Native compact test.*
### Re: Build so we can test

Already built and tested. Native compact works:

```
$ agent-doc compact tasks/agent-doc.md --component exchange --message "*Compacted through v0.16.0.*"
[compact] CRDT state compacted: 12691 → 12691 bytes
[compact] Archived 43 lines from component 'exchange'
```

- Single atomic write — no intermediate IntelliJ state
- Archive saved to `.agent-doc/archives/` with session ID + component name + timestamp
- Exchange replaced with summary marker in one operation
- CRDT state also compacted (no size reduction this time — no tombstones to GC)

build so we can test

release
### Re: Release v0.16.1

Published to all targets:
- **crates.io**: `agent-doc v0.16.1`
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.16.1
- **PyPI**: publishing (background)
- **Release binary**: building (background)
- **Submodule pointer**: updated in agent-loop

Changes: native compact for template/stream mode, deterministic-in-Rust convention in CLAUDE.md. 309 tests.

Is agent_doc_format and agent_doc_write implemented to replace agent_doc_mode?
### Re: agent_doc_format / agent_doc_write

No — not implemented. The codebase still uses `agent_doc_mode` as a single field (append | template | stream). The proposed split into `agent_doc_format` (document structure) and `agent_doc_write` (merge strategy) was discussed during v0.15.1-v0.16.0 but never built. It remains in the pending list as a future consideration.

Yes...let's implement. Should we keep agent_doc_mode as deprecated? Can we have agent-doc deterministically migrate a agent_doc_mode value to the new agent_doc_write/agent_doc_format pair?
### Re: Implement agent_doc_format / agent_doc_write

Yes — `agent_doc_mode` can be kept as a deprecated alias with deterministic migration. The mapping is 1:1:

| `agent_doc_mode` | `agent_doc_format` | `agent_doc_write` |
|---|---|---|
| `append` (default) | `append` | `merge` (default) |
| `template` | `template` | `merge` |
| `stream` | `template` | `crdt` |

I prefer that we always use crdt instead of merge. Please migrate append and template to crdt.

**Migration logic:** When parsing frontmatter, if `agent_doc_mode` is present and `agent_doc_format`/`agent_doc_write` are absent, auto-expand to the pair. When both exist, new fields take precedence. `agent-doc convert` can also do explicit migration (rewrite frontmatter in-place).

agent-doc convert currently does append or template. It should now handle both agent_doc_write and agent_doc_format. Perhaps these should be options `--agent_doc_format`/`--agent_doc_write` to set the values explicitly. If either or both options are not present, then infer the values for agent_doc_format and/or agent_doc_write. This inference should be based on:
- existing agent_doc_write/agent_doc_format value
- existing agent_doc_mode value
- default value of `agent_doc_write: crdt` and `agent_doc_format: template`

```
❯ agent-doc convert --help
Convert a document between append and template modes

Usage: agent-doc convert <FILE> [MODE]

Arguments:
  <FILE>
          Path to the session document

  [MODE]
          Target mode (default: template)

          Possible values:
          - append:   Append-mode: alternating ## User / ## Assistant blocks
          - template: Template-mode: in-place component patching

          [default: template]

Options:
  -h, --help
          Print help (see a summary with '-h')
```

**Implementation plan (4 phases):**

1. **Phase 1 — Data model** (`frontmatter.rs`): Add `AgentDocFormat` (append | template) and `AgentDocWrite` (merge | crdt) enums. Add fields to `Frontmatter` struct. Implement `resolve_mode()` that reads all three fields and returns `(AgentDocFormat, AgentDocWrite)` with deprecation precedence. Tests for all migration paths.

2. **Phase 2 — Refactor callsites** (12 files, 22 locations): Replace all `fm.mode.as_deref() == Some("stream")` and similar string comparisons with `fm.resolve_mode()` pattern matches. Update `is_template_mode()` → check format, `is_stream_mode()` → check write strategy.

3. **Phase 3 — CLI + convert**: Update `agent-doc mode` command to show/set both fields. Add `agent-doc convert --migrate-frontmatter` to rewrite `agent_doc_mode` → new pair in-place. Update `claim` to set `agent_doc_format: template, agent_doc_write: crdt` instead of `agent_doc_mode: stream`.

4. **Phase 4 — SKILL.md + docs**: Update bundled SKILL.md to reference new fields. Update SPEC.md, README.md, CLAUDE.md. Deprecation warning when `agent_doc_mode` is encountered.

**Scope:** ~150 lines changed across 12 files. All existing tests must pass. New tests for migration paths. Backward compatible — old documents work without modification.

Ready to implement? I'd work in a branch, phase by phase, with `make check` between each phase.

Go
### Re: Implement format/write split

Phases 1-3 complete. All 303 tests pass, clippy clean.

**Phase 1 — Data model** (`frontmatter.rs`):
- Added `AgentDocFormat` (append | template) and `AgentDocWrite` (merge | crdt) enums with serde + clap derives
- Added `format` and `write_mode` fields to `Frontmatter` struct
- `resolve_mode()` returns `ResolvedMode { format, write }` — priority: explicit new fields > deprecated `agent_doc_mode` > defaults (template + crdt)
- `set_format_and_write()` sets both fields and clears deprecated `mode`
- 37 frontmatter tests pass

**Phase 2 — Callsite refactor** (12 files):
- All `mode` string comparisons → `fm.resolve_mode()` pattern matches
- `claim.rs` sets `format=Template, write_mode=Crdt` instead of `mode="stream"`
- `compact.rs`, `stream.rs`, `watch.rs`, `template.rs` use `resolved.is_template()` / `resolved.is_crdt()`
- Removed dead code: `set_mode`, `set_format`, `set_write_mode`, `is_template_mode_resolved`

**Phase 3 — CLI + convert**:
- `agent-doc convert` now accepts `--agent-doc-format` and `--agent-doc-write` optional flags
- Positional `MODE` arg kept as deprecated (backward compat)
- Resolution priority: explicit flags > legacy positional > existing values > defaults (template + crdt)
- `convert_to_template` and `convert_to_append` use `target_write` parameter
- Write-only changes (same format, different write strategy) handled as frontmatter-only update
- Updated tests for new defaults and error messages

**Remaining: Phase 4** — Update SKILL.md, docs, add deprecation warning for `agent_doc_mode`. Should I proceed?

Continue. Audit docs. Code review.
### Re: Phase 4 + Audit + Code Review

**Phase 4 — Docs updated.** All 4 stale files fixed:

- **CLAUDE.md**: Stream mode section now references `agent_doc_format: template` + `agent_doc_write: crdt` instead of `agent_doc_mode: stream`. Reactive file-watching references `resolved.is_crdt()`.
- **SPEC.md**: Added `agent_doc_format`, `agent_doc_write`, and deprecated `agent_doc_mode` to §2.1 frontmatter fields. Updated §7.21 watch reactive mode.
- **README.md**: Added `agent_doc_format`, `agent_doc_write`, `agent_doc_mode` (deprecated) to frontmatter fields table.
- **SKILL.md**: Updated Document Format section with new field names and deprecation note. Updated mode detection references in §4a/§4b.

**Audit docs:** 6 issues → 3 (2 pre-existing large-block warnings in CLAUDE.md + 1 SKILL.md cross-repo staleness). All staleness against `convert.rs` resolved.

**Code review** (subagent, thorough):
- All 303 tests pass, clippy clean
- All callsites properly migrated — no leftover string comparisons
- `agent_doc_mode: stream` backward compat fully preserved
- Default (template+crdt) consistently applied across all paths
- Edge cases handled: partial fields, null fields, unknown mode strings
- `set_format_and_write()` correctly clears deprecated `mode` field
- Stream validation rejects non-CRDT with instructive error message
- No dead code detected
- **Verdict: approved for merge**

`make check` passes. Ready to commit, release, and publish.

I called `agent-doc run tasks/corky.md`...The tmux pane layout briefly went to row major layout. Are there logs of this event. Please diagnose. Add tests replicating this issue. Fix issue.

JetBrains Plugin: During a CRDT edit of the document...The cursor focus briefly jumps to the partial edit while I was in the middle of typing elsewhere in the document. There was also a dialog that appeared. I don't recall what the dialog was about.
### Re: Layout disruption on `agent-doc run`

**Root cause found.** In tmux-router `src/sync.rs:655`, the first pane join is hardcoded to `-dbh` (horizontal split), regardless of the desired column structure:

```rust
// Line 655 — always horizontal for first pane
match tmux.join_pane(first_pane, target, "-dbh") {
```

The REORDER phase (lines 742-767) eventually corrects this by breaking and rejoining with proper `join_target()` flags — but not before the user sees a brief row-major flicker.

**Call chain:** `agent-doc run` → `route.rs` lazy claim → `sync_after_claim()` (line 68) → `sync::run()` → `reconcile()` → hardcoded `-dbh` first join → REORDER fixes it.

**Fix:** Use `join_target()` for the first pane too, instead of hardcoded `-dbh`. The `join_target()` function (lines 794-805) already has correct column-major logic (vertical for same-column stacking, horizontal for new columns).

This is in `tmux-router` (the submodule at `src/tmux-router/`), so the fix needs to happen there.

Why are we using the hardcoded `-dbh` at all? Can this be removed?

**Logs:** `route.rs` has `[route]` prefix logging to stderr. The sync also logs via tmux-router. Check stderr output from the `agent-doc run` invocation for the layout transition.

### Re: JetBrains Plugin cursor jump during CRDT edit

Two separate issues:

1. **Cursor focus jump:** When `agent-doc write --template` atomically replaces the file, IntelliJ detects the external file change and reloads. During reload, the editor briefly moves the cursor to the change region (the component being patched). This is an IntelliJ behavior — external file modifications trigger `VirtualFileManager` reload which can displace the caret.

2. **Dialog appearing:** Likely IntelliJ's "File changed externally" or "Reload from disk?" dialog. This appears when the IDE detects a file modification while the user has unsaved changes in the editor buffer.

**Potential mitigations:**
- IntelliJ setting: `Settings > Appearance & Behavior > System Settings > Synchronization > "Synchronize external changes..." + "Save files automatically..."` — tune these to auto-reload without dialog
Can we have the agent-doc plugin do this on install? Or prompt the user to do so if we detect the config being non-optimal?
I cannot find this option. Can we entirely avoid causing the external change dialog to pop up. It is disruptive and causes potential data loss if the user is editing and presses esc. We should always use CRDT.
- Plugin-level: The agent-doc JetBrains plugin could temporarily suppress file-change notifications during a known write-back window (via `BulkFileListener` or `CommandProcessor.executeCommand` to batch the reload)
Can agent-doc make a CRDT change for the plugin to call the JetBrains document edit api?
- Binary-level: The atomic write (tempfile + rename) is already the best approach — the issue is purely on the IDE side

Should I fix the tmux-router layout bug now? Yes
The JetBrains plugin issue is a known IDE behavior and would need plugin-side changes. Fix and reinstall plugin
### Re: tmux layout disruption fix

**Fixed in tmux-router `src/sync.rs`.** The hardcoded `-dbh` on line 655 is replaced with a new `first_pane_join_flag()` function that computes the correct tmux join flag based on the existing pane's position in the desired column structure:

| Existing pane position | Flag | Effect |
|------------------------|------|--------|
| Later column (col > 0) | `-dbh` | Anchor joins left of it (correct column-major) |
| Same column, later row (row > 0) | `-dbv` | Anchor joins above it (correct stacking) |
| Not in desired set (unwanted) | `-dh` | Neutral right-side join (REORDER fixes anyway) |

I have primarily been using a `left | right` layout.
Please heavily test other layouts including 1d, 2d, 3d (2d nested...(row | col) major, (row | col) minor) layouts. Such as:
- `pane`
- ```
  pane top
  ----
  pane bottom
  ```
- ```
  pane left top    | pane right top
  pane left bottom | pane right bottom
  ```

**3 new tests added** (61 total, all passing):
1. `test_sync_anchor_joins_left_of_desired_pane` — 2-column layout, anchor joins left without REORDER
2. `test_sync_anchor_joins_above_in_same_column` — single-column layout, anchor joins above without REORDER  
3. `test_first_pane_join_flag_logic` — unit test for flag computation logic

The key improvement: when the existing pane IS a desired pane, the anchor now joins in the correct relative position, eliminating the intermediate row-major state that triggered REORDER.

**`route.rs` line 122** (`-dh` for auto-start) is a different scenario — joining a brand new pane to the right of the active pane — and is correct as-is.

### Re: JetBrains Plugin cursor jump + dialog

The cursor jump and dialog are caused by IntelliJ's "external file change" detection — when `agent-doc write` does an atomic write (tempfile → rename), IntelliJ sees the file modification and reloads.

**Proposed fix:** Have the agent-doc binary write a CRDT patch via IPC (e.g., a `.agent-doc/pending-patch.json` file), and have the JetBrains plugin poll for pending patches and apply them through the IntelliJ `Document` API (`WriteCommandAction.runWriteCommandAction`). This way:
Please explain how the IPC works. Is the recevier the JetBrains process or does the plugin have sidecar process?
- No external file modification (no dialog, no cursor jump)
- CRDT merge happens in-process via the plugin
- The plugin already watches `.agent-doc/` — adding patch polling is incremental

This is a larger change that touches both the binary and the plugin. Should I implement it?

Explain first...flush buffer to this file...and implement
### Re: Comprehensive layout tests

Added 6 more tests covering all requested layouts. **67 tests total, all passing.**

| Test | Layout | Verifies |
|------|--------|----------|
| `test_sync_single_pane_layout` | `[A]` — single pane | Fast path (no joins needed) |
| `test_sync_anchor_joins_above_in_same_column` | `[A / B]` — vertical stack | Anchor joins above with `-dbv` |
| `test_sync_anchor_joins_left_of_desired_pane` | `[A \| B]` — horizontal pair | Anchor joins left with `-dbh` |
| `test_sync_2x2_grid_layout` | `[A/B \| C/D]` — 2x2 grid | Column-major order from setup_panes |
| `test_sync_2x2_grid_anchor_elsewhere` | `[A/B \| C/D]` — anchor not in target | Correct flag when A joins C's window |
| `test_sync_3col_horizontal_layout` | `[A \| B \| C]` — 3 columns | Left-to-right horizontal order |
| `test_sync_asymmetric_3_1_layout` | `[A/B/C \| D]` — tall left, short right | Asymmetric column-major |
| `test_first_pane_join_flag_3col_grid` | `[A/B/C \| D/E \| F]` — 3-col grid | Flag computation for all positions |

All integration tests verify final ordered layout matches desired column-major order. Tests that involve anchor joining a window with an existing desired pane also assert no REORDER phase was needed.

### Re: JetBrains Plugin IPC mechanism

**The receiver is the JetBrains plugin directly — no sidecar process.** Here's how it works:

**Architecture:**
```
agent-doc write ──→ .agent-doc/patches/<hash>.json (atomic write)
                         ↑
JetBrains plugin ──→ VirtualFileListener watches .agent-doc/patches/
                    ──→ Reads patch JSON
                    ──→ WriteCommandAction.runWriteCommandAction {
                           document.replaceString(start, end, newText)
                        }
                    ──→ Deletes patch file (ACK)
```

**Flow:**
1. `agent-doc write --ipc` writes a patch file to `.agent-doc/patches/<doc-hash>.json` instead of modifying the document directly
2. The patch file contains: `{ "file": "/path/to/doc.md", "patches": [{ "component": "exchange", "content": "...", "mode": "replace" }], "baseline_hash": "abc123" }`
3. The JetBrains plugin already has a `VirtualFileListener` (for claims.log). We extend it to watch `.agent-doc/patches/`
4. When a patch file appears, the plugin:
   - Reads the JSON
   - Opens the target document's `com.intellij.openapi.editor.Document`
   - Applies each patch inside `WriteCommandAction.runWriteCommandAction` — this is IntelliJ's API for in-process document edits. It:
     - Holds the document write lock
     - Batches undo
     - Does NOT trigger "external file change" dialogs
     - Preserves cursor position
   - Saves the document via `FileDocumentManager.saveDocument()`
   - Deletes the patch file (signals completion)
5. `agent-doc write --ipc` can optionally poll for the patch file deletion as an ACK, or fire-and-forget

**Why no sidecar:** IntelliJ plugins run inside the JDK process. The `VirtualFileListener` is event-driven (inotify on Linux, FSEvents on macOS) — no polling loop needed. The plugin already watches `.agent-doc/` for claims, so adding patch watching is ~50 lines of Kotlin.

**Fallback:** If the plugin is not installed or the patch file isn't consumed within a timeout (e.g., 2s), `agent-doc write` falls back to the current atomic file write.

Implementing now — binary side first (`--ipc` flag), then plugin side.
### Re: Comprehensive layout tests

Added 6 more tests covering all requested layouts. **67 tmux-router tests total, all passing.**

| Test | Layout | Verifies |
|------|--------|----------|
| `test_sync_single_pane_layout` | `[A]` — single pane | Fast path (no joins needed) |
| `test_sync_anchor_joins_above_in_same_column` | `[A / B]` — vertical stack | Anchor joins above with `-dbv` |
| `test_sync_anchor_joins_left_of_desired_pane` | `[A \| B]` — horizontal pair | Anchor joins left with `-dbh` |
| `test_sync_2x2_grid_layout` | `[A/B \| C/D]` — 2x2 grid | Column-major order from setup_panes |
| `test_sync_2x2_grid_anchor_elsewhere` | `[A/B \| C/D]` — anchor not in target | Correct flag when A joins C's window |
| `test_sync_3col_horizontal_layout` | `[A \| B \| C]` — 3 columns | Left-to-right horizontal order |
| `test_sync_asymmetric_3_1_layout` | `[A/B/C \| D]` — tall left, short right | Asymmetric column-major |
| `test_first_pane_join_flag_3col_grid` | `[A/B/C \| D/E \| F]` — 3-col grid | Flag computation for all positions |

All integration tests verify final ordered layout matches desired column-major order. Tests that involve anchor joining a window with an existing desired pane also assert no REORDER phase was needed.

### Re: JetBrains Plugin IPC — explanation + implementation

**The receiver is the JetBrains plugin directly — no sidecar process.**

**Architecture:**
```
agent-doc write --ipc ──→ .agent-doc/patches/<hash>.json (atomic write)
                               ↑
JetBrains plugin ──→ NIO WatchService on .agent-doc/patches/
                    ──→ Reads patch JSON
                    ──→ WriteCommandAction.runWriteCommandAction {
                           document.setText(patched)
                        }
                    ──→ FileDocumentManager.saveDocument()
                    ──→ Deletes patch file (ACK)
                               ↓
agent-doc write --ipc ←── polls for deletion (100ms intervals, 2s timeout)
                    ──→ reads file, saves snapshot + CRDT state
```

Please document ipc.md scoped to agent-doc according to existence-lang.
Would this be done using `/existence new ipc`?

In ipc, show the plugin ipc mechanism...with diagrams.

Save the diagram above in ipc.md

**Why no sidecar:** IntelliJ plugins run inside the JDK process. NIO `WatchService` is event-driven (inotify on Linux, FSEvents on macOS). The plugin starts the watcher on `projectOpened` — no polling loop, no extra process.

**Fallback:** If the patch file isn't consumed within 2s (plugin not installed or not running), `agent-doc write --ipc` falls back to the current direct CRDT write. This makes `--ipc` safe to use unconditionally.

**Implemented:**

1. **Binary side** (`src/agent-doc/src/write.rs`): Added `run_ipc()` function
   - Reads response from stdin, parses patch blocks
   - Writes JSON to `.agent-doc/patches/<doc-hash>.json`
   - Polls for deletion (100ms x 20 = 2s timeout)
   - If consumed: reads current file, saves snapshot + CRDT state
   - If timeout: falls back to full `run_stream()` logic

2. **CLI** (`src/agent-doc/src/main.rs`): Added `--ipc` flag to `write` command

3. **Plugin** (`editors/jetbrains/.../PatchWatcher.kt`): New class
   - NIO `WatchService` on `.agent-doc/patches/` directory
   - `processPatchFile()`: reads JSON, applies patches via Document API
   - `applyComponentPatch()`: finds `<!-- agent:name -->` markers, replaces content
   - `WriteCommandAction.runWriteCommandAction`: holds document lock, batches undo, NO external file change dialog, preserves cursor
   - Hand-written JSON parser (no library dependency, consistent with plugin conventions)

4. **Lifecycle** (`PluginLifecycleListener.kt`): Wired `PatchWatcher.getInstance()` on `projectOpened`, `PatchWatcher.disposeProject()` on `projectClosed`

**All builds pass:**
- agent-doc: 303 tests, clippy clean
- JetBrains plugin: buildPlugin + signPlugin both succeed
- tmux-router: 67 tests

**To test:** Install the updated plugin zip, then use `--ipc` flag:
```bash
echo '<patch blocks>' | agent-doc write tasks/foo.md --baseline-file /tmp/baseline.md --ipc
```
The SKILL.md can be updated to pass `--ipc` instead of `--stream` for the write-back step, enabling seamless cursor-stable editing.

release
### Re: IPC documentation

`/existence new ipc` would create a term in the existence-lang ontology repo — that's for universal concepts. For agent-doc-scoped documentation, I created `docs/reference/ipc.md` directly, using existence-lang's Ontology/Axiology/Epistemology structure:

**Created:** `src/agent-doc/docs/reference/ipc.md`
- **Ontology:** IPC as a System bridging CLI and IDE Domains, preserving user Focus
- **Axiology:** Why file-based writes cause cursor jump, dialogs, and undo disruption
- **Epistemology:** Architecture diagram, sequence diagram, JSON format spec, fallback behavior, component mapping, pattern expression at IDE and CLI scopes
- Added to `docs/SUMMARY.md` under Reference

### Re: Release

Preparing v0.17.0 release. Changes since v0.16.1:

1. **format/write split** — `agent_doc_mode` → `agent_doc_format` + `agent_doc_write` (12 files, backward-compatible)
2. **tmux layout fix** — `first_pane_join_flag()` replaces hardcoded `-dbh` (67 tmux-router tests)
3. **IPC write** — `--ipc` flag + JetBrains PatchWatcher (cursor-stable editing)
4. **IPC docs** — `docs/reference/ipc.md`

Committing, bumping to v0.17.0, merging to main, tagging, and publishing now.
 
The plugins should have a version bump as well.
### Re: Plugin version bump

Bumped JetBrains plugin from v0.2.0 → **v0.2.1** (PatchWatcher IPC feature). VS Code stays at v0.2.0 (no changes).

**v0.17.0 release status:**

| Target | Status |
|--------|--------|
| GitHub Release | Done — binary + plugin zips attached |
| crates.io | **Auth error** (401) — needs `cargo login` re-auth |
| PyPI | **Auth error** (403) — needs token refresh |
| Binary installed | Done (v0.17.0 in PATH) |
| Skill updated | Done (v0.17.0) |
| Submodule pointers | Done (agent-doc + tmux-router) |
| JetBrains plugin | Built (v0.2.1) — install from disk to test IPC |

To fix publishing:
- **crates.io**: `cargo login` with a fresh token from https://crates.io/settings/tokens
- **PyPI**: `maturin publish` needs a fresh API token from https://pypi.org/manage/account/

agent-doc should have a transfer command to transfer context from one document to another.

### Transferred from tasks/sift.md — CRDT issues + plugin architecture

**CRDT issues identified (filed as GitHub issues):**
1. **Duplicate text after merge** ([#15](https://github.com/btakita/agent-doc/issues/15)) — When both user and agent produce identical text, CRDT merge keeps both copies. Fix: conservative post-merge dedup pass.
2. **Frontmatter not updated via CRDT** ([#16](https://github.com/btakita/agent-doc/issues/16)) — Frontmatter changes via Edit tool get clobbered by CRDT merge. Fix: add patch:frontmatter target to CRDT pipeline.

**Conservative dedup rule:** If two identical paragraph blocks appear adjacent after merge, and one came from "ours" and one from "theirs", collapse to one. Safe because keeping one copy of identical content loses nothing.

**Frontmatter via CRDT:** Add a special patch target that applies YAML key/value updates through the same write path as content patches. No more Edit tool for frontmatter.

**Thin plugin architecture (confirmed direction):**
- Plugins are triggers only — detect save/focus events, call shell commands
- agent-doc CLI is the controller — all merge, snapshot, patch, frontmatter, git logic
- Cross-editor interface: just shell commands (claim, submit, status)
- Works for JetBrains, VSCode, Neovim, Vim, Helix, Zed, Ki — any editor that runs shell commands on save

> - Patch application
> - Frontmatter management

I want to avoid the file externally changed dialog. So the plugin does need to handle updating the file so external change dialog does not pop up.

**Re: transfer command** — Yes, agent-doc should have a transfer command. Proposed:
```
agent-doc transfer <source> <target> [--lines START:END] [--component NAME]
```
Moves content (with proper snapshot/CRDT state updates) from one document to another. Could extract specific line ranges or named components. The transferred content gets appended to the target's exchange component.

The transfer that I ran from tasks/sift.md to tasks/agent-doc.md truncated the context way too much. It should basically keep all of the context for the relevant topic.
### Re: Transfer command + CRDT issues from sift.md

**Transfer command proposal:**

```
agent-doc transfer <source> <target> [--component NAME] [--lines START:END] [--topic HEADING]
```

The `--topic` flag would extract all content under a matching heading (including nested subheadings and the full discussion thread). This addresses the truncation issue — a topic-based extraction keeps all context for the relevant discussion rather than summarizing.

**Implementation approach:**
1. Parse source document to identify content boundaries (by heading, component, or line range)
2. Extract the full content block (preserving all user/agent exchanges, inline annotations, etc.)
3. Append to target document's exchange component (or specified component)
4. Update both documents' snapshots + CRDT state
5. Optionally mark the source content as transferred (without deleting — user decides cleanup)

**Regarding the truncated transfer from sift.md:** The full context is now preserved above in the transferred section. The key items:

- **CRDT dedup (#15)**: Post-merge pass to collapse identical adjacent blocks. Filed: [#15](https://github.com/btakita/agent-doc/issues/15)
- **Frontmatter via CRDT (#16)**: `patch:frontmatter` target for YAML key/value updates through the write path. Filed: [#16](https://github.com/btakita/agent-doc/issues/16)
- **Plugin = thin layer + IPC**: Already implemented in v0.2.1 — PatchWatcher applies changes via `WriteCommandAction` (no external file dialog). Plugin triggers events, CLI controls logic. Cross-editor interface via shell commands.
- **External change dialog**: Solved by IPC mechanism. Plugin watches `.agent-doc/patches/`, applies via Document API. No external file modification = no dialog, no cursor jump.

Adding `agent-doc transfer` to the pending list.

Do you have the following?

> - Patch application
> - Frontmatter management

I want to avoid the file externally changed dialog. So the plugin does need to handle updating the file so external change dialog does not pop up.

The plugin needs to handle the mechanics of the CRDT changes to gracefully integrate these updates with the editor process...avoiding externally changed file dialogs
### Re: Plugin patch application + frontmatter management

**Patch application: Yes, implemented.** `PatchWatcher.kt` handles this end-to-end:
The Patch implementation should be SPEC'd. Since each editor will have it's own implementation
We need a "Software Factory" to ensure that all of the plugins work according to spec.
Perhaps a reference implementation with a virtual editor?
- NIO WatchService on `.agent-doc/patches/`
- Reads JSON patch files written by `agent-doc write --ipc`
- `applyComponentPatch()` finds `<!-- agent:name -->` markers, replaces content
- All edits via `WriteCommandAction.runWriteCommandAction` — holds document write lock, batches undo, preserves cursor, NO external file change dialog
- Saves document via `FileDocumentManager.saveDocument()`, then deletes patch file (ACK)

**Frontmatter management: Not yet.** This is [#16](https://github.com/btakita/agent-doc/issues/16). Currently frontmatter changes go through the Edit tool (direct file write), which can trigger the external change dialog. Fix: add `patch:frontmatter` target so frontmatter updates flow through the same IPC pipeline as content patches.

**Summary of what avoids the dialog vs what doesn't:**

| Operation | Path | Dialog-free? |
|-----------|------|-------------|
| Content patches (exchange, status, etc.) | `--ipc` → PatchWatcher → Document API | Yes |
| Frontmatter updates | Edit tool → disk write | **No** (triggers dialog) |
| Snapshot/CRDT state | `.agent-doc/` files (not open in editor) | N/A |

Once #16 lands, ALL document modifications will flow through IPC — fully dialog-free. The plugin stays thin: it only handles the mechanics of applying changes through the IDE's Document API. The merge logic, CRDT state, snapshot management, and patch computation remain in the agent-doc CLI.

Implement fix for #16.
Discuss how we are going to spec editor plugins. Including a reference implementation using a virtual editor.
<!-- /agent:exchange -->
