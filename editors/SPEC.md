# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, call `agent-doc route --dispatch-only --plain-trigger <relative-path>` from the project root. This action must send the plain `agent-doc <FILE>` reopen into the owning live session; it must not restart Codex just because the latest tracked prompt was `/clear`. While the editor reports active typing for the document, the binary-owned supervisor must defer idle-queue `/clear` and `agent-doc <FILE>` continuation submits so it never races a half-edited queue head.
- **Feedback:** Show an immediate in-flight info notification while `agent-doc route` is running, then finish with an inline hint near the cursor. Error notifications persist. If an editor persists exact route failures to disk, a later successful route for the same document must clear that saved diagnostic so obsolete startup/proof failures are not surfaced after recovery.
- **Availability:** Only enabled when a `.md` file is active.

## 1a. Typing / Live-Buffer Tracking

- **Behavior:** On every markdown document change, plugins must record both the typing debounce event and the editor-visible buffer digest through the shared FFI (`agent_doc_document_changed_digest(file, len, sha256)`). The digest sidecar is not a write authorization channel; it is a fail-closed direct-disk guard so commands such as Compact Exchange can detect an idle but unsaved editor buffer before rewriting the file on disk.

## 2. Claim for Tmux Pane

- **Trigger:** `Ctrl+Shift+Alt+C` (configurable)
- **Behavior:** Detect which editor split the file is in (left/right/top/bottom), call `agent-doc claim <relative-path> --position <pos>`. Falls back to no `--position` if split is not detected. The binary must use the current live `agent-doc` tmux session for the claim before falling back to a configured project `tmux_session`, so users do not have to edit `.agent-doc/config.toml` when their visible `agent-doc` window is temporarily in another tmux session.
- **Feedback:** Inline hint near cursor. After claiming, trigger a layout sync (silent).

## 3. Sync Tmux Layout

- **Trigger:** `Ctrl+Shift+Alt+L` (configurable)
- **Behavior:** Collect all visible `.md` files, preserve one visible editor group per `--col` (including empty split placeholders when the API exposes them), detect split orientation, and reconcile the existing tmux layout without replacing live or ambiguous owners. Manual sync first lets full `agent-doc sync` repair recoverable crash-left commit boundaries and window order to `0:agent-doc`, `1:stash`, and adjacent overflow `N:stash` windows, but it must not expand the visible `agent-doc` tmux window beyond the editor-visible projection when a protected open-cycle pane prevents safe detach; it should preserve the current cardinality and warn. Sync target-session resolution must prefer the current tmux session when it already has an `agent-doc` window, then fall back to the configured project `tmux_session`. Automatic editor sync paths should use `agent-doc sync --no-autostart ...`; explicit focus-only moves can still use `agent-doc focus <file>`. The `--no-autostart` contract is non-destructive, not "never start anything": on the fast happy path it should reuse the latest matching pane immediately, otherwise fall back to an alive exclusive registered pane, and only then cold-start a new pane when no owner remains.
- **Feedback:** Inline hint near cursor for applied sync. If manual sync exits `0` but reports preserve-layout output, show a concise deferred-sync warning instead of replacing it with a generic success hint. The full CLI output should remain available in logs for diagnosis.

## 4. Tab-to-Pane Sync (Automatic)

