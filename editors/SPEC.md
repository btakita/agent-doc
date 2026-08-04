# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, then send a Project Controller `editor_route` request for the relative path from the project root. This action must send the plain `agent-doc <FILE>` reopen into the owning live session; it must not restart Codex just because the latest tracked prompt was `/clear`. While the editor reports active typing for the document, the binary-owned supervisor must defer idle-queue `/clear` and `agent-doc <FILE>` continuation submits so it never races a half-edited queue head.
- **Feedback:** Show an immediate in-flight info notification while the Project Controller `editor_route` request is running, then finish with an inline hint near the cursor. Error notifications persist. If an editor persists exact route failures to disk, a later successful route for the same document must clear that saved diagnostic so obsolete startup/proof failures are not surfaced after recovery.
- **Availability:** Only enabled when a `.md` file is active.

## 1a. Typing / Live-Buffer Tracking

- **Behavior:** On every markdown document change, plugins must apply the incremental edit to their CRDT replica and report typing/sync epochs through the shared FFI. The Lazily/CP CRDT is the only live-text authority; plugins must not create a full-buffer filesystem sidecar.

## 2. Claim for Tmux Pane

- **Trigger:** `Ctrl+Shift+Alt+C` (configurable)
- **Behavior:** Detect which editor split the file is in (left/right/top/bottom), call `agent-doc claim <relative-path> --position <pos>`. Falls back to no `--position` if split is not detected. If that target is already owned by another document, the binary provisions a distinct pane instead of replacing it. On a cross-session reject, both editors offer **New Pane in This Session**, which invokes `agent-doc claim <relative-path> --new-pane` without positional/force flags; the binary owns authoritative-session selection and pane allocation.
- **Feedback:** Inline hint near cursor. After a successful claim, trigger a layout sync (silent). A failed claim must leave the active editor document selected: do not run a layout sync after the failure, and do not let reverse tmux-to-editor focus mirroring reopen the previously focused tmux document as a side effect of the failed claim.

## 3. Sync Tmux Layout

- **Trigger:** `Ctrl+Shift+Alt+L` (configurable)
- **Behavior:** Collect all visible `.md` files, preserve one visible editor group per column (including empty split placeholders when the API exposes them), detect split orientation, and reconcile the existing tmux layout without replacing live or ambiguous owners. Manual sync submits `agent-doc.sync_tmux_layout.v1` to the Project Controller command plane and waits for its terminal receipt. The controller runs the full sync path so recoverable crash-left commit boundaries and window order are repaired to `0:agent-doc`, `1:stash`, and adjacent overflow `N:stash` windows. If a requested document has no pane, the same manual command must create and register one in the resolved target session, wait until its harness is dispatch-ready, and submit the document route before reporting success. It must not expand the visible `agent-doc` tmux window beyond the editor-visible projection when a protected open-cycle pane prevents safe detach; it should preserve the current cardinality and warn. Sync target-session resolution must prefer the current tmux session when it already has an `agent-doc` window, then fall back to the configured project `tmux_session`. Automatic editor sync paths should use the passive `sync --no-autostart ...` reconciliation path through the same Project Controller command-plane submit. Explicit focus-only moves submit `agent-doc.focus_document_pane.v1` to the Project Controller command plane. The `--no-autostart` contract is non-destructive, not "never start anything": on the fast happy path it should reuse the latest matching pane immediately, otherwise fall back to an alive exclusive registered pane, and only then cold-start a new pane when no owner remains.
- **Feedback:** Inline hint near cursor for applied sync. If a Project Controller-backed manual sync reports a preserved-layout outcome, show a concise deferred-sync warning instead of replacing it with a generic success hint. Pane creation, readiness, or route-submit failures must be surfaced from the terminal controller output in the IDE; the full controller receipt or automatic-sync CLI output should remain available in logs for diagnosis.
- **Acknowledgement:** For automatic layout sync, the plugin may trust the controller's terminal applied command receipt once the desired projection generation is published; it need not wait for the later tmux effect receipt. Foreground routes instead wait for physical pane count/order and focused-pane convergence before dispatch. Repeating an identical foreground request is itself a physical observation edge, so focus or geometry drift cannot be acknowledged from stale `Converged` state. This admission rule does not apply to document CRDT delivery, whose closeout barrier still requires the exact editor-visible projection receipt.
- **Structural edge:** A controller-derived automatic `Sync` intent (first observation, changed columns, or observed tmux drift) must execute the tmux structural effect for that generation; it must not declare convergence from a structural pane assignment retained from an older editor generation. Pure focus changes remain separate focus effects, but their receipt is terminal only after the active pane in the target window matches the focused document's actor pane.

## 4. Tab-to-Pane Sync (Automatic)

- **Selection/layout coherence:** A selection event must not publish a surface while the selected-files view has advanced to the new document but the split-layout view still names the prior document. Adapters may reread that external editor projection on bounded later event-loop turns; after the budget, they may apply only the exact old→new event edge to each stale projection independently. Missing prior-file evidence remains fail-closed.

