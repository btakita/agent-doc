# JetBrains Plugin Specification

Extends `editors/SPEC.md` with JetBrains-specific behavior.

## Plugin Metadata

- **ID:** `com.github.btakita.agent-doc`
- **Name:** Agent Doc
- **Restart:** Required for Kotlin plugin package upgrades (`require-restart="true"`)
- **Native upgrades:** Safe in-process generation handoff

## Implementation Details

An installed-library mtime change or typed `reload_library` intent enters one
application-wide handoff. On Linux, JetBrains marshals JNA calls onto a bounded
generation-owned worker pool. Calls for distinct document replicas may run in
parallel, while each replica remains serialized by its document worker and its
Rust per-replica lock. A call that times out before it starts is cancelled
without poisoning the generation; only a call that actually ran beyond the
lease disables it. Reload stops and joins every CRDT/listener worker, drains
calls, asks the old cdylib to quiesce replicas, terminates all owner threads so
Rust TLS destructors run, and closes the old handle. If glibc retains that
closed Rust cdylib mapping because another JVM thread once acquired Rust TLS,
the mapping is inert and remains on disk until process exit; it does not cause
the plugin to reopen stale code. The replacement loads from a distinct
per-install shadow path/inode and becomes the only published generation.
Controller launch from the cdylib uses a short-lived external helper, so no
child-reaper thread retains active old-generation work. Failure at any quiesce,
drain, owner-thread, close, replacement ABI, or replacement-load boundary
publishes no second generation and requires restart; a replacement-load
failure may restore the old named shadow. Durable reliable-sync
outboxes live in the project controller; the reloadable cdylib sends typed
controller RPCs and retains no SQLite connection.

`require-restart="true"` applies to Kotlin package/classloader upgrades only;
it does not disable the native-library handoff.

### Claim — Split Position Detection

Two strategies for detecting the file's position in the editor split:

1. **Splitter tree walk:** Get the `EDITOR` component from action context, walk the Swing `Splitter` tree to determine if it's in the first child (left/top) or second child (right/bottom).
2. **Window index fallback:** If no EDITOR context (e.g., context menu), enumerate `FileEditorManagerEx.windows`, find which window contains the file, determine position from the Splitter tree or use window index + orientation as heuristic.

On a cross-session claim reject, the first recovery choice is **New Pane in This Session**. The plugin invokes `agent-doc claim <file> --new-pane` without `--position` or `--force`; all allocation/session decisions remain binary-owned. The existing Force Claim and Switch Project Session choices remain explicit destructive/migration alternatives.

### Prompt Panel

- Rendered as a `JLayeredPane` overlay at `POPUP_LAYER` — no `JDialog` (avoids WM leaks and focus-loss dismissal).
- Uses IDE editor font via `EditorColorsManager`.
- Single-row, non-wrapping layout. The panel height is fixed to one prompt row and does not grow on narrow windows.
- Question and option labels truncate with ellipsis; full text is preserved in tooltips.
- Secondary detail (hotkeys, pending-count context) lives in tooltip/secondary UI instead of the main prompt row.

### Tab Sync Listener

