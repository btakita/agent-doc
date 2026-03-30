# Versions

agent-doc is alpha software. Expect breaking changes between minor versions.

Use `BREAKING CHANGE:` prefix in version entries to flag incompatible changes.

## 0.28.1

- **Column memory**: `.agent-doc/last_layout.json` saves column→agent-doc mapping. When a column has no agent doc, sync substitutes the last known agent doc from the state file. Preserves 2 tmux panes when one column switches to a non-agent file.

## 0.28.0

- **Empty col_args filtering**: `sync` now filters out empty strings from `col_args` before processing. Fixes phantom empty columns sent by the JetBrains plugin during rapid editor split changes.
- **Sync debug logging**: Added `/tmp/agent-doc-sync.log` trace logging at key sync decision points (col_args, repair_layout, auto-start, pre/post tmux_router::sync pane counts).
- **Post-auto_start stash removed**: The explicit stash after auto-start is no longer needed — `tmux_router::sync` always runs the full reconcile path (no early exits), so excess panes are stashed during the DETACH phase.
- **tmux-router v0.3.6**: Early exits removed from `sync` — the full reconcile path now runs for 0, 1, or 2+ resolved panes uniformly. Previous early exits for `resolved < 2` bypassed the DETACH phase, leaving orphaned panes from previous layouts visible.
- **JetBrains plugin v0.2.36**: Filter empty columns in SyncLayoutAction.kt

## 0.27.9

- **tmux-router v0.3.5**: Updated dependency — trace logging at key sync decision points + early-exit stash removal (preserves previous-column panes)

## 0.27.8

- **tmux-router v0.3.4**: Updated dependency — early-exit stash now derives session from pane via `pane_session()` instead of dead `doc_tmux_session` path
- **VERSIONS.md backfill**: Added entries for v0.23.2 through v0.26.6

## 0.27.7

- **Sync path column-aware split**: `auto_start_no_wait` now accepts `col_args` and computes `split_before` via `is_first_column()`. Previously hardcoded `split_before = false`, causing new panes to always split alongside the rightmost pane regardless of column position. The sync path (editor tab switches) now matches the route path behavior.

## 0.27.6

- **Bold-text pseudo-header fallback for `(HEAD)` marker**: `add_head_marker()` in `git.rs` now falls back to bold-text lines (`**...**`) when no markdown headings are found in new content. `strip_head_markers()` also handles stripping `(HEAD)` from bold-text lines.
- **SKILL.md header format guidance**: Added "Response header format (template mode)" section instructing agents to use `### Re:` headers. Bold-text pseudo-headers are supported as a fallback but real headings are preferred for outline visibility and sub-section nesting.

## 0.27.5

- **Column-aware split target**: `auto_start_in_session` picks the split target based on column position — first pane (leftmost) for left-column files, last pane (rightmost) for right-column files. Fixes 3-pane layout bug where new panes split the wrong existing pane.
- **Early-exit stash**: Before the `resolved < 2` early return in `tmux-router::sync`, excess panes in the agent-doc window are now stashed. Previously, old panes from previous layouts stayed visible when only one file resolved.
- **tmux-router v0.3.3**: Published with the early-exit stash fix.

## 0.27.4

- **Rescue stashed panes in sync**: `sync.rs` now rescues stashed panes back to the agent-doc window via swap-pane/join-pane before falling back to auto-start. Preserves Claude session context across editor tab switches.

## 0.27.3

- **Revert auto-kill**: Reverts v0.27.2 auto-kill of idle stashed Claude sessions. The `❯` prompt is the normal state of a stashed session waiting to be rescued — not an orphan indicator.

## 0.27.2

- **Auto-kill idle stashed Claude sessions**: Added auto-cleanup in `return_stashed_panes_bulk()` for stashed panes running agent-doc/claude at the `❯` prompt with no return target. (Reverted in v0.27.3 — too aggressive, killed active sessions.)

## 0.27.1

- **Fix "externally modified" popup**: Removed stale boundary disk write that caused spurious file modification notifications in editors.

## 0.27.0

- **Fix stash rescue deregistration**: Fixed pane deregistration during stash rescue operations.
- **Socket IPC**: Added `ipc_socket` module using Unix domain sockets via the `interprocess` crate for direct binary-to-plugin communication.
- **Bulk resync**: `return_stashed_panes_bulk()` for batch stash rescue operations.

## 0.26.6

- **FFI sync lock/debounce**: Added `agent_doc_sync_try_lock`/`unlock` FFI exports for cross-editor concurrency control. Added `agent_doc_sync_bump`/`check_generation` for cross-editor event coalescing.
- **Layout debounce fix**: `LayoutChangeDetector` uses generation counter instead of spawning concurrent threads per event.
- **JetBrains plugin v0.2.35**: Uses FFI sync primitives with local fallback.

## 0.26.5

- **Skip no-op IPC reposition**: IPC reposition signal skipped when boundary position is unchanged, eliminating ~64% of no-op PatchWatcher operations.
- **Handle inotify overflow**: PatchWatcher scans for missed files on inotify OVERFLOW events.
- **CI: crates.io-only dependencies**: All path dependencies (instruction-files, tmux-router, agent-kit, module-harness, existence) replaced with crates.io versions in CI workflows.

