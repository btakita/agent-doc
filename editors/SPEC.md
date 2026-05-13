# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, call `agent-doc route --dispatch-only --plain-trigger <relative-path>` from the project root. This action must send the plain `agent-doc <FILE>` reopen into the owning live session; it must not restart Codex just because the latest tracked prompt was `/clear`.
- **Feedback:** Show an immediate in-flight info notification while `agent-doc route` is running, then finish with an inline hint near the cursor. Error notifications persist. If an editor persists exact route failures to disk, a later successful route for the same document must clear that saved diagnostic so obsolete startup/proof failures are not surfaced after recovery.
- **Availability:** Only enabled when a `.md` file is active.

## 2. Claim for Tmux Pane

- **Trigger:** `Ctrl+Shift+Alt+C` (configurable)
- **Behavior:** Detect which editor split the file is in (left/right/top/bottom), call `agent-doc claim <relative-path> --position <pos>`. Falls back to no `--position` if split is not detected.
- **Feedback:** Inline hint near cursor. After claiming, trigger a layout sync (silent).

## 3. Sync Tmux Layout

- **Trigger:** `Ctrl+Shift+Alt+L` (configurable)
- **Behavior:** Collect all visible `.md` files, preserve one visible editor group per `--col` (including empty split placeholders when the API exposes them), detect split orientation, and reconcile the existing tmux layout without replacing live or ambiguous owners. Manual sync first lets full `agent-doc sync` repair window order to `0:agent-doc`, `1:stash`, and adjacent overflow `N:stash` windows, but it must not expand the visible `agent-doc` tmux window beyond the editor-visible projection when a protected open-cycle pane prevents safe detach; it should preserve the current cardinality and warn. Automatic editor sync paths should use `agent-doc sync --no-autostart ...`; explicit focus-only moves can still use `agent-doc focus <file>`. The `--no-autostart` contract is non-destructive, not "never start anything": on the fast happy path it should reuse the latest matching pane immediately, otherwise fall back to an alive exclusive registered pane, and only then cold-start a new pane when no owner remains.
- **Feedback:** Inline hint near cursor for applied sync. If manual sync exits `0` but reports preserve-layout output, show a concise deferred-sync warning instead of replacing it with a generic success hint. The full CLI output should remain available in logs for diagnosis.

## 4. Tab-to-Pane Sync (Automatic)

- **Trigger:** Editor tab selection changes.
- **Behavior:** For every real active `.md` file or visible layout change, call `agent-doc sync --no-autostart --exact-visible ...` when the editor API has captured the full visible markdown projection, rather than substituting `agent-doc focus <file>` for single-file handoffs. The passive sync path owns stash rescue/layout reconciliation and safe pane replacement while still avoiding destructive replacement of live or ambiguous owners. That passive path should optimize for fast pane handoff: matching pane first, alive registered pane second, cold-start only when neither exists, and open closeout panes must protect only their own DETACH rather than deferring a different document's sync.
- **Focus-only split switches:** If an integration cannot capture the full visible projection and reports only the newly focused file while the previous visible layout had multiple columns, plain `sync --no-autostart` must treat it as a same-side tab switch: replace only the currently active tmux column and preserve remembered sibling columns until the editor reports a fuller layout. When the editor does report the full projection, `--exact-visible` makes a single visible markdown file authoritative so stale remembered siblings are not resurrected.
- **Immediate focus handoff:** When the active markdown document changes, plugins should first attempt `agent-doc focus <file>` for the selected document without waiting for debounce, sync-lock acquisition, controller actor-binding RPC, stash pruning, or full layout reconciliation. This fast focus attempt is best-effort: failures such as "no pane registered" are ignored because background sync owns rescue/provisioning. The full `agent-doc sync --no-autostart ...` reconciliation still runs afterward through the guarded/debounced path.
- **Debounce:** 100ms. Skip only exact duplicate selection state. Concurrency guard is last-wins: while one automatic command is running, retain only the latest newer request and replay it immediately after the running command finishes. If the running command later reports a retryable deferred result, do not schedule a retry for that superseded intermediate snapshot; leave the completed process in the background and replay only the latest request the user landed on. First opposite-pane selections in a visible split must still dispatch.
- **Legacy deferred preserve-layout handling:** Older `agent-doc` builds may report that they preserved the current tmux layout because a visible protected pane could not be detached yet. If a plugin sees that legacy marker, it must not mark the selection as synchronized unless the same output includes `[sync] safe_passive_layout_preserved_reselected_focus`; current builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- **Sync contention:** Automatic `agent-doc sync --no-autostart ...` output containing `[sync] safe_passive_sync_lock_contention_retry` is retryable, not applied. Editors must leave the dedup state unchanged and replay only the latest pending selection/layout request. Manual `Sync Tmux Layout` must share the editor-side sync guard with automatic sync; if another editor sync is already running, it should show a concise deferred warning instead of launching a second CLI process that waits on the project lock.