- **Trigger:** Editor tab selection changes.
- **Behavior:** For real tab-selection or visible-layout changes, call `agent-doc sync --no-autostart --exact-visible ...` when the editor API has captured the full visible markdown projection, rather than substituting `agent-doc focus <file>` for single-file handoffs. The passive sync path owns stash rescue/layout reconciliation and safe pane replacement while still avoiding destructive replacement of live or ambiguous owners. That passive path should optimize for fast pane handoff: matching pane first, alive registered pane second, cold-start only when neither exists, and open closeout panes must protect only their own DETACH rather than deferring a different document's sync. Editor-specific focus-only events that do not change the visible markdown projection, such as JetBrains focus-gained events while clicking between already-open split editors, are a lightweight focus handoff and must not repeatedly launch full passive sync.
- **Focus-only split switches:** If an integration cannot capture the full visible projection and reports only the newly focused file while the previous visible layout had multiple columns, plain `sync --no-autostart` must treat it as a same-side tab switch: replace only the currently active tmux column and preserve remembered sibling columns until the editor reports a fuller layout. When the editor does report the full projection, `--exact-visible` makes a single visible markdown file authoritative so stale remembered siblings are not resurrected.
- **Immediate focus handoff:** When the active markdown document changes, plugins should first attempt `agent-doc focus <file>` for the selected document without waiting for debounce, sync-lock acquisition, controller actor-binding RPC, stash pruning, or full layout reconciliation. Default focus is the fast no-promotion handoff; blocking stash promotion is available only through explicit CLI flags for manual use. This focus attempt is best-effort and must use a short editor-side timeout: failures such as "no pane registered" or an over-budget subprocess are ignored because background sync owns rescue/provisioning. The full `agent-doc sync --no-autostart ...` reconciliation still runs afterward through the guarded/debounced path.
- **Debounce:** 100ms. Use an event loop or scheduler keyed by editor events, not hot polling, spinlocks, or thread-per-delay retry loops. Skip only exact duplicate selection state. Concurrency guard is last-wins: while one automatic command is running, retain only the latest newer request and replay it immediately after the running command finishes. If the running command later reports a retryable deferred result, do not schedule a retry for that superseded intermediate snapshot; leave the completed process in the background and replay only the latest request the user landed on. First opposite-pane selections in a visible split must still dispatch through the lightweight focus handoff, and a real tab/layout change must still dispatch passive sync.
- **Legacy deferred preserve-layout handling:** Older `agent-doc` builds may report that they preserved the current tmux layout because a visible protected pane could not be detached yet. If a plugin sees that legacy marker, it must not mark the selection as synchronized unless the same output includes `[sync] safe_passive_layout_preserved_reselected_focus`; current builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- **Sync contention:** Automatic `agent-doc sync --no-autostart ...` output containing `[sync] safe_passive_sync_lock_contention_retry` is retryable, not applied. Editors must leave the dedup state unchanged and replay only the latest pending selection/layout request. Manual `Sync Tmux Layout` must share the editor-side sync guard with automatic sync; if another editor sync is already running, it should show a concise deferred warning instead of launching a second CLI process that waits on the project lock.
- **Sync subprocess timeout:** Editor-owned sync subprocesses must be bounded. If a manual or automatic sync process does not exit within the editor timeout budget, the plugin must terminate that subprocess, release its local sync guard, leave automatic dedup state unsynchronized, and allow the latest manual retry or queued automatic request to invoke the binary recovery path again. Automatic sync must use a short timeout and exponential retry backoff so a slow tmux/controller repair cannot freeze ordinary editor focus or tab movement by repeatedly consuming the full manual-sync timeout. A stuck sync spawned before a pane crash must not permanently turn later `Sync Tmux Layout` actions into local "already running" warnings.

## 5. Prompt Polling Removed

- **First-party editor plugins:** No prompt-polling timer. JetBrains and VS Code must not poll `agent-doc prompt --all`, call `agent-doc prompt --answer`, auto-save tracked documents, refresh tracked files, or mutate editor buffers from a defensive prompt UI loop. Permission prompts stay in the owning agent/tmux surface.

## 6. Popup Menu

- **Trigger:** `Alt+Enter` on a `.md` file.
- **Behavior:** Show numbered popup with Run, Claim, Compact Exchange, Sync Layout, Show Session Status, Recycle Supervisor, Restart Agent, Clear Session Context, Interrupt and Clear Session Context, and Copy Session Diagnostics actions. Lower-frequency operator actions such as Run with Junie and Force Claim stay available from a non-numbered overflow path instead of consuming top-level numeric shortcuts.

## 6a. Session Operator Actions