- Registered as `FileEditorManagerListener` in `plugin.xml`.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Selection settling requires both `FileEditorManager.selectedFiles` and the detected split columns to contain the event's new document before publishing. If one advances before the other, bounded later-EDT reads continue; on exhaustion, the exact event old→new edge repairs each stale projection independently, so a current selected-files list cannot mask stale one-column geometry.
- Every markdown selection, component-focus, and visible-layout change reports one editor-surface observation containing the focused document, visible documents, open documents, and detected columns. Native transport adds an ordered reload generation/cursor and publishes it to an existing controller. The controller's process-scoped reactive graph derives idle, focus, or layout reconciliation and owns the tmux effect; the plugin and native library do not choose or enqueue an imperative tmux command.
- A fresh editor selection is authoritative for the next projection even if the split component tree is briefly interstitial and still exposes the opposite editor as selected. Component-focus observations cannot replace a newer document-selection observation with a different file.
- The listener first re-reads an interstitial selection projection on bounded later EDT turns. If it still contains the old selected file when that budget is exhausted, the listener applies the selection event's old-to-new file edge to the stale visible/layout projection before publishing; it never synthesizes a replacement unless the old event file is actually present.
- Reverse tmux-to-editor focus sync reads the Project Controller-owned `tmux_focus_state` projection through the Project Controller socket. That projection yields a document only when the configured tmux session's current window is `agent-doc`, so switching to another tmux window must not recall the editor selection from the stale active pane in the hidden agent-doc window. If the active pane still has an exact route-owned process-tree binding but its actor projection was pruned, the controller reports that bound document instead of `active_pane_unbound`; foreign-root and ambiguous owners remain unbound. A reverse mirror may select a different editor tab but must pass `focusEditor=false`, so it cannot reactivate the JetBrains desktop window after the operator moves to another i3 window. Before `Claim for Tmux Pane` starts a CLI claim, JetBrains records the current tmux-focused document as already seen; if the claim fails and tmux focus remains on the previous pane, the reverse focus mirror must not reopen that previous document over the editor-selected claim target.
- The plugin applies only a short event-storm debounce and generation guard before reporting the final surface. It stores no previous layout signature, last-focused file, command-in-flight flag, pending retry, focus lease, or durable controller copy.
- A `sync` projection is the product of columns and focused document. The controller first reconciles the passive layout, then applies the requested pane through a generation-fenced effect. Matching columns alone cannot retire the projection; the effect receipt must also prove focus when it was requested. A newer surface generation cancels stale focus before it reaches tmux.
- If a Project Controller-backed manual `Sync Tmux Layout` terminal outcome later reports that the current layout was preserved because a visible protected pane could not detach yet, the command projection/log must retain the protected pane id, open-cycle phase, and document path so the user can tell which pane is delaying sync. Current controller builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- Automatic layout sync completes at desired-state publication rather than waiting for that exact plane version to become observed. The controller owns a single latest-wins worker, interrupts obsolete retry waits when a newer generation arrives, and never reports a superseded automatic version as a user-visible failure. Manual sync keeps its terminal receipt boundary.
- **Resync / Fix Sessions** first runs registry/liveness cleanup, which must not
  promote registered stash panes. On successful cleanup the same operator
  action captures the current editor columns on the EDT and publishes one
  exact-visible desired state with `caller_kind=resync` and
  `no_autostart=true`. The distinct caller kind forces a new retained
  reconciliation generation even when the documents match the last automatic
  surface. Repeated resyncs preserve editor pane cardinality; they never derive
  visible panes by enumerating the session registry.
- Manual `Sync Tmux Layout` submits `agent-doc.sync_tmux_layout.v1` to the Project Controller command plane
  with `no_autostart=false` and waits for the terminal command receipt. The controller runs the full sync
  path and repairs window order before reconciliation: `0:agent-doc`, `1:stash`, then adjacent
  overflow `N:stash` windows. When the requested document has no pane, that terminal boundary includes
  pane creation in the resolved tmux session, registration, harness readiness, and document-route submission;
  any failure is shown with the controller diagnostic. Automatic tab sync stays on
  `agent-doc.sync_tmux_layout.v1` with `no_autostart=true` and must not perform that repair step.