## 0.26.4

- **Prompt detection for Claude Code v2.1+**: Support numbered list format (`N. label`) in prompt option parsing alongside bracket format (`[N] label`).
- **Auto-start PromptPoller**: Plugin auto-starts PromptPoller on project open.
- **JetBrains plugin v0.2.32**: PromptPoller auto-start, `.bin/` path resolution, diagnostic logging.

## 0.26.3

- **Sync no longer auto-inits frontmatter**: Sync returns `Unmanaged` for files without session UUIDs; only `claim` adds frontmatter now.
- **Plugin mixed-layout sync**: Uses focus-only when non-`.md` files are in editor splits, preventing stashing.
- **JetBrains plugin v0.2.25**: Alt+Space popup, removed ActionPromoter (frees Alt+Enter for native JetBrains intentions).

## 0.26.2

- **Route single exit point**: Refactored route to `resolve_or_create_pane()` eliminating propagation bugs. `sync_after_claim` now runs on ALL route paths.
- **Response status signals**: File-based status signals (`.agent-doc/status/<hash>`) for cross-process visibility. FFI: `set_status`/`get_status`/`is_busy` for in-process plugin checks.
- **Auto-init unclaimed files in sync**: Sync writes session UUID for unclaimed files.
- **`agent_doc_version()` FFI export**: Runtime version tracking for plugins.
- **JetBrains plugin v0.2.24**: `is_busy()` guard in `EditorTabSyncListener` + `TerminalUtil`.

## 0.26.1

- **Sync layout authority**: `sync_after_claim` uses editor-provided `col_args`, preventing 3-pane layout regression on file switch.
- **Clippy fixes**: `doc_lazy_continuation` fixes in sync.rs, upgrade.rs. Unused variable fix in tmux-router `break_pane_to_stash`.
- **SPEC.md updates**: Added sections on project config, IPC write verification, and sync layout authority.

## 0.26.0

- **Kill pane safety**: `kill_pane` refuses to destroy a session's last window (tmux-router v0.3.0).
- **IPC verification**: Content verification catches partial plugin application failures. `--force-disk` cleans stale patches to prevent double-writes.
- **Module harness context**: All 53+ modules annotated with Spec/Contracts/Evals doc comments (468 named evals, 68% coverage).
- **Existence-lang ontology**: 9 domain terms defined (Document, Session, Component, Boundary, Snapshot, Patch, Exchange, Route, Claim). Dev dependencies: existence v0.4.0, module-harness v0.2.0.
- **README rewrite**: Concise GitHub-facing guide.

## 0.25.15

- **Sync layout repair**: Added `repair_layout()` to fix window index mismatches (agent-doc window not at index 0). Sync tests added for repair skip and move scenarios.
- **Blank line collapse on tmux_session strip**: Collapsing 3+ consecutive newlines to 2 when stripping deprecated `tmux_session` frontmatter field.

## 0.25.14

- **Sync pane repair**: Window index repair, pane state reconciliation, effective window tracking.
- **Resync enhancements**: Enhanced dead pane detection and session validation.
- **Route improvements**: Improved command routing logic.

## 0.25.13

- **Install script**: Rewritten `install.sh` with platform detection and improved install paths.
- **Homebrew formula**: Added `Formula/agent-doc.rb` for macOS/Linux Homebrew installation.
- **Deprecate `tmux_session` frontmatter**: Sync strips the field on encounter instead of repairing it. Route `auto_start` no longer attempts repair.

## 0.25.12

- **Sync swap-pane atomic reconcile**: `context_session` overrides frontmatter `tmux_session`, auto-repairs on mismatch.
- **Visible-window split**: New panes split in the visible agent-doc window instead of stash.
- **Resync report-only in sync**: `resync --fix` disabled in sync path to preserve cross-session panes.
- **tmux-router v0.2.9**: Swap-pane atomic transitions.

## 0.25.11

- **Tmux-router swap-pane atomic transitions**: Pane moves use `swap-pane` for flicker-free layout changes. CI fix for path dependencies (agent-kit, tmux-router).

## 0.25.10

- **Preflight mtime debounce**: 500ms idle gate before computing diff.
- **Unified diff context**: Diff output uses unified format with 5-line context radius.
- **Route `--debounce` flag**: Opt-in mtime polling for coalescing rapid editor triggers.
- **`is_tracked` FFI export**: For editor plugins to check file tracking status.
- **Sync no-wait auto-start**: `auto_start_no_wait` for non-blocking session creation during sync.
- **JetBrains plugin v0.2.21**: Sync logging improvements.

## 0.25.9

- **`is_tracked()` FFI export**: Conservative debounce on untracked files (fallback to local tracking).
- **Untracked file debounce fix**: Untracked files no longer bypass debounce.
- **JetBrains plugin v0.2.20**: `is_tracked` binding + FFI logging tags.

## 0.25.8

