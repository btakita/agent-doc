# Agent-Doc Editor Plugin Specification

Shared functional requirements for all agent-doc editor plugins (JetBrains, VSCode, and future editors).

## 1. Overview

Editor plugins are a **thin integration layer** between the agent-doc CLI binary and the editor's native APIs. All document logic, session management, and agent orchestration live in the `agent-doc` binary. Plugins handle only:

- Watching for IPC patch files and applying them via the editor's Document API
- Exposing user actions (submit, claim, sync) as keybindings/commands
- Polling for permission prompts and presenting them in the editor UI
- Reporting editor layout state to the CLI for tmux sync
- Providing slash-command autocomplete from CLI metadata

**Architecture principle:** If a feature requires document manipulation logic, it belongs in the Rust binary, not the plugin. Plugins must never parse markdown, resolve components, or make document-level decisions.

## 2. Required Features (Phase 1 -- Current)

### 2.1 IPC Patch Watcher

The patch watcher receives document updates from `agent-doc write --ipc` and applies them through the editor's native Document API. This preserves cursor position, undo stack, and avoids "file externally modified" dialogs.

**Startup:**
- Locate `.agent-doc/patches/` by walking up from workspace root. Create it if the `.agent-doc/` directory exists but `patches/` does not.
- Process any existing `*.json` files on startup (pending patches from before plugin load).
- Begin watching for new `*.json` files via filesystem watcher (NIO WatchService for JB, `FileSystemWatcher` for VSCode).

**Patch application flow:**
1. Read and parse patch JSON file.
2. Open the target document via editor API (`FileDocumentManager` for JB, `workspace.openTextDocument` for VSCode).
3. If `fullContent` is non-empty: replace entire document content (inline-mode documents).
4. Otherwise, apply structured patches:
   a. Apply `frontmatter` field: merge YAML key/value pairs into existing frontmatter block (`---\n...\n---\n`). Preserve key order; append new keys.
   b. Apply `patches[]` array: for each `{component, content}`, find the matching component markers and apply content according to mode.
   c. Apply `unmatched` content: try `exchange` component first, fall back to `output` component.
5. Write changes atomically via editor's write-command API (`WriteCommandAction` for JB, `WorkspaceEdit` for VSCode).
6. Save the document to disk.

**Component matching:**
- Open tag regex: `<!-- agent:NAME(\s[^>]*)? -->`  (supports inline attributes)
- Close tag: `<!-- /agent:NAME -->`
- Content region is everything between the open tag's end and the close tag's start.

**Mode resolution:**
- Parse `patch=<value>` (or `mode=<value>` as backward-compatible alias) from the open tag's inline attributes (the `(\s[^>]*)` capture group). `patch=` takes precedence if both are present.
- Supported modes:
  - `replace` (default): replace content region with `\n` + trimmed content + `\n`
  - `append`: preserve existing content, append `\n` + trimmed content + `\n` before close tag
  - `prepend`: insert `\n` + trimmed content before existing content (after open tag)

**ACK protocol:**
- On successful application: delete the patch JSON file. This signals to the CLI that the patch was consumed.
- On failure: leave the file in place and log a warning. The CLI will time out and exit with code 75 (`EX_TEMPFAIL`).

**File-not-found retry:**
- If the target file is not found in the editor's VFS, wait 200ms, refresh VFS, and try once more.
- If still not found, log warning and leave patch file for retry.

### 2.2 Claim Action

- **Trigger:** User action (default: `Ctrl+Shift+Alt+C`, configurable).
- **Precondition:** Active file is `.md`.
- **Behavior:**
  1. Detect editor split position (left/right/top/bottom) by walking the editor's splitter component tree.
  2. Run `agent-doc claim <relative-path> --position <pos>` via subprocess from project root. Include `--window <id>` if a tmux window ID is known from `sessions.json`.
  3. On success: show inline hint, then trigger a silent layout sync (Section 2.4).
  4. On failure: show persistent error notification.
- **Position detection:** Map the file's editor group to a split position. JetBrains uses Swing `Splitter` tree traversal; VSCode uses `TabGroups` API with `viewColumn` heuristic.