- If passive `agent-doc sync --no-autostart ...` output from an older build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must treat that as deferred rather than complete for both the generic `[sync] sync preserved...` marker and the safe-passive `[sync] safe passive sync preserved...` marker: leave dedup state unchanged and schedule bounded retries until the requested selection applies or a newer request supersedes it.
- If a passive sync terminal outcome contains `[sync] safe_passive_sync_lock_contention_retry`, JetBrains must treat the command as deferred, keep the dedup state unchanged, and retry the newest pending automatic selection/layout request rather than waiting for the CLI's full sync-lock budget. Manual `Sync Tmux Layout` uses Project Controller command supersede/admission instead of a long-lived editor-side native sync guard.
- JetBrains split-editor focus follows both editor focus-gained events and editor mouse-press activation because Swing focus is not guaranteed to change on every click between already-open split editors. Consecutive events for the same markdown path are deduped locally, but alternating paths such as A -> B -> A must each attempt the Project Controller focus handoff.
- The structural layout-change detector only reports a new editor surface observation into `EditorTabSyncListener`; it has no second CLI planner, lock, or tmux process. The JetBrains/native boundary validates and enqueues that observation without waiting for a controller probe or tmux consequence. One worker per project root performs those consequences off the native-call generation and replaces any not-yet-started surface with the newest one. Tab selection, focus, and structural changes therefore share one latest-wins surface graph without letting a blocked controller call freeze or disable the IDE bridge.
- JetBrains bounds every command-plane automatic and manual sync call. On timeout or failure, it releases the plugin-local request guard, logs the failure, and leaves automatic tab-sync dedup state unchanged so the latest queued selection or a manual retry can run again. A dead or externally killed tmux pane may delay one request, but it cannot leave a second editor-side sync owner wedged.

### Prompt Poller Removed

- JetBrains must not start a defensive `PromptPoller` / `PromptPanel`, poll `agent-doc prompt --all`, auto-save tracked documents, or run timer-based merge/reload logic from prompt handling.
- Permission prompts remain in the owning agent/tmux surface.

### Run Feedback

- The active-turn banner is derived from the Project Controller
  `state_subscribe` closeout payload, including `realtime_steering` kind/count
  and the full aggregate as hover text. The identity-keyed `elements` object is
  preserved verbatim from the Rust projection. Its `observed_content_hash` is
  the controller's canonical CRDT generation receipt; a receipt-only empty set
  renders no steering label. Kotlin must not re-read disk or derive steering.
- Exchange prompt-prefix normalization must remain response-aware. A Markdown
  blockquote under `### Re:` is quoted response context even when its complete
  text is present in the normalization target set; it cannot transition the
  parser into prompt mode or cause later response prose to acquire `❯`.