- **Preflight debounce**: Mtime-based 500ms idle gate before computing diff.
- **Unified diff context**: Switch diff output to unified format with 5-line context radius.
- **Route `--debounce`**: New flag for opt-in mtime polling to coalesce rapid editor triggers.
- **Truncation detection fix**: Smarter dot handling for domain fragments in `looks_truncated`.

## 0.25.7

- **Rename `submit` to `run`**: `submit.rs` renamed to `run.rs`; all internal "submit" terminology updated to "run".
- **FFI debounce module**: `document_changed()` + `await_idle()` FFI exports for editor-side debounce.
- **Route sync fix**: Route calls `sync::run_layout_only()` to prevent auto-start race conditions.
- **JetBrains plugin v0.2.19**: FFI debounce, conditional typing wait, layout-only sync.

## 0.25.6

- **Route `--col`/`--focus` args**: Declarative layout sync from the route command. Plugin `sendToTerminal` passes editor layout in a single CLI call.
- **Layout change detection**: `LayoutChangeDetector` using `ContainerListener` with 5s fallback poll in the JetBrains plugin.
- **EDT-safe threading**: Plugin uses `invokeLater` for Swing reads, background thread for CLI calls.
- **JetBrains plugin v0.2.17**.

## 0.25.5

- **FFI boundary reposition**: Export `agent_doc_reposition_boundary_to_end()` for plugin use.
- **Boundary ID summaries**: 8-char hex IDs with optional `:summary` suffix (filename stem). `new_boundary_id_with_summary()` wired into all write paths.
- **Snapshot boundary cleanup**: Commit path uses `remove_all_boundaries()`. Working tree cleaned via `clean_stale_boundaries_in_working_tree()` on commit.
- **JetBrains plugin v0.2.14**: FFI-first reposition with Kotlin fallback.

## 0.25.4

- **Boundary accumulation fix**: Plugin `repositionBoundaryToEnd` removes ALL boundaries, not just the last one.
- **Short boundary IDs**: 8 hex chars instead of full UUID (centralized in `lib.rs`).
- **Autoclaim pruning**: Validate file existence, prune stale entries on rename/delete.
- **Sync stale pane detection**: Detect alive panes with non-existent registered files (rename), kill stale pane and auto-start new session.

## 0.25.3

- **Fix IPC boundary reposition for prompt ordering**: All IPC write paths call `reposition_boundary_to_end()` before extracting boundary IDs. Previously the stale boundary position caused responses to appear before the prompt.

## 0.25.2

- **Fix skill install superproject root resolution**: Added `resolve_root()` to detect git superproject when CWD is in a submodule. `skill install`/`check` now writes to the project root, not the submodule's `.claude/skills/`.

## 0.25.1

- **IPC boundary reposition from commit**: After committing, send an IPC reposition signal to the plugin so it moves the boundary marker to end-of-exchange in its Document buffer. Avoids writing to the working tree (which would lose user keystrokes).

## 0.25.0

- **`agent-doc preflight` command**: Consolidated pre-agent command (recover + commit + claims + diff + document read) returning JSON for skill consumption.
- **Boundary reposition fix**: Snapshot-only reposition prevents losing user input; no working tree writes during reposition.
- **CRDT merge simplification**: Removed `reorder_agent_before_human()`, deterministic client IDs.
- **Pulldown-cmark outline**: CommonMark-compliant heading parser for outline.
- **Plugin boundary reposition via IPC**: `reposition_boundary: true` flag in IPC payloads.
- **Stash window routing**: Target largest pane, overflow to stash windows.
- **JetBrains plugin v0.2.12**: Plugin-side boundary reposition.

## 0.24.4

- **Deterministic boundary re-insertion in `apply_patches`**: Binary handles boundary re-insertion after checkpoint writes, removing the need for SKILL.md to manually re-insert boundaries.

## 0.24.3

- **Context session for auto_start**: Pass context session to `auto_start` to prevent routing to the wrong tmux session. Post-sync resync for consistency.

## 0.24.2

- **SKILL.md step 3b**: Added mandatory pending updates check each cycle.
- **`plugin install --local`**: Install JetBrains/VS Code plugins from local build directory.
- **JetBrains plugin v0.2.10**: `resync --fix` on startup.
- **JetBrains plugin v0.2.9**: VCS refresh signal fix (ENTRY_MODIFY event).

## 0.24.1

- **SKILL.md heredoc examples**: Updated bundled SKILL.md with heredoc examples for the write command.

## 0.24.0

- **`agent-doc install` command**: System-level setup that checks prerequisites (tmux, claude) and detects/installs editor plugins.
- **`agent-doc init` project mode**: No-arg `init` now initializes a project (creates `.agent-doc/` directory structure, installs SKILL.md) instead of requiring a file argument.
- **SKILL.md content tests**: CLI integration tests for skill install/check content verification.
- **Sync pane guard**: Pre-sync alive pane check prevents duplicate session creation.

## 0.23.3

- **Cross-platform sync pane guard**: `find_alive_pane_for_file()` uses `ps(1)` instead of `/proc` for Linux+macOS compatibility. Pre-sync auto-start checks alive panes before creating duplicates.
- **Clippy fixes**: Fix `collapsible_if` warnings in template.rs, git.rs, terminal.rs. Suppress `dead_code` warnings for library-only boundary functions.