### 2.3 Submit Action (Route)

- **Trigger:** User action (default: `Ctrl+Shift+Alt+A`, configurable).
- **Precondition:** Active file is `.md`.
- **Behavior:**
  1. Save the active document to disk.
  2. Run `agent-doc route <relative-path>` via subprocess from project root.
  3. Show inline hint on success, persistent error notification on failure.
  4. Register the file for prompt polling (Section 2.6).
- **Concurrency guard:** Use an atomic boolean to prevent rapid double-invocation. Subsequent calls while a route is in-flight are silently skipped with a hint.

### 2.4 Layout Sync

- **Trigger:** User action (default: `Ctrl+Shift+Alt+L`, configurable) or automatic after claim.
- **Behavior:**
  1. Collect all visible `.md` files across editor split groups.
  2. Detect 2D columnar layout (which files are stacked vertically vs. side-by-side).
  3. Run `agent-doc sync --col <files,...> [--col <files,...>] --focus <active-file> [--window <id>]`.
  4. Show inline hint with layout summary on manual trigger. Silent on automatic trigger.

### 2.5 Tab-to-Pane Sync (Automatic)

- **Trigger:** Editor tab selection or visible editor set changes.
- **Debounce:** 500ms. Skip if the visible file set + active file signature is unchanged.
- **Concurrency guard:** One sync command at a time; skip if previous is still running.
- **Behavior:** Same as Section 2.4, but runs silently (no user notification). Errors are silently ignored.

### 2.6 Prompt Polling

- **Trigger:** Starts after a Submit action registers a file. Polls every 1.5 seconds.
- **Behavior:**
  1. Auto-save all tracked documents before each poll cycle.
  2. Run `agent-doc prompt --all` and parse JSON array response.
  3. Filter for entries with `active: true` and non-empty `options[]`.
  4. Show one prompt at a time. Stick with the current prompt until it resolves before advancing to the next.
  5. Display prompt UI with question text, option buttons, and keyboard hints.
- **Answer:** Run `agent-doc prompt --answer <index> <file>` when the user selects an option.
- **Dedup:** Track `answeredPromptKey` to suppress re-showing a prompt until the answer takes effect (prompt disappears from poll results).
- **UI requirements:**
  - JetBrains: Bottom-anchored `JPanel` overlay with `Alt+Esc` focus toggle, `Alt+1..9` direct selection, `1-9` selection when focused, `Esc` dismiss, auto-focus after 1s inactivity.
  - VSCode: `QuickPick` dialog with numbered options.

### 2.7 Popup Menu

- **Trigger:** `Alt+Enter` on a `.md` file.
- **Behavior:** Show a numbered action list including Submit, Claim, Sync Layout (and editor-specific extras like "Run with Junie" for JetBrains).

### 2.8 Slash Command Autocomplete

- **Trigger:** User types `/` at the start of a line in a `.md` file.
- **Behavior:**
  1. On first trigger, run `agent-doc commands` and cache the JSON array result.
  2. Provide completion items with `name`, `args` (detail), and `description`.
  3. Top-level commands (no spaces after `/`) sort above subcommands.
- **Cache:** Commands are loaded once per session and cached in memory.

### 2.9 CLI Resolution

Resolve the `agent-doc` binary path by checking these locations in order:
1. `~/bin/agent-doc`
2. `~/.local/bin/agent-doc`
3. `~/.cargo/bin/agent-doc`
4. `/usr/local/bin/agent-doc`
5. Fall back to bare `agent-doc` (rely on `$PATH`).

Cache the resolved path for the session lifetime.

### 2.10 Notifications

- **Success:** Lightweight inline hint near cursor, auto-dismissing after ~2 seconds.
- **Error:** Persistent notification (balloon/error message). Never auto-dismiss errors.
- **Logging:** Use the editor's built-in logging facility (`Logger` for JB, `OutputChannel` for VSCode). No temp files.

## 3. Future Features (Phase 2 -- CRDT-via-FFI)

### 3.1 CRDT Document Model