- `Run Agent Doc` saves only the active markdown document and immediately dispatches a Project Controller `editor_route` request carrying dispatch-only, plain-trigger routing with a 15-second ready wait. The editor path does not block on the typing debounce and does not save unrelated open documents; the active document save is the editor-owned flush boundary before the controller route request runs. Even after `Clear Session Context`, this action still sends the plain `agent-doc <FILE>` reopen into the live session instead of restarting Codex.
- Editor selection never forks `Run Agent Doc` onto another transport. Selected-text invocations and saved-document diff steering use the exact same Project Controller request as an unselected click and submit the same bare `agent-doc <FILE>` trigger; Rust route policy decides whether that starts, replaces, or steers the live turn. Kotlin must not send `selected_text` / `steering_id`, await a turn-steering acknowledgement, or derive actor-state admission independently.
- Every `Run Agent Doc` click writes a durable attempt ledger entry for click receipt, active document save, `editor_route` request construction/start, retry/dedupe, and terminal route outcome. The ledger may persist diagnostic route shape and route output summaries, but it must not persist raw document prompt text; prompt/trigger proof is represented by byte counts and hashes in binary/controller diagnostics.
- Repeating `Run Agent Doc` while an `editor_route` request is still in flight coalesces with that request instead of canceling and recreating route/controller work. The duplicate click is recorded as deduped and gets an already-dispatching hint; a fresh click is eligible as soon as the bounded request completes.
- That JetBrains request-level coalescing must not leak into controller admission for an already-authorized operator reopen. `managed_reopen` and `dispatch_only_reopen` bypass stale same-generation in-flight dispatch receipts so a fresh `Run Agent Doc` request cannot settle successfully without reaching the pane; automatic/non-operator redispatches remain coalesced.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the JetBrains action layer before route/clear work is submitted. Repeated Run clicks coalesce with the first alive `editor_route` request; `Clear Session Context` preempts a still-dispatching Run by canceling the in-flight request and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear completes synchronously. A clear accepted for deferred delivery by the binary does not release the queued Run immediately.
- If route fails only because the authoritative actor is still in its startup window, `Run Agent Doc` performs one short retry before surfacing the final route failure. This includes dispatch-only `latest run is still booting ... (timed_out)` results; active-turn blockers get a still-running notification, and protected-input blockers such as shell history search are not retried. Repeated clicks while that bounded retry is active coalesce with it.
- A stale startup record must not keep a settled actor in the boot wait. If the requested pane's authoritative actor is `Ready`, dispatch-eligible, and the pane has a recognized busy or interactive blocker, routing exits startup probing and applies the normal busy/queue policy.
- If route succeeds by queueing a prompt behind an already-busy authoritative actor instead of injecting a duplicate trigger, `Run Agent Doc` must surface a visible queued/still-running warning rather than treating the route request as silent success. Repeating `Run Agent Doc` after editing that same prompt must replace the sole live route-owned `agent:queue` prompt instead of leaving stale wording queued behind the active turn. The plugin accepts the new `active agent:queue` route diagnostic and the older `agent:queue auto` wording for compatibility.
- If `Run Agent Doc` observes an explicit frontmatter `agent:` change while the authoritative pane still runs the previous harness, route accepts a typed boundary handoff instead of returning a restart recovery error. No trigger is sent into the old harness; the supervisor preserves an active turn, switches at the next safe idle boundary, and auto-triggers the document on the new harness. A paused queue reports that the accepted handoff is held for queue resume and must not offer supervisor restart as the normal recovery.
- When Codex hook tracking is installed, `Run Agent Doc` must not report success from a live reroute that only proved tmux acceptance. If the bare reopen was accepted but Codex never records routed submission proof, the binary must fail once with that exact stage-specific reason and must not precede it with an optimistic success/progress line.
- If the binary exhausts its bounded submit or dispatch-start proof window for `Run Agent Doc`, it must also file a deduped `#jbrunautobug #agent-doc-bug` item in the session document backlog. The item must include the saved route diagnostic path, failure class, document, stage, pane, best-effort actor generation, editor attempt id, dispatch proof, and `ops.log` marker/path; repeated failures for the same document/stage/failure append evidence to the existing item instead of creating duplicate backlog work.
- When the same `agent-doc <FILE>` reopen is already drafted in a Codex/Claude composer, the binary must press one bare `Enter` instead of appending a duplicate trigger. A visible trigger with a later idle prompt below it is stale scrollback and must not receive an Enter.
- A delayed direct-pane resubmit may press `Enter` only while the pane still exposes a recognizable harness surface. It must preserve the draft and refuse injection after the harness has exited to a bare shell.
- The action is silent on route progress/success. Failures are logged to the IDE Event Log / notification tool window instead of showing bottom-right balloon popups.
- A failed route persists the exact `editor_route` output under `.agent-doc/state/editor-route-errors/` and the notification exposes copy/open actions so startup-miss and pending-drift diagnostics remain inspectable after the toast moment. Typed recovery states such as queued, paused, protected prompt input, dispatch-start-unproven, busy/running, and actor-switch defer do not persist generic route-error files. A later successful `Run Agent Doc`, binary route, or focused sync for that document deletes the saved route-error file so the editor cannot keep showing an obsolete startup/proof failure after route recovery.
- Route session targeting follows the same root-aware chooser as sync: a nested document reroute uses that file's nearest `.agent-doc` root, while a mixed-root visible layout stays pinned to the shared workspace root instead of the focused child repo.

### Patch Application Safety