Automatic tab-to-pane sync is **reported, not planned** (`#jbsurfaceswap` / `#jbpluginlazilyeffects`). A plugin publishes an ordered fact to an already-running Project Controller; the controller's process-scoped reactive graph derives the intent and its local `Effect` owns the tmux consequence.

- **Trigger:** Editor tab selection changes, visible-layout changes, and editor-specific focus-only activation (JetBrains focus/mouse activation, VS Code active-editor changes while clicking between already-open split editors).
- **Behavior:** Every one of those events reports exactly one observation through `agent_doc_editor_surface_observe_json(project_root, surface_json)`. Native transport adds `(client_id, generation, sequence)` and sends the fact over the existing-controller socket; it never launches the controller. `surface_json` is an `EditorSurface`: `{ "focused": "<absolute path>", "visible": ["<absolute path>", ...], "columns": [{ "files": [...] }, ...], "force_reconcile": bool }`. Paths are absolute so a derived focus can address the document. The request reply is admission/diagnostic framing; accepted results are also controller state-plane projections.
- **What plugins must NOT do:** choose between `focus_document_pane` and `sync_tmux_layout`; hold a previous-observation / previous-signature / last-focused-file field; dedup repeat events; run a preserved-layout or lock-contention retry ladder; apply sync-timeout backoff; or report whether tmux has drifted. All of that is derived in `agent-doc-editor-surface` and driven by an `Effect` in the Project Controller, so the rule exists once rather than once per editor. An observation identical to the previous one is idle and costs nothing, which is why repeat events need no plugin-side guard.
- **Layout detection:** report the split layout the editor detected, preserving column order and including empty split placeholders where the API exposes them. An editor that could **not** detect a layout reports **no** columns rather than synthesizing a single column, so the graph can tell "one column" apart from "unknown" and skip the drift comparison in the latter case.
- **Drift:** whether tmux still matches the reported layout is the controller's observation, never the editor's. The controller fills that side of the mirror locally for multi-column surfaces; a plugin must not query it and report the answer back.
- **Layout + focus projection:** a `sync` intent is a product of the desired columns and focused document. The controller's retained pane-layout `Effect` first reconciles the passive layout, then applies the focused document through a generation-fenced `select-pane`. Matching columns alone are not convergence when focus is required; the typed effect receipt must also prove that the final focus consequence landed. Identical desired values retain their generation, rapid changes collapse to the newest generation, and stale work is cancelled at every safe structural-effect boundary. A focus-only generation reuses the retained structurally converged file-to-pane assignment, so pane selection does not serialize behind another full reconciliation.
- **Mixed-root projection:** one visible editor layout may contain documents
  owned by different project controllers. The parent layout effect retains
  the columns, validates the explicitly observed `agent-doc` window, and asks
  each nested controller only for its existing actor binding. Pane movement
  must not wait for editor-content authority, and the parent must not resume,
  provision, or durably register the foreign document.
- **Debounce:** 100ms, plus a generation guard, on an event loop or scheduler keyed by editor events — never hot polling, spinlocks, or thread-per-delay retry loops. This is the only coalescing a plugin owns: it exists so an event storm reports its final state, not so identical states are suppressed. Report off the UI thread; controller effects never run in the extension host.
- **Project close:** call `agent_doc_editor_surface_forget(project_root)` for every root the plugin observed. The controller also fences observations by client/generation/sequence, so a late callback from a retired native mapping cannot regain effect authority.
- **Feedback:** a derived `sync` projection may show the concise inline layout hint; `focus` and `idle` show nothing. The full projection stays in the plugin log for diagnosis.
- **Manual `Sync Tmux Layout`** (§ 3) remains a direct controller submit and keeps its own preserved-layout warning; it is an explicit operator action, not an observation.

## 4a. Reactive Document Rename / Move

- A VFS rename or move is a retained old-path → new-path transition, not an implicit layout-sync request. The plugin first publishes a single reliable-sync liveness batch ordered as new-path `Open`, new-path `Register`, old-path `Close`; an enqueue or flush retry must replay the exact original OR-set tags.
- The existing Project Controller moves the live CRDT hub, aliases in-flight old-path requests, merges any raced old/new durable history, retires both hashes' ACK cursors, and rekeys the actor/session registry. Requests are effect attempts; the retained projection and convergence receipt are authority.
- The editor registers and seeds its new-path replica before retiring the old-path forwarder. Pane and window bindings move with the session registry entry. Rename handling must never invoke `sync_tmux_layout`, rebalance panes, or alter split orientation.

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

**CRDT-only attached write convergence:** Unless the operator explicitly requested the `--force-disk` recovery escape hatch, a document with an attached CRDT replica must wait for typing quiescence and use the canonical CRDT document as the sole visible mutation plane. Every accepted candidate is first retained in the Lazily deferred-write lineage. Each editor publishes its complete visible-state hash after integrating deliveries; that projection cumulatively retires every represented pending frontier, and an identical retry is an idempotent canonical no-op. An accepted CRDT replacement must never be followed by component/full-content IPC for the same candidate. Disk materialization is only a projection of exact canonical text after every attached replica projects the delivery frontier, and it is refused if canonical text advances first. No replay, re-register, force-refresh, save, or build-skew recovery request is emitted from the convergence subscription. Explicit force-disk writes still pass through the per-document actor, retain their pre-write base and target in Lazily state, and intentionally skip the CRDT gate.

