# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, call `agent-doc route --dispatch-only <relative-path>` from the project root.
- **Feedback:** Show an immediate in-flight info notification while `agent-doc route` is running, then finish with an inline hint near the cursor. Error notifications persist.
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

**Reposition behavior:** When the plugin receives a `reposition` IPC signal (or `reposition_boundary: true` in a patch payload):
1. Remove ALL `<!-- agent:boundary:... -->` lines from the exchange component (not just the last one)
2. Insert a single boundary at the end of the exchange content.
   If the IPC payload includes `reposition_boundary_id` / `boundary_id`, reuse that exact ID.
   Otherwise generate a fresh 8-char hex ID.
3. Check the `preserve_head` field in the IPC JSON (default `false`):
   - **`preserve_head: false`** (clean variant) — strip all transient ` (HEAD)` heading annotations during cleanup. Use `agent_doc_reposition_boundary_to_end()` / `_with_id()` FFI.
   - **`preserve_head: true`** — keep ` (HEAD)` annotations on `### Re:` headings so the user sees which responses are new. Use `agent_doc_reposition_boundary_to_end_preserve_head()` / `_with_id()` FFI.
4. Skip boundary markers inside fenced code blocks

**FFI variant families:**
| Variant | FFI function | Behavior |
|---------|-------------|----------|
| Clean | `agent_doc_reposition_boundary_to_end()` | Strips `(HEAD)`, collapses boundaries |
| Clean + ID | `agent_doc_reposition_boundary_to_end_with_id(doc, id)` | Same + reuses explicit boundary ID |
| Preserve | `agent_doc_reposition_boundary_to_end_preserve_head()` | Keeps `(HEAD)`, collapses boundaries |
| Preserve + ID | `agent_doc_reposition_boundary_to_end_preserve_head_with_id(doc, id)` | Same + reuses explicit boundary ID |

**Recommended implementation:** Call the appropriate FFI variant via JNA on the shared library (`libagent_doc.so` / `libagent_doc.dylib`). This ensures identical cleanup logic across all platforms and prevents divergence between plugin and binary behavior.

**When to reposition:**
- After applying an IPC patch (when `reposition_boundary` flag is set)
- After receiving a standalone reposition IPC signal (post-commit). Post-commit signals should carry the committed `boundary_id` so the editor normalizes back to `HEAD` instead of inventing a new local diff.

**Response heading visibility:** During normal IPC patch application (before post-commit cleanup), plugins must preserve transient ` (HEAD)` markers on newly added `### Re:` headings in `agent:exchange`. That marker is part of the user-visible "fresh response is still uncommitted" state; only the explicit clean reposition/commit path is allowed to strip it.

## 10. CLI Dependency

- Plugins resolve `agent-doc` from: `~/bin/`, `~/.local/bin/`, `~/.cargo/bin/`, `/usr/local/bin/`, or `$PATH`.
- All commands run from the project root directory.
- Plugins are thin wrappers — business logic lives in the CLI.

## 11. Visual Distinction

- Markdown editor integrations should visually distinguish agent-doc structures without requiring the user to switch document formats or install a separate grammar pack.
- Both JetBrains and VS Code must source their highlight ranges from the shared FFI surface (`agent_doc_visual_tokens_json`) so component markers, agent-managed component bodies, patch markers, boundary markers, `### Re:` headings, `❯` prompts, tracked `[#id]` tags, standalone bracket labels such as `[recommended]`, and ordinary HTML scratch comments plus their bodies stay in sync across editors.
- `agent_doc_visual_tokens_json` returns UTF-16 document offsets, not raw UTF-8 byte positions. Plugins must treat those offsets as editor-ready range endpoints and pass them directly to native document APIs.
- Matches inside fenced code blocks or inline code are excluded from this highlighting contract; example markup in code samples must remain untouched.