- JetBrains defers socket and file-watch patch application until the target markdown document has been idle long enough for the typing debounce. If the bounded wait times out, the plugin logs the timeout, does not mutate the document, and retries file-watch patches after another debounce window. Socket patches fail closed so the CLI can retry or fall back through the binary-owned closeout path.
- JetBrains has no `save_document` socket intent. It projects visible CRDT state after local/remote integration and after IntelliJ's ordinary save lifecycle; the controller derives persistence convergence without an imperative request or per-operation ACK.
- Replica refresh registers from the controller bootstrap, reports reconnect propagation, and schedules an ordinary canonical-projection drain. The observed editor cut is only a swap fence; it is never seeded or published as a whole-document recovery baseline.
- Component intents must honor an explicit JSON `op` (`replace`, `append`, or `prepend`) ahead of the component marker's `patch=` / `mode=` attribute. Lazily convergence uses `op: "replace"` for an `agent:exchange patch=append` body so recovery cannot append a second response; it never falls back behind an attached editor.
- JetBrains must not apply a socket or file-watch patch while IntelliJ has a pending File Cache Conflict for that document. The plugin treats the conflict as a terminal editor-side refusal for that IPC payload: socket IPC returns failure, file-watch IPC records `file_cache_conflict_pending`, deletes the queued patch file, refreshes agent-doc visual highlighting, and leaves the response for the binary-owned retry path. It must not retain a conflict-deferred patch id, wait for the dialog to resolve, or replay the old payload after the user keeps memory changes or later accepts filesystem changes. The binary-side follow-up contract for older already-written Cancel-shaped closeouts remains: if the response already reached the working tree/snapshot but not `HEAD`, the next `agent-doc preflight` auto-commits the missing boundary rather than surfacing the older manual `write --commit` recovery requirement.
- Every disk change observed while an IntelliJ buffer is open is retained by the binary as an independent pending external-disk candidate. Accepting filesystem changes produces a clean document event; JetBrains asks the shared resolver for the exact candidate, resets and re-registers the replica from the visible buffer, and reports settlement only after successful CRDT propagation. Keeping memory changes never receives or merges the disk candidate: a later edit or save clears it in the shared binary state. Closing the final editor clears the candidate and falls back to disk; closing one of several editor replicas does not.
- JetBrains file-watch IPC must also accept reposition-only patches emitted by commit cleanup (`patches: []`, `reposition_boundary: true`, `preserve_head: true`). These patches are applied through `Document.setText` / VFS APIs, reuse the committed boundary id when provided, and preserve visible ` (HEAD)` response markers so closeout cleanup does not trigger IDE file-cache conflict dialogs.
- `Run Agent Doc` does not use the patch-application typing debounce. It saves the active markdown document and routes immediately; patch/socket mutation paths keep their bounded idle guards.

### Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in an IDE notification instead of re-deriving status inside the plugin. A successful status response deletes the saved route-error file for that document because the old failure is no longer the latest observed state.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The action ID remains `AgentDoc.RestartSupervisorProcess`. If the binary refuses because the pane is busy or the authoritative actor is still starting, JetBrains must show the typed restart warning with `Interrupt and restart`, `Show status`, and `Copy details` actions; the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. The binary owns live pane status for diagnostics and clear safety: stale busy actor/supervisor projection is reconciled when direct live pane evidence is `alive-idle` with `prompt_ready=true`, including a bottom Codex model/cwd/context footer below older transcript text. A live `agent-doc` wrapper process is not by itself proof that the session is still running; ordinary active/status panes must proceed through the normal clear submit path. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clear clicks report the existing queued clear instead of injecting another `/clear`, and JetBrains surfaces that output as accepted deferred work. Clear may fail closed when the captured pane contains protected prompt input such as a permission prompt, queued draft, shell history search, or drafted user text, or when it shows an explicit busy cue that cannot be deferred such as an active Codex turn, hook-review prompt, or help screen; the operator can then choose the explicit interrupt-clear discard path.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals must show a typed warning with the relevant interrupt action, `Show status`, and `Copy details` instead of the generic command-failed text. Legacy alive-busy and `active_agent_doc` refusals may still use the busy-session warning, but the binary must not emit that warning solely because the live pane is the `agent-doc` wrapper.
- `Interrupt and clear` requires an explicit IDE confirmation and then runs `agent-doc session interrupt-clear <relative-path>`, leaving harness-specific interrupt keys, Vim/Neovim prompt recovery, idle/closed waiting, and the final clear retry in the binary-owned operator path. If the binary finds the live pane already idle with `prompt_ready=true`, it must skip the interrupt key sequence and proceed directly to the clear retry so the standalone `Interrupt and Clear Session Context` action cannot perturb an idle Codex pane into an editor or other terminal mode. It is available both from busy/protected clear-refusal notifications and as the standalone `Interrupt and Clear Session Context` IDE action. It is the only action that intentionally discards protected prompt input.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>`, copies the exact output, and keeps the binary-owned diagnostics text available for bug reports.
- Plugin verification must cover exact session-status display, `session clear` command wiring, and persistent route-failure retention for stage-specific dispatch diagnostics.

### Safe Sync Surface

- JetBrains startup must not run automatic `agent-doc resync` or `resync --fix`. Session repair/audit is explicit operator action only, because startup audits can traverse large process/session graphs and make the IDE unresponsive.
- Editor-driven layout syncs report absolute file paths to the binary sync surface, preserve empty column placeholders for mixed markdown/non-markdown splits, and keep cross-root markdown siblings in the reported layout even when the focused file lives in a nested submodule.
- When the visible markdown layout spans multiple nested agent-doc roots, JetBrains uses the workspace root `.agent-doc/` as the sync project root instead of the focused file's nearest submodule root. This keeps shared `state.db` column memory stable when focus moves from a workspace session doc to an unmanaged spec/doc file inside a child repo.
- The Rust binary owns passive autostart, ambiguity handling, remembered-column restoration, and tmux window targeting.
- JetBrains handles a session-document rename or move through a retained Lazily path-transition projection. It publishes new-path liveness before old-path removal, waits for the existing Project Controller's convergence receipt, then registers/swaps the new-path replica before retiring the old forwarder. The exact liveness frame is retained across enqueue/flush failure. This path does not launch the CLI and does not invoke layout sync, so the existing pane, window, and horizontal/vertical layout remain untouched.
- JetBrains CRDT replica IPC uses the Project Controller socket (`.agent-doc/controller.sock`) with the controller `crdt_replica` envelope. It must not connect to per-session supervisor sockets for replica register/update/pull/projection/deregister/current-text work.
- A CRDT replica refresh captures the current IntelliJ `Document` only as an expected-editor-text generation fence, then registers and swaps a replacement opened from the controller bootstrap. Pending local work or editor drift rejects the swap without replacing the cached member. The retained Lazily target is projected downstream after registration; no editor text-adopt or full-state request surface exists.
- JetBrains drains remote CRDT deliveries from targeted `EditorIntent` events and the Lazily-backed controller subscription. It must not watch a filesystem event directory or run a fixed interval remote-update pull loop.
- Remote CRDT delivery into the editor is backpressured by one keyed RelayCell hot head per document. The merge retains the oldest guarded editor baseline and newest converged text. Every document has one FIFO replica worker (with idle thread timeout), so attach, normalization, local deltas, remote drain, and projection work stay ordered within that document without blocking another open document.
- After integrating the coalesced remote frontier, JetBrains publishes one full visible-state hash. The controller derives the cumulative represented prefix from that hash; the plugin keeps no pending-ACK sidecar and sends no replay request.
- Before mutating a clean document, the plugin refreshes that target file's VFS stamp and rechecks that the document stayed clean. Novel external disk text rejects the delivery without mutating or overwriting it. IntelliJ's ordinary save lifecycle produces the separate persistence projection.
- JetBrains turn-state projection is event-driven, cached, and read from the Project Controller `state_subscribe` Lazily projection. It does not read filesystem state for ordinary turn-state UI. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`, with no fallback authority. Projection drains cap each work slice and yield between backlog slices so bursts cannot monopolize a plugin worker or indirectly starve the UI.
- Prompt steering is Project Controller-owned. JetBrains must not treat stale supervisor freshness as a local editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.