**Open-editor external-disk pending:** Any disk change observed while one or more editor buffers are open is retained in an independent Lazily external-disk slot; it must not replace the pending agent-response lineage or enter canonical CRDT state. Authority is editor/CRDT, then disk after the final editor closes, then Git only as historical recovery evidence. Accepting/reloading the exact disk candidate reboots the stale replica from the visible editor buffer and clears the candidate only after CRDT propagation. A newer editor mutation clears the candidate and propagates the editor cut without component merge. Saving the editor and flushing those exact bytes to disk also clears any older candidate and makes the saved cut authoritative. If several replicas are still converging and no exact editor cut is proven, the disk candidate remains mutation-free pending. JetBrains and VS Code must implement the same resolver/propagation FFI contract.

**Replica-churn provenance (`#falsetyping-guard`):** A `remoteCrdtApply` is not operator typing and must not be reported as a genuine unsaved operator edit. The compatibility FFI entrypoint `agent_doc_document_changed_digest_content_for_editor_v3(..., no_unsaved_operator_edits)` carries only that provenance; its legacy metadata and full-content arguments must never become a second registration or text channel. Current text is read from the CRDT. Each plugin tracks a per-document unsaved-local-edit flag: set on a non-`remoteCrdtApply` document change and cleared when saved, closed, or observed clean. JetBrains uses `Document`/`FileDocumentManager.isDocumentUnsaved`; VS Code uses `TextDocument.isDirty`.

**Monotone editor registration:** JetBrains and VS Code include identity, path, kind, version, and capabilities in the same durable reliable-sync batch as each `Open` fact. Registration is therefore once per open epoch, not a side effect of rate-shaped content reports. Text-change listeners use the matching lazily-kt/js `DebounceCore` KeepLatest primitive only for compatibility provenance reports and CRDT ops; superseding input cancels the old timer without losing the logical pending value. Close/dispose emits the observed-tag `Close`, so an old close cannot erase a newer open/registration epoch.

**CRDT replica transport:** First-party editor plugins send CRDT replica IPC only to the Project Controller socket (`.agent-doc/controller.sock`) using the controller `crdt_replica` envelope. Plugins must not connect to per-session supervisor sockets such as `.agent-doc/supervisor/*.sock` for replica register/update/pull/ack/deregister/current-text work. Remote CRDT delivery is event-driven through the shared targeted `EditorIntent` channel and Lazily-backed controller subscriptions; it never writes a per-document event file. Startup/open-document attach and local-delta forwarding may also queue one-shot drains, but plugins must not run a fixed interval pull loop, hot polling loop, spinlock, or thread-per-delay CRDT retry loop.

**Cross-editor native harness:** `make cross-editor-simworld` runs the shipped JetBrains and VS Code forwarders, Project Controller transports, and native FFI nodes as headless peers through a real `agent-doc` Project Controller. `editors/plugin-parity.tsv` selects only peers whose required feature rows are `supported`; `staged` peers are excluded until their endpoint and receipts exist. This suite proves plugin transport/FFI behavior, while IDE-host Document/VFS wiring remains the responsibility of live editor smokes.

**Prompt steering and stale process state:** The Project Controller owns prompt-steering reconciliation for concurrent operator edits. Editor IPC apply, receipt, visible-write proof, and repair decisions must not be vetoed by legacy per-session supervisor freshness checks. Stale supervisor detection may remain an operator/session-management diagnostic, but it is not an editor-plugin transport guard and must not short-circuit Project Controller-owned editor delivery.

**Editor state signals:** Turn-state status is read from the Project Controller `state_subscribe` Lazily projection and mirrored in the plugin; first-party plugins do not read filesystem state for ordinary status/UI synchronization. If the Project Controller is unavailable, the editor status bar shows `agent-doc: Project Controller disconnected` rather than electing a fallback. CRDT remote delivery, VCS refresh, and native-library reload use the same targeted editor-intent channel and reliable-sync receipts. Turn-state projection refreshes are cached/coalesced and bounded with minimum refresh spacing so repeated editor events cannot monopolize an editor worker.

**Reactive persistence projection:** There is no editor save request. Applying a
controller CRDT projection and the editor platform's ordinary save lifecycle
publish visible-state and persisted-state Sources. The binary derives exact
convergence and resumes retained effects from those projections. Plugins must
not implement a `save_document` intent, save signal, ACK sidecar, reconnect
reread repair, or secondary full-content delivery channel.

**Editor apply proof:** After the debounce succeeds and before computing a visible mutation, plugins must capture an editor-local apply proof for the target buffer: the exact text plus the editor's generation signal (`Document.modificationStamp` in JetBrains, `TextDocument.version` in VS Code). Immediately before `Document.setText` or `WorkspaceEdit`, the plugin must re-check that both the text and generation still match that proof. If either changed, the intent is stale relative to live typing and must be rejected without a Lazily receipt. This applies to every named socket intent and exchange append operation; file IPC does not exist.

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