- **Show Session Status:** Run `agent-doc session status <relative-path>` and surface the full output in an IDE-owned diagnostics surface. A successful status command must clear any persisted route-error diagnostic for the same document.
- **Recycle Supervisor:** Run `agent-doc session restart-supervisor <relative-path>` (the legacy `session restart` alias remains valid) and show an inline success hint once the recycle request is accepted. If recycle is refused because the pane is busy or the authoritative actor is still starting, show an operator warning with `Interrupt and restart`, `Show status`, and `Copy details`; the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- **Clear Session Context:** Run `agent-doc session clear <relative-path>` so the authoritative session receives the harness-native clear command instead of the plugin pasting `/clear` directly into tmux. The next Run action must still dispatch the bare reopen into that same session. Plugins must not block clear on plugin-local response-status or busy flags. The binary owns live pane evidence for status and diagnostics: stale busy projection is reconciled when `agent-doc session status` / `session clear` observes direct `alive-idle` evidence with `prompt_ready=true`, but direct `alive-busy` and `pane_current_command=agent-doc` alone are not Clear Session Context blockers because clear is an explicit operator action. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary and repeated clear clicks/commands report the existing queued clear without injecting another `/clear`. If clear is refused because the pane contains protected prompt input or an explicit busy cue that cannot be deferred, plugins must surface an operator warning with `Interrupt and clear`, `Show status`, and `Copy details` actions instead of showing the raw command failure. Codex dim placeholder text is not protected prompt input; the CLI owns that ANSI-aware distinction. `Interrupt and clear` must require explicit confirmation and then run `agent-doc session interrupt-clear <relative-path>`; it is available from the popup/menu as `Interrupt and Clear Session Context` as well as from clear-refusal notifications, and it is the only plugin action that intentionally discards protected prompt input. After that confirmation, editor plugins must not synthesize additional terminal keystrokes: the binary owns harness interrupts, Vim/Neovim prompt recovery, idle/closed waiting, clear retry, and exact recovery messages.
- **Copy Session Diagnostics:** Run `agent-doc session doctor <relative-path>`, show the output in an IDE-owned diagnostics surface, and offer a one-click copy path for the exact text.
- **Verification floor:** editor-plugin tests must cover exact session-status display, `session clear` command routing, and a persistent route-dispatch failure surface with the exact stage-specific CLI output.

## 7. Notifications

- **Success:** Lightweight inline hint near cursor (auto-dismissing, ~1-2 seconds).
- **Error:** Persistent notification entry/tool-window output. Errors never auto-dismiss.
- **No temp files:** All diagnostic logging uses the IDE's built-in logger, not file I/O.

## 8. File Filtering

- All actions are only enabled/visible when a `.md` file is active or selected.
- Non-`.md` files are ignored by tab sync.

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

**Compact full-content patches:** `agent-doc compact <file> --component exchange --commit` uses the guarded direct-write path rather than emitting editor IPC `fullContent`. Editor plugins must reject or delete any legacy/foreign `fullContent` payload without applying a whole-document replacement.

**Recommended implementation:** Call the appropriate FFI variant via JNA on the shared library (`libagent_doc.so` / `libagent_doc.dylib`). This ensures identical cleanup logic across all platforms and prevents divergence between plugin and binary behavior.

**When to reposition:**
- After applying an IPC patch (when `reposition_boundary` flag is set)
- After receiving a standalone reposition IPC signal (post-commit). Post-commit signals should carry the committed `boundary_id` so the editor normalizes back to `HEAD` instead of inventing a new local diff.
- After commit cleanup when only file-watch IPC is available. The CLI must enqueue a reposition-only JSON patch in `.agent-doc/patches/<hash>.json` with `patches: []`, `reposition_boundary: true`, `preserve_head: true`, and the committed `reposition_boundary_id` instead of rewriting the markdown file on disk. Editors must apply that patch through the native document API and preserve ` (HEAD)` markers.

**Response heading visibility:** During normal IPC patch application (before post-commit cleanup), plugins must preserve transient ` (HEAD)` markers on newly added `### Re:` headings in `agent:exchange`. That marker is part of the user-visible "fresh response is still uncommitted" state; only the explicit clean reposition/commit path is allowed to strip it.

**Visible-write structure guard:** Before publishing an IPC receipt for a component patch and before mutating the editor-visible document buffer, plugins must run the shared Rust template-structure guard (`agent_doc_normalize_template_structure`) on the final candidate document. The guard may repair safe duplicate scaffold shells, such as a duplicated queue/backlog scaffold between two `<!-- /agent:exchange -->` closers, but it must fail closed when that duplicated shell contains user text or other ambiguous content. A rejected guard means the plugin must not call `Document.setText`, `WorkspaceEdit`, or VFS binary-content replacement for that candidate.