## 0.23.2

- **Explicit patch boundary-aware insertion**: `apply_patches_with_overrides()` checks for boundary markers when applying explicit patch blocks in append mode, not just unmatched content. Prevents boundary markers from accumulating as orphans.
- **Version bump**: Includes all v0.23.1 fixes (IPC snapshot, HEAD marker cleanup, boundary insertion).

## 0.23.1

- **Boundary-aware insertion for unmatched content**: `apply_patches_with_overrides()` now uses boundary-aware insertion for both explicit append-mode patches and unmatched content routed to `exchange`/`output`. Previously only explicit patches used boundary markers; unmatched content used plain append.
- **IPC snapshot correctness**: `try_ipc()` now accepts a `content_ours` parameter (baseline + response, without user concurrent edits). On IPC success the snapshot is saved from `content_ours` instead of re-reading the current file, preventing user edits typed after the boundary from being absorbed into the snapshot.
- **IPC synthesized exchange patch**: When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware component patch so the plugin inserts at the correct position.
- **`boundary.insert()` cleans stale markers**: Before inserting a new boundary marker, `insert()` strips all existing boundary markers from the document. Prevents orphaned markers accumulating across interrupted sessions.
- **`boundary::find_boundary_id_in_component()`**: New public function. Scans a pre-parsed `Component` for any boundary marker UUID, skipping matches inside code blocks. Used by `template.rs` and external callers without re-parsing components.
- **Post-commit working tree cleanup**: After `git.commit()` succeeds, `strip_head_markers()` is applied to both the snapshot and the working tree file. Ensures `(HEAD)` markers never appear in the editor — they exist only in the committed version (creating the blue gutter diff).

## 0.23.0

- **Boundary marker for response ordering**: New `agent-doc boundary <FILE>` command inserts `<!-- agent:boundary:UUID -->` at the end of append-mode component content. The marker acts as a physical anchor — responses are inserted at the marker position, ensuring correct ordering when the user types while a response is being generated. Replaces the fragile caret-offset approach.
- **Boundary-aware FFI**: New `agent_doc_apply_patch_with_boundary()` C ABI export. JetBrains plugin (`NativeLib.kt`, `PatchWatcher.kt`) uses boundary markers with priority over caret-aware insertion.
- **Component parser: boundary marker exclusion**: `<!-- agent:boundary:* -->` comments are now skipped by the component parser (no longer cause "invalid component name" errors).
- **IPC boundary_id**: All IPC patch JSON payloads include `boundary_id` when a boundary marker is present in the target component.
- **SKILL.md: boundary marker step**: Updated bundled SKILL.md to call `agent-doc boundary <FILE>` after reading the document (step 1b).
- **Claim auto-start**: JetBrains plugin "Claim for Tmux Pane" action now auto-starts the agent session after successful claim.
- **JetBrains plugin v0.2.8**: Boundary-aware patching + claim auto-start.

## 0.22.2