- Load `libagent_doc_crdt` shared library via FFI (JNI for JetBrains, napi/N-API for VSCode).
- Maintain an in-memory CRDT document (Yrs) per open session document.
- Capture user keystrokes in the editor buffer and convert them to CRDT operations.
- Receive agent CRDT state via IPC and merge with local document.
- Sync merged result to editor buffer atomically (single write-command action).
- Re-initialize CRDT from file on external edit detection.

### 3.2 State Synchronization

Three states must be reconciled:

| State | Location | Authority |
|-------|----------|-----------|
| **File** | Disk | Persisted state, used for snapshots and git |
| **Editor** | Buffer | User's live edits, may be unsaved |
| **CRDT** | Yrs (memory) | Authoritative during active sessions |

- CRDT is authoritative while a session is active.
- External edit (file changed outside editor+CRDT) triggers CRDT reset: re-initialize from disk content.
- Plugin crash recovery: re-initialize CRDT from file on restart.
- Session end: flush CRDT to disk, discard in-memory state.

## 4. IPC Protocol

### 4.1 Patch File Format

- **Path:** `.agent-doc/patches/<sha256_hash>.json`
- **Lifecycle:** CLI writes file, plugin reads + applies + deletes (ACK). CLI polls for deletion with a 2-second timeout.

**JSON schema:**

```json
{
  "file": "/absolute/path/to/document.md",
  "patches": [
    {
      "component": "exchange",
      "content": "New content for the component"
    }
  ],
  "unmatched": "Content that didn't match any component",
  "frontmatter": "key: value\nanother_key: value",
  "fullContent": "Complete document replacement (mutually exclusive with patches)"
}
```

- `file` (required): Absolute path to the target document.
- `patches` (required): Array of component-level patches. May be empty.
- `unmatched` (required): Content that didn't match a named component. Falls back to `exchange` then `output`.
- `frontmatter` (optional): YAML key/value pairs to merge into the document's frontmatter.
- `fullContent` (optional): If non-empty, replaces the entire document content. Used for inline-mode documents without component markers.

### 4.2 Future: CRDT State Exchange

- **Path:** `.agent-doc/crdt/<sha256_hash>.yrs`
- **Format:** Binary Yrs state vector encoding.
- **Lifecycle:** Same ACK-by-deletion pattern as patch files.

## 5. Error Handling

- **Never swallow errors silently.** Every failure must produce a log entry at minimum.
- **Log to editor-visible output:** JetBrains `Logger` (visible in `idea.log` with debug enabled), VSCode `OutputChannel` (visible in Output panel).
- **Leave IPC files on failure** so the CLI detects the timeout and can report the error.
- **Content hash verification** (optional): Compare document content hash before and after patch application to detect race conditions with concurrent edits.
- **Graceful degradation:** If a component marker is not found during patch application, skip that patch entry and log a warning. Do not abort the entire patch.

## 6. Testing Requirements

Each plugin implementation should have tests (or manual test procedures) covering:

1. **Patch application -- replace mode:** Content between markers is fully replaced.
2. **Patch application -- append mode:** New content is appended after existing content, before the close marker.
3. **Patch application -- prepend mode:** New content is inserted after the open marker, before existing content.
4. **Component matching with inline attributes:** `<!-- agent:exchange patch=append -->` is correctly parsed; mode is extracted from attributes. `mode=append` also works as a backward-compatible alias.
5. **Full content replacement:** `fullContent` field replaces entire document; `patches` array is ignored.
6. **Frontmatter merge:** New keys are appended, existing keys are updated, key order is preserved.
7. **Missing component graceful fallback:** Patch for a non-existent component is skipped without error; `unmatched` falls back from `exchange` to `output`.
8. **File-not-found retry:** Target file not in VFS triggers 200ms wait + VFS refresh + retry.
9. **ACK protocol:** Patch file is deleted only after successful application; left in place on failure.
10. **Concurrent edit safety:** Patch application while user is typing does not corrupt the document or lose user edits.
11. **Double-invocation guard:** Rapid submit/claim calls do not produce duplicate CLI invocations.