**Active-typing visible-write guard:** Before any editor-visible document mutation, including socket patches, file-watch patches, and boundary reposition, plugins must wait for the shared typing debounce. If the bounded wait times out, the plugin must not call `Document.setText`, `WorkspaceEdit`, or VFS binary-content replacement. File-watch patches stay on disk for retry; socket writes fail closed so the CLI can take its normal retry/fallback path. Disabled full-content file-watch payloads are deleted as stale/foreign patches instead of retried.

**Replica-churn live-buffer provenance (`#falsetyping-guard`):** A `remoteCrdtApply` (a CRDT-replica reconciliation applying another replica's update to the local buffer) is not operator typing and must not be reported as a genuine unsaved operator edit. Plugins must report the live-buffer snapshot through the shared FFI provenance entrypoint `agent_doc_document_changed_digest_content_for_editor_v3(..., no_unsaved_operator_edits)`, passing `1` only when the buffer holds no unsaved local operator edits ahead of disk (any divergence is replica-driven), and `0` (the conservative default older `_v2`-only plugins imply) when unsaved operator text may be present. Each plugin tracks a per-document unsaved-local-edit flag: set on a non-`remoteCrdtApply` document change and cleared when the document is saved, closed, or observed clean (not dirty). This lets the CLI visible-write guard re-merge on replica churn instead of failing closed with "save or discard", while a genuine unsaved operator edit still fails closed. Plugins stay thin reporters; the reconcile decision lives in the binary. The JetBrains plugin uses `Document`/`FileDocumentManager.isDocumentUnsaved` and the VS Code extension uses `TextDocument.isDirty` for the clean observation. Falling back to `_v2`/`_v1` against an older cdylib degrades safely to the conservative fail-closed default.

**CRDT replica transport:** First-party editor plugins send CRDT replica IPC only to the CPC/project-controller socket (`.agent-doc/controller.sock`) using the controller `crdt_replica` envelope. Plugins must not connect to per-session supervisor sockets such as `.agent-doc/supervisor/*.sock` for replica register/update/pull/ack/deregister/current-text work. Remote CRDT delivery is event-driven: when the CPC queues fan-out or replace-rebootstrap work, it writes `.agent-doc/crdt-replica-events/<document-hash>.json`; plugins watch that directory and drain pending deliveries for the named document. Startup/open-document attach and local-delta forwarding may also queue one-shot drains, but plugins must not run a fixed interval pull loop, hot polling loop, spinlock, or thread-per-delay CRDT retry loop.

**Prompt steering and stale process state:** CPC owns prompt-steering reconciliation for concurrent operator edits. Editor IPC apply, receipt, visible-write proof, and repair decisions must not be vetoed by legacy per-session supervisor freshness checks. Stale supervisor detection may remain an operator/session-management diagnostic, but it is not an editor-plugin transport guard and must not short-circuit CPC-owned editor delivery.

**Editor state signals:** CPC-owned state files such as `.agent-doc/turn-scope/*.json`, `.agent-doc/crdt-replica-events/*.json`, and the global cdylib reload-broadcast file are watched through editor/file-system event APIs. If a platform drops a file event, the next direct user/editor event may refresh opportunistically, but first-party plugins must not maintain defensive fixed-interval polling for those paths. Turn-state projection refreshes must be cached/coalesced and bounded with minimum refresh spacing plus slow-projection backoff so file-event bursts cannot repeatedly call native projection on the UI path or monopolize an editor worker.

**Typed editor-owned save:** The `save_document` IPC operation is a flush request, not a document replacement. It may only save the already-open editor buffer through the editor's native save API (`FileDocumentManager.saveDocument` in JetBrains, `TextDocument.save` in VS Code), after the active-typing debounce succeeds. The plugin must then publish the saved buffer through `agent_doc_editor_content_applied_for_editor_v1` with `lazily_transport_receipts_v1` so the binary can prove what reached disk. It must not write `.agent-doc/ack-content`, call `Document.setText`, `WorkspaceEdit`, VFS binary-content replacement, reconnect reread repair, or synthesize `fullContent` delivery from this path. Missing lazily receipt support is an incompatible plugin/native-library version error. VS Code receives the same typed operation via `.agent-doc/patches/save-document.signal` because it does not run the socket listener.

**Editor apply proof:** After the debounce succeeds and before computing a visible mutation, plugins must capture an editor-local apply proof for the target buffer: the exact text plus the editor's generation signal (`Document.modificationStamp` in JetBrains, `TextDocument.version` in VS Code). Immediately before `Document.setText` or `WorkspaceEdit`, the plugin must re-check that both the text and generation still match that proof. If either changed, the patch is stale relative to live typing and must be rejected without a lazily receipt. This applies to file IPC, socket IPC, and exchange append patches.

**Full-content IPC receiver disabled:** First-party CLI paths no longer emit full-content IPC payloads, and editor plugins must not apply any legacy/foreign `fullContent` payload. File-watch payloads with `fullContent` are logged and deleted so they cannot retry indefinitely; socket payloads fail closed. Source-buffer proof fields remain parseable only for diagnostics and are not authorization to replace the document.

**Cycle 1779845677327 receiver fixture:** The historical race packet combined a legacy `fullContent` payload with a post-exchange scratch HTML comment containing `#spec-test-build-install-commit-push` and `dispatch #spec-test-build-install-commit-push`. Both JetBrains and VS Code receivers must reject that payload before any visible write API call; the directive-looking lines inside the ordinary comment are not an editor-side authorization to replace, move, or delete user text.

**IPC repair decision contract:** When lazily visible-write normalization diverges or post-IPC dedupe changes the committed snapshot candidate, the binary must materialize a typed repair decision before touching the visible buffer. The decision records the repaired snapshot content, snapshot source (`lazily_visible_write_event`, `content_ours`, or `file_read`), disk-repair reason, normalization targets, editor bad-state length/hash fingerprint, and `redeliver_editor=true|false`. Snapshot-only repair is binary-owned: when `redeliver_editor=false` or full-content IPC is disabled, the binary may repair snapshot/disk state without sending editor `fullContent`, and plugins must not synthesize a visible replacement. Prefix-only visible-write divergence should be re-delivered as a narrow normalization/reposition patch (`patches: []`, `unmatched: ""`, `normalize_prefix_lines`, `reposition_boundary: true`) when the visible buffer still matches the rejected bad-state proof; plugins must treat that payload as normal patch work, not pure reposition. Full-content editor redelivery is disabled for all first-party editor integrations.

**Per-connection listener thread (`#jbacceptwedge`):** The shared FFI socket listener (`agent_doc_start_ipc_listener[_v2]` in `ipc_socket.rs`) must service each accepted connection on a fresh thread so a slow CRDT-merge / disk-I/O apply handler can never block the accept loop and pile connections up in the socket backlog (the "22 unaccepted connections" wedge). Both IntelliJ and VS Code consume that same listener through JNA/FFI, so accept-loop threading stays in the binary — the editor plugin owns only the per-message callback body. Each accepted connection records an `ipc_accept_thread_spawned inflight=N` ops.log marker; any live entry with `inflight>=2` proves the per-connection path is exercising concurrency that the old single-threaded loop could never reach.

## 10. CLI Dependency

- Plugins resolve `agent-doc` from: `~/bin/`, `~/.local/bin/`, `~/.cargo/bin/`, `/usr/local/bin/`, or `$PATH`.
- All commands run from the project root directory.
- Plugins are thin wrappers — business logic lives in the CLI.
- Native/FFI code embedded in an editor host must never treat `std::env::current_exe()` as the agent-doc CLI unless that executable basename is `agent-doc*`. In JetBrains and VS Code hosts, `current_exe()` is the editor process; controller, recycle, and handoff launches must resolve the real CLI through an explicit agent-doc binary override or the same install-path/PATH search used by plugin subprocess commands.

## 11. Visual Distinction

- Markdown editor integrations should visually distinguish agent-doc structures without requiring the user to switch document formats or install a separate grammar pack.
- Both JetBrains and VS Code must source their highlight ranges from the shared FFI surface (`agent_doc_visual_tokens_json`) so component markers, agent-managed component bodies, patch markers, boundary markers, `### Re:` headings, `❯` prompts, tracked `[#id]` tags, standalone bracket labels such as `[recommended]`, and ordinary HTML scratch comments plus their bodies stay in sync across editors.
- `agent_doc_visual_tokens_json` returns UTF-16 document offsets, not raw UTF-8 byte positions. Plugins must treat those offsets as editor-ready range endpoints and pass them directly to native document APIs.
- Matches inside fenced code blocks or inline code are excluded from this highlighting contract; example markup in code samples must remain untouched.