- **SKILL.md: immediate commit after write**: Updated bundled SKILL.md to call `agent-doc commit` right after `agent-doc write`, replacing the old "Do NOT commit after writing" instruction. All sessions get the new behavior after `agent-doc skill install`.
- **Plugin default modes**: `exchange` and `findings` components now default to `append` mode in the JetBrains plugin (matching the Rust binary's `default_mode()`), so `<!-- agent:exchange -->` works without explicit `patch=append`.

## 0.22.1

- **Any-level HEAD markers**: `(HEAD)` marker now matches any heading level (`#`–`######`), not just `###`. Only root-level (shallowest) headings in the agent's appended content are marked.
- **Multi-heading markers**: When the agent response has multiple sections, ALL new root headings get `(HEAD)` markers (comparing snapshot vs git HEAD).
- **VCS refresh signal**: After `agent-doc commit`, writes `vcs-refresh.signal` to `.agent-doc/patches/`. Plugin watches for this and triggers `VcsDirtyScopeManager.markEverythingDirty()` + VFS refresh so git gutter updates immediately.
- **JetBrains plugin v0.2.7**: VCS refresh signal handling, cursor-aware FFI, VFS refresh before dirty scope.

## 0.22.0

- **`agent-doc terminal` subcommand**: Cross-platform terminal launch from editor plugins. Config-first (no hard-coded terminal list): `[terminal] command` in `config.toml` with `{tmux_command}` placeholder. Fallback to `$TERMINAL` env var. Detects stale frontmatter sessions and scans registry for live panes.
- **Selective commit**: `agent-doc commit` stages only the snapshot content via `git hash-object` + `git update-index`, leaving user edits in the working tree as uncommitted. Agent response → committed (no gutter). User input → uncommitted (green gutter).
- **HEAD marker**: Committed version of the last `### ` heading gets ` (HEAD)` suffix, creating a single modified-line gutter as a visual boundary and navigation point.
- **First-submit snapshot fix**: When no snapshot exists and git HEAD content matches the current file, treat as first submit (entire file is the diff) instead of "no changes detected".
- **Cursor-aware FFI**: `agent_doc_apply_patch_with_caret()` in shared library — inserts append-mode patches before the cursor position. `Component::append_with_caret()` in `component.rs`. JNA binding in `NativeLib.kt`.
- **JetBrains plugin v0.2.7**: Cursor-aware append ordering via native FFI with Kotlin fallback. Captures caret offset from `TextEditor` before `WriteCommandAction`.

## 0.21.0

- **`agent-doc parallel` subcommand**: Fan-out parallel Claude sessions across isolated git worktrees. Each subtask gets its own worktree and tmux pane. Results collected as markdown with diffs. `--no-worktree` for read-only tasks.
- **CRDT post-merge reorder**: Agent content ordered before human content at append boundary using Yrs per-character attribution (`Text::diff` with `YChange::identity`).
- **README**: Added parallel fan-out documentation section.

## 0.20.3

- **`agent-doc claims` subcommand**: Read, print, and truncate `.agent-doc/claims.log` in a single binary call. Replaces the shell one-liner (`cat + truncate`) that was prone to zombie process accumulation when the Bash tool auto-backgrounded it.

## 0.20.2

- **Fix: numeric session name ambiguity** (tmux-router v0.2.8): `new_window()` now appends `:` to session name (`-t "0:"` instead of `-t "0"`). Without the colon, tmux interprets numeric names as window indices, creating windows in the wrong session. Root cause of persistent session 1 bleedover bug.

## 0.20.1

- **Session affinity enforcement**: Route and auto_start bail with error instead of falling back to `current_tmux_session()` when `tmux_session` is set in frontmatter. Prevents pane creation in wrong tmux session.

## 0.20.0

- **CRDT conservative dedup** (#15): Post-merge pass removes identical adjacent text blocks.
- **CRDT frontmatter patches** (#16): `patch:frontmatter` now applied on disk write path (was IPC-only).
- **Binary-vs-agent responsibility** documented in CLAUDE.md.

## 0.19.0

- **ExecutionMode in config.toml**: `execution_mode = "hybrid|parallel|sequential"` in global config.
- **TmuxBatch**: Command batching in tmux-router v0.2.7 — reduces flicker via `\;` separator. `select_pane()` uses batch (2 → 1 invocation).

## 0.18.1

- **Revert Gson**: Hand-written JSON parser restored in JetBrains plugin (Gson causes ClassNotFoundException).
- **H2 scaffolding**: `claim` scaffolds h2 headers before components for IDE code folding.
- **SKILL.md**: Canonical pattern documented — h2 header before every component.

## 0.18.0

- **`agent-doc undo`**: Restore document to pre-response state (one-deep).
- **`agent-doc extract`**: Move last exchange entry between documents.
- **`agent-doc transfer`**: Move entire component content between documents.
- **Pre-response snapshots**: Saved before every write for undo support.

## 0.17.30

- **Immutable session binding**: `claim` refuses to overwrite `tmux_session` unless `--force`. Prevents cross-session pane swapping.

## 0.17.29

- **JNA FFI integration**: `NativeLib.kt` JNA bindings for JetBrains plugin with Kotlin fallback.
- **`agent_doc_merge_frontmatter()`**: New FFI export for frontmatter patching.
- **`agent-doc lib-path`**: Print path to shared library for plugin discovery.
- **VS Code prepend mode**: Fixed missing `prepend` case in `applyComponentPatch()`.

## 0.17.28

- **Validate tmux_session before routing**: Guard against routing to a non-existent tmux session.

## 0.17.27

- **Plugin code-block fix**: JetBrains and VS Code plugins skip component tags inside fenced code blocks. JB plugin 0.2.4, VSCode 0.2.2.

## 0.17.26

- **PLUGIN-SPEC docs update**: Document recent plugin features in PLUGIN-SPEC.

## 0.17.25

- **Stash else-branch fix**: Fix else-branch stash logic. Use `diff --wait` for truncation detection.

## 0.17.24

- **Pulldown-cmark for code range detection**: Replace hand-rolled code span/fence parser with `pulldown-cmark` in component parser. Stash overflow panes instead of creating new windows.

## 0.17.23

- **Stash overflow fix**: Overflow panes stashed instead of creating new tmux windows.

## 0.17.22

- **UTF-8 corruption fix**: Sanitize component tags in response content before writing to prevent UTF-8 corruption in `sanitize_component_tags`.

## 0.17.21

- **Indented fenced code blocks**: Component parser skips markers inside indented fenced code blocks. Scaffold `agent:pending` in claim for template documents.

## 0.17.20

- **BREAKING CHANGE: Rename `mode` to `patch`** for inline component attributes (`patch=append|replace`). `mode=` accepted as backward-compatible alias.

## 0.17.19

- **Split-window in auto_start**: Use `split-window` instead of `new-window` for auto-started Claude sessions. Resync tests added.

## 0.17.18

- **Resync `--fix` enhancements**: Detect wrong-session panes and wrong-process registrations. Renamed `--dangerously-set-permissions` to `--dangerously-skip-permissions`.

## 0.17.17

- **Parse fix**: `parse_option_line` matches `[N]` bracket format only. Fix `find_registered_pane_in_session` lookup.

## 0.17.16

- **Cursor editor support**: Add Cursor as a supported editor. `claude_args` frontmatter field for custom CLI arguments. Tmux session routing fix. VS Code extension bumped to v0.2.1.

## 0.17.15

- **Route/sync improvements**: Routing and sync refinements for multi-session workflows.

## 0.17.14

- **Plugin IPC fix**: VS Code IPC parity with JetBrains. History command improvements. Documentation updates.

## 0.17.13

- **Fix exchange append mode**: Remove hardcoded replace override in `run_stream`, allowing exchange component to use its configured patch mode.

## 0.17.12

- **Inline component attributes**: `<!-- agent:name mode=append -->` — patch mode configurable directly on the component tag.

## 0.17.11

- **History command**: `agent-doc history` shows exchange version history from git with restore support. IPC-priority writes with `--force-disk` flag to bypass.

## 0.17.10

- **Default component scaffolding**: Auto-scaffold missing components on claim. Append-mode exchange default. Route flash notification via `tmux display-message`.

## 0.17.9

- **Fix CRDT character interleaving**: Switch to line-level diffs to prevent character-level interleaving artifacts.

## 0.17.8

- **Template parser code block awareness**: Component markers inside fenced code blocks are now skipped by the template parser.

## 0.17.7

- **Fix CWD drift**: Recover and claim commands no longer drift from the project root working directory.

## 0.17.6

- **Documentation update**: Align docs with IPC-first write architecture from v0.17.5.

## 0.17.5

- **IPC-first writes**: All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin via `.agent-doc/patches/` before falling back to disk. Exit code 75 on IPC timeout.

## 0.17.4

- **Tmux pane orientation fix**: Arrange files side-by-side (horizontal split) instead of stacking vertically.

## 0.17.3

- **Fix CRDT character-level interleaving bug**: Resolve text corruption caused by character-level merge conflicts in CRDT state.

## 0.17.2

- **Fix CRDT shared prefix duplication bug**: Prevent duplicate content when CRDT documents share a common prefix.

## 0.17.1

- **Fix stream snapshot**: Use replace mode for exchange component in stream snapshot writes.

## 0.17.0

- **BREAKING CHANGE: `agent_doc_format`/`agent_doc_write` split**: Replace `agent_doc_mode` with separate format (`inline`|`template`) and write strategy (`disk`|`crdt`) fields. IPC write path for IDE plugins. Layout fix.

## 0.16.1

- **Native compact for template/stream mode**: `agent-doc compact` now works natively with template and stream mode documents.

## 0.16.0

- **Reactive stream mode**: CRDT-mode documents get zero-debounce reactive file-watching from the watch daemon. Truncation detection and CRDT stale base fix.

## 0.15.1

- **Patch release**: Version bump and minor fixes.

## 0.15.0

- **CRDT-based stream mode**: Real-time streaming output with CRDT conflict-free merge (`agent-doc stream`). Chain-of-thought support with optional `thinking_target` routing. Deferred commit workflow. Snapshot resolution prefers snapshot file over git.

## 0.14.9

- **Multi-backtick code span support**: `find_code_ranges` handles multi-backtick code spans (e.g., ` `` ` and ` ``` `).

## 0.14.8

- **Code-range awareness for strip_comments**: Fix `<!-- -->` stripping inside code spans and fenced blocks. Stash window purge for orphaned idle shells.

## 0.14.7

- **Bidirectional convert**: `agent-doc convert` works in both directions (inline <-> template). Autoclaim sync improvements.

## 0.14.6

- **Auto-sync on lazy claim**: Automatically sync tmux layout after lazy claim in route. Plugin autocomplete fixes for JetBrains.

## 0.14.5

- **`agent-doc commands` subcommand**: List available commands. Plugin autocomplete for JetBrains/VS Code. Remove auto-prune (moved to resync). Purge orphaned claude/stash tmux windows in resync.

## 0.14.4

- **Claim pane focus**: Focus the claimed pane after `agent-doc claim`. `convert` handles documents with pre-set template mode.

## 0.14.3

- **Autoclaim pane refresh**: Refresh pane info during autoclaim. Template missing-component recovery on write.

## 0.14.2

- **Skill reload via `--reload` flag**: Compact and restart skill installation in a single command.

## 0.14.1

- **SKILL.md workflow fix**: Move git commit to after write step in the skill workflow to prevent committing stale content.

## 0.14.0

- **Route focus fix + claim defaults to template mode**: New documents claimed via `agent-doc claim` default to template format. `agent-doc mode` CLI command for inspecting/changing document mode.

## 0.13.3

- **Bump tmux-router to v0.2.4**: Fix spare pane handling in tmux-router dependency.

## 0.13.2

- **Sync registers claims**: `agent-doc sync` registers claims for previously unregistered files in the layout.

## 0.13.1

- **Sync updates registry file paths**: Fix autoclaim file path tracking when sync moves files between panes.

## 0.13.0

- **Autoclaim + git-based snapshot fallback**: Automatic claim on route when no claim exists. Fall back to git for snapshot when snapshot file is missing.

## 0.12.2

- **Exchange component defaults to append mode**: The `exchange` component uses append patch mode by default instead of replace.

## 0.12.1

- **Lazy claim fallback**: `agent-doc claim` without `--pane` falls back to the active tmux pane.

## 0.12.0

- **`agent-doc convert` command**: Convert between inline and template document formats. Lazy claim support. `agent-doc compact` for git history squashing. Exchange component as default template target.

## 0.11.2

- **Strip trailing `## User` heading**: Also strip trailing `## User` heading from agent responses (complement to v0.11.1).

## 0.11.1

- **Strip duplicate `## Assistant` heading**: Remove duplicate `## Assistant` heading from agent responses when already present in the document.

## 0.11.0

- **Append-friendly merge strategy**: Improved 3-way merge strategy optimized for append-style document workflows.

## 0.10.1

- **Bundle template-mode instructions in SKILL.md**: SKILL.md now includes template-mode workflow instructions for the Claude Code skill.

## 0.10.0

- **BREAKING CHANGE: Rename `response_mode` to `agent_doc_mode`**: Frontmatter field renamed with backward-compatible aliases.

## 0.9.10

- **Code-span parser fix**: Component parser skips markers inside fenced code blocks and inline backticks. Template input/output component support.

## 0.9.9

- **Template mode + compaction recovery**: New template mode for in-place response documents using `<!-- agent:name -->` components. Durable pending response store for crash recovery during compaction.

## 0.9.8

- **Relocate advisory locks**: Move document advisory locks from project root to `.agent-doc/locks/`.

## 0.9.7

- **`agent-doc write` command**: Atomic response write-back command for use by the Claude Code skill.

## 0.9.6

- **Race condition mitigations**: Stale snapshot recovery, atomic file writes, and various race condition fixes.

## 0.9.5

- **Advisory file locking**: Lock the session registry during writes. Stale claim auto-pruning.

## 0.9.4

- **Bump tmux-router to v0.2**: Update tmux-router dependency.

## 0.9.3

- **Bump tmux-router to v0.1.3**: Fix stash window handling in tmux-router.

## 0.9.2

- **`agent-doc plugin install` CLI**: Install editor plugins from GitHub Releases. VS Code extension reaches feature parity with JetBrains.

## 0.9.1

- **Stash window resize fix**: Bump tmux-router to v0.1.2 to fix stash window resize issues.

## 0.9.0

- **Dashboard-as-document**: Component-based documents with `<!-- agent:name -->` markers, `agent-doc patch` for programmatic updates, `agent-doc watch` daemon for auto-submit on file change.

## 0.8.1

- **Auto-prune registry**: Prune dead session entries before route/sync/claim operations.

## 0.8.0

- **Tmux-router integration**: Wire `tmux-router` as a dependency for pane management. Fix `route` auto_start bug.

## 0.7.2

- **Attach-first reconciliation**: Sync uses attach-first strategy with auto-register for untracked panes. Column-positional focus. Tmux session affinity.

## 0.7.1

- **Additive reconciliation**: Convergent reconciliation loop (max 3 attempts) with deferred eviction and reorder phase. Nuclear rebuild fallback.

## 0.7.0

- **Snapshot-diff sync architecture**: Rewrite sync to use snapshot-based diffing for tmux layout reconciliation. Dead window handling and column inversion fix.

## 0.6.6

- **`--focus` on sync**: `agent-doc sync` accepts `--focus` flag. Inline hint notification at cursor position in JetBrains plugin.

## 0.6.5

- **Always use `sync --col`**: Single-file sync uses column mode. Break out unwanted panes. Plugin notification balloon for detected layout.

## 0.6.4

- **Sync window filtering + layout equalization**: Filter sync to target window only. Equalize pane sizes after layout.

## 0.6.3

- **LayoutDetector fix**: Skip non-splitter Container children in JetBrains plugin 3-column layout detection.

## 0.6.2

- **Fire-and-forget Junie bridge**: Junie bridge script resolved automatically. Plugin clipboard handoff for non-tmux editors.

## 0.6.1

- **Junie agent backend**: Add Junie as an agent backend with JetBrains plugin action support.

## 0.6.0

- **`agent-doc sync` command**: 2D columnar tmux layout synced to editor split arrangement. Dynamic pane groups.

## 0.5.6

- **Commit message includes doc name**: `agent-doc commit` message format now includes the document filename. `agent-doc outline` command for markdown section structure with token counts.

## 0.5.5

- **Window-scoped routing**: Route commands scoped to tmux window (not just session). `--pane`/`--window` flags. Layout safeguards. JetBrains plugin self-disabling Alt+Enter popup (removes ActionPromoter).

## 0.5.4

- **Positional claim**: `agent-doc claim <file>` accepts file as positional argument. Editor plugin improvements and SPEC updates.

## 0.5.3

- **Bundled SKILL.md with absolute snapshot paths**: Snapshot paths use absolute paths for reliability. Resync subcommand and claims log documentation.

## 0.5.2

- **Claim notifications + resync + plugin popup**: Notification on claim. `agent-doc resync` validates sessions.json and removes dead panes. JetBrains and VS Code editor plugins added.

## 0.5.1

- **Windows build fix**: Cfg-gate unix-only exec in `start.rs` for cross-platform compilation.

## 0.5.0

- **`agent-doc focus` and `agent-doc layout`**: Focus a tmux pane for a session document. Layout arranges tmux panes to mirror editor split arrangement.

## 0.4.4

- **Rename SPECS.md to SPEC.md**: Standardize specification filename.

## 0.4.3

- **Commit CWD fix**: Fix working directory for `agent-doc commit`. SKILL.md prohibition rules.

## 0.4.2

- **SPEC.md gaps filled**: Document comment stripping as skill-level behavior (§4), `--root DIR` flag for audit-docs (§7.6), `agent-doc-version` frontmatter field for auto-update detection (§7.12), and startup version check (`warn_if_outdated`).
- **Flaky test fix**: Skill tests no longer use `std::env::set_current_dir`. Refactored `install`/`check` to accept an explicit root path (`install_at`/`check_at`), eliminating CWD races in parallel test execution.
- **CLAUDE.md module layout updated**: Added `claim.rs`, `prompt.rs`, `skill.rs`, `upgrade.rs` to the documented module layout.

## 0.4.1

- **SKILL.md: comment stripping for diff**: Strip HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) before comparing snapshot vs current content. Comments are a user scratchpad and no longer trigger agent responses.
- **SKILL.md: auto-update check**: New `agent-doc-version` frontmatter field enables pre-flight version comparison. If the installed binary is newer, `agent-doc skill install` runs automatically before proceeding.
- **PromptPanel: JDialog to JLayeredPane overlay**: Replace `JDialog` popup with a `JLayeredPane` overlay in the JetBrains plugin, eliminating window-manager popup leaks.

## 0.4.0

- **`agent-doc claim <file>`**: New subcommand — claim a document for the current tmux pane. Reads session UUID from frontmatter + `$TMUX_PANE`, updates `sessions.json`. Last-call-wins semantics. Also invokable as `/agent-doc claim <file>` via the Claude Code skill.
- **`agent-doc skill install`**: Install the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. The skill content is embedded in the binary via `include_str!`, ensuring version sync.
- **`agent-doc skill check`**: Compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.
- **SKILL.md updated**: Fixed stale `$()` pattern → `agent-doc commit <FILE>`. Added `/agent-doc claim` support.
- **SPEC.md expanded**: Added §7.7–7.13 (all commands), §8 Session Routing with use case table (U1–U11), §8.3 Claim Semantics.

## 0.3.0

- **Multi-session prompt polling**: `agent-doc prompt --all` polls all live sessions in one call, returns JSON array. `SessionEntry` now includes a `file` field for document path (backward-compatible).
- **`agent-doc commit <file>`**: New subcommand — `git add -f` + commit with internally-generated timestamp. Replaces shell `$()` substitution in IDE/skill workflows.
- **Prompt detection**: `agent-doc prompt` subcommand added in v0.2.0 (unreleased).
- **send-keys fix**: Literal text (`-l`) + separate Enter, `new-window -a` append flag (unreleased since v0.2.0).

## 0.1.4

- **`agent-doc upgrade` self-update**: Downloads prebuilt binary from GitHub Releases as the primary upgrade strategy. Falls back to `cargo install`, then `pip install --upgrade`, then manual instructions including `curl | sh`.

## 0.1.3

- **Upgrade check**: Queries crates.io for latest version with a 24h cache. Prints a one-line stderr warning on startup if outdated.
- **`agent-doc upgrade`**: New subcommand tries `cargo install` then `pip install --upgrade`, or prints manual instructions.

## 0.1.2

- **Language-agnostic audit-docs**: Replace Cargo.toml-only root detection with 3-pass strategy (project markers → .git → CWD fallback). Scan 28 file extensions across 6 source dirs instead of .rs only.
- **--root CLI flag**: Override auto-detection of project root for audit-docs.
- **Test coverage**: Add unit tests for frontmatter, snapshot, and diff modules.

## 0.1.0

Initial release.

- **Interactive document sessions**: Edit a markdown document, run an AI agent, response appended back into the document.
- **Session continuity**: YAML frontmatter tracks session ID, agent backend, and model. Fork from current session on first run, resume on subsequent.
- **Diff-based runs**: Only changed content is sent as a diff, with the full document for context. Double-run guard via snapshots.
- **Merge-safe writes**: 3-way merge via `git merge-file` if the file is edited during agent response. Conflict markers written on merge failure.
- **Git integration**: Pre-commit user changes before agent call, leave agent response uncommitted for editor diff gutters. `-b` flag for auto-branch, `--no-git` to skip.
- **Agent backends**: Agent-agnostic core. Claude backend included. Custom backends configurable via `~/.config/agent-doc/config.toml`.
- **Commands**: `run`, `init`, `diff`, `reset`, `clean`, `audit-docs`.
- **Editor integration**: JetBrains External Tool, VS Code task, Vim/Neovim mapping.