## 5. Prompt Polling

- **Trigger:** After a Run action, poll `agent-doc prompt --all` every 1.5s.
- **Behavior:** Detect numbered-option permission prompts. Display a bottom-anchored panel with buttons for each option. Support keyboard selection (Alt+1..9, Alt+Esc toggle, Esc dismiss).
- **Answer:** Call `agent-doc prompt --answer <N> <file>` when user selects an option.
- **Auto-save:** Save tracked files before each poll cycle to capture user edits.

## 6. Popup Menu

- **Trigger:** `Alt+Enter` on a `.md` file.
- **Behavior:** Show numbered popup with Run, Claim, Compact Exchange, Sync Layout, Show Session Status, Restart Supervisor Process, Clear Session Context, and Copy Session Diagnostics actions. Lower-frequency operator actions such as Run with Junie and Force Claim stay available from a non-numbered overflow path instead of consuming top-level numeric shortcuts.

## 6a. Session Operator Actions

- **Show Session Status:** Run `agent-doc session status <relative-path>` and surface the full output in an IDE-owned diagnostics surface. A successful status command must clear any persisted route-error diagnostic for the same document.
- **Restart Supervisor Process:** Run `agent-doc session restart-supervisor <relative-path>` (the legacy `session restart` alias remains valid) and show an inline success hint once the restart request is accepted.
- **Clear Session Context:** Run `agent-doc session clear <relative-path>` so the authoritative session receives the harness-native clear command instead of the plugin pasting `/clear` directly into tmux. The next Run action must still dispatch the bare reopen into that same session. Plugins must not block clear on plugin-local response-status or busy flags. The binary owns live pane evidence for status and diagnostics, but file-scoped clear does not refuse solely because stale or conservative live evidence says `alive-busy`; it logs that classification and sends the clear command through the resolved live pane or supervisor path.
- **Copy Session Diagnostics:** Run `agent-doc session doctor <relative-path>`, show the output in an IDE-owned diagnostics surface, and offer a one-click copy path for the exact text.
- **Verification floor:** editor-plugin tests must cover exact session-status display, `session clear` command routing, and a persistent route-dispatch failure surface with the exact stage-specific CLI output.

## 7. Notifications

- **Success:** Lightweight inline hint near cursor (auto-dismissing, ~1-2 seconds).
- **Error:** Persistent notification entry/tool-window output. Errors never auto-dismiss.
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

**Compact full-content patches:** When `agent-doc compact <file> --component exchange --commit` emits an IPC payload with `fullContent` and no component patches, both editor plugins must apply the replacement through the editor-native document API (`Document.setText` / `WorkspaceEdit`) and save the document before acknowledging the patch. This keeps Compact Exchange from surfacing an external-file-change dialog for the active markdown buffer while still letting the binary own archive, snapshot, and commit closeout.

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