### Action Promoter

- `AgentDocActionPromoter` ensures `AgentDocPopupAction` (Alt+Enter) takes priority over the built-in `ShowIntentionActions`.

### Logging

- Uses `com.intellij.openapi.diagnostic.Logger` (IntelliJ platform logger).
- Enable debug output: `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`.
- Output appears in `idea.log`. No temp files.

#### Layout sync diagnostics

Navigation/tab-switch layout decisions are traceable end to end across these prefixes:

- `[layout-detect]` (`LayoutDetector.detectEditorLayout`) — the **editor-side input** to every sync. Logs the editor window count, each window's on-screen position and selected `.md` file (`x=… y=… file=…`), and the resulting column grouping (e.g. `grouped into 2 column(s): [left.md] | [right.md]`). This is the line that explains *why* a navigation produced an N-editor-pane layout: a 2-column grouping is what makes sync provision two editor panes plus the agent-doc pane. Detection failures are logged via `LOG.warn` instead of being silently swallowed.
- `[layout-sync]` (`EditorTabSyncListener`) — `selectionChanged`, micro-coalesced command-plane `focus`, the single surface-graph observation enqueue (including the columns derived from `[layout-detect]`), plus debounce/guard/deferred/timeout/generation diagnostics. One editor click's selection and component-focus notifications collapse into one project-scoped intent; inactive or expired intent is refused, component focus alone does not enqueue surface reconciliation, and `LayoutChangeDetector` only reports structural changes into the authoritative graph.
- `[sync:PHASE]` (controller-owned tmux-router reconciler) — `GLOBAL` window+pane state at sync-start/sync-end and `SELECT`/`ATTACH`/`DETACH`/`REORDER`/`VERIFY`/`SWAP` phases. The `DETACH` phase names the reason each pane is kept or stashed (last pane in window, protected busy pane, registered to another session, stashed/broke, focus-steal), which explains *why* an extra pane survived into the visible window.

Binary auto-start forensics also land in `/tmp/agent-doc-sync.log` and the per-document `.agent-doc/logs/ops.log` (`[sync] auto-started %XX for <file>` plus batch summaries and per-phase latency).

### Dynamic Lifecycle

- `PluginLifecycleListener` handles `projectOpened`/`projectClosing`.
- Startup root discovery and native listener registration run on the pooled application executor, not the IDEA event-dispatch thread. This keeps fallback scans away from UI startup when Linux inotify watches are exhausted.
- Editor layout/window snapshots are captured on the event-dispatch thread, while command-plane/native work runs in background executors. The native generation bridge rejects event-dispatch-thread calls so a busy controller cannot freeze IDEA.
- Project close and plugin unload dispose CRDT replica, patch watcher, layout detector, and visual highlighter resources.

## Keybindings

| Action | Default Shortcut |
|--------|-----------------|
| Run | `Ctrl+Shift+Alt+A` |
| Fix Document | none |
| Claim | `Ctrl+Shift+Alt+C` |
| Sync Layout | `Ctrl+Shift+Alt+L` |
| Popup Menu | `Alt+Enter` |
| Prompt Select | `Alt+1..9` |
| Prompt Toggle | `Alt+Esc` |
| Prompt Dismiss | `Esc` |

## Context Menu

Run, Fix Document, Claim, Compact Exchange, Sync Layout, Show Session Status, Recycle Supervisor, Restart Agent, Stop Agent, Clear Session Context, Interrupt and Clear Session Context, and Copy Session Diagnostics are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run, Fix Document, Claim, and session operator actions)
