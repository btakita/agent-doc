# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, call `agent-doc route <relative-path>` from the project root.
- **Feedback:** Inline hint near cursor (auto-dismissing). Error notifications persist.
- **Availability:** Only enabled when a `.md` file is active.

## 2. Claim for Tmux Pane

- **Trigger:** `Ctrl+Shift+Alt+C` (configurable)
- **Behavior:** Detect which editor split the file is in (left/right/top/bottom), call `agent-doc claim <relative-path> --position <pos>`. Falls back to no `--position` if split is not detected.
- **Feedback:** Inline hint near cursor. After claiming, trigger a layout sync (silent).

## 3. Sync Tmux Layout

- **Trigger:** `Ctrl+Shift+Alt+L` (configurable)
- **Behavior:** Collect all visible `.md` files, detect split orientation, call `agent-doc layout <files...> --split h|v` (or `agent-doc focus <file>` for single file).
- **Feedback:** Inline hint near cursor.

## 4. Tab-to-Pane Sync (Automatic)

- **Trigger:** Editor tab selection changes.
- **Behavior:** When the active `.md` file changes, call `agent-doc focus <file>`. When the visible file set changes, call `agent-doc layout`.
- **Debounce:** 500ms. Skip if file set unchanged. Concurrency guard (one command at a time).

## 5. Prompt Polling

- **Trigger:** After a Run action, poll `agent-doc prompt --all` every 1.5s.
- **Behavior:** Detect numbered-option permission prompts. Display a bottom-anchored panel with buttons for each option. Support keyboard selection (Alt+1..9, Alt+Esc toggle, Esc dismiss).
- **Answer:** Call `agent-doc prompt --answer <N> <file>` when user selects an option.
- **Auto-save:** Save tracked files before each poll cycle to capture user edits.

## 6. Popup Menu

- **Trigger:** `Alt+Enter` on a `.md` file.
- **Behavior:** Show numbered popup with Run, Claim, Sync Layout actions.

## 7. Notifications

- **Success:** Lightweight inline hint near cursor (auto-dismissing, ~1-2 seconds).
- **Error:** Persistent notification balloon. Errors never auto-dismiss.
- **No temp files:** All diagnostic logging uses the IDE's built-in logger, not file I/O.

## 8. File Filtering

- All actions are only enabled/visible when a `.md` file is active or selected.
- Non-`.md` files are ignored by tab sync and prompt polling.

## 9. Boundary Marker Management

Boundary markers (`<!-- agent:boundary:{id} -->`) are transient UI elements that mark the insertion point for agent responses in the exchange component. Plugins must maintain the following invariants:

**Invariants:**
- At most ONE boundary marker exists in the document at any time (outside of code blocks)
- The boundary is always at the END of the exchange component content
- Boundaries inside fenced code blocks are never touched

**Boundary ID format:** 8-character hex string (e.g., `a0cfeb34`). Plugins generating boundary markers must use 8-char hex IDs, not full UUIDs.

**Reposition behavior:** When the plugin receives a `reposition_boundary: true` IPC signal:
1. Remove ALL `<!-- agent:boundary:... -->` lines from the exchange component (not just the last one)
2. Insert a single boundary at the end of the exchange content.
   If the IPC payload includes `reposition_boundary_id` / `boundary_id`, reuse that exact ID.
   Otherwise generate a fresh 8-char hex ID.
3. Do not introduce any transient ` (HEAD)` heading annotations during this cleanup
4. Skip boundary markers inside fenced code blocks

**Recommended implementation:** Call `agent_doc_reposition_boundary_to_end()` via FFI/JNA on the shared library (`libagent_doc.so` / `libagent_doc.dylib`). This ensures identical cleanup logic across all platforms and prevents divergence between plugin and binary behavior.

**When to reposition:**
- After applying an IPC patch (when `reposition_boundary` flag is set)
- After receiving a standalone reposition IPC signal (post-commit). Post-commit signals should carry the committed `boundary_id` so the editor normalizes back to `HEAD` instead of inventing a new local diff.

**Response heading visibility:** During normal IPC patch application (before post-commit cleanup), plugins must preserve transient ` (HEAD)` markers on newly added `### Re:` headings in `agent:exchange`. That marker is part of the user-visible "fresh response is still uncommitted" state; only the explicit clean reposition/commit path is allowed to strip it.

## 10. CLI Dependency

- Plugins resolve `agent-doc` from: `~/bin/`, `~/.local/bin/`, `~/.cargo/bin/`, `/usr/local/bin/`, or `$PATH`.
- All commands run from the project root directory.
- Plugins are thin wrappers — business logic lives in the CLI.
