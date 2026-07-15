# JetBrains Plugin Specification

Extends `editors/SPEC.md` with JetBrains-specific behavior.

## Plugin Metadata

- **ID:** `com.github.btakita.agent-doc`
- **Name:** Agent Doc
- **Restart:** Not required (`require-restart="false"`)

## Implementation Details

### Claim — Split Position Detection

Two strategies for detecting the file's position in the editor split:

1. **Splitter tree walk:** Get the `EDITOR` component from action context, walk the Swing `Splitter` tree to determine if it's in the first child (left/top) or second child (right/bottom).
2. **Window index fallback:** If no EDITOR context (e.g., context menu), enumerate `FileEditorManagerEx.windows`, find which window contains the file, determine position from the Splitter tree or use window index + orientation as heuristic.

### Prompt Panel

- Rendered as a `JLayeredPane` overlay at `POPUP_LAYER` — no `JDialog` (avoids WM leaks and focus-loss dismissal).
- Uses IDE editor font via `EditorColorsManager`.
- Single-row, non-wrapping layout. The panel height is fixed to one prompt row and does not grow on narrow windows.
- Question and option labels truncate with ellipsis; full text is preserved in tooltips.
- Secondary detail (hotkeys, pending-count context) lives in tooltip/secondary UI instead of the main prompt row.

### Tab Sync Listener

- Registered as `FileEditorManagerListener` in `plugin.xml`.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Single-document tab-selection changes submit `agent-doc.focus_document_pane.v1` through the Project Controller command plane from the focused document's own project root. The plugin must not select tmux panes directly; the Project Controller owns the active-window guard and pane selection. Every document in one project uses the same latest-wins focus idempotency key with supersession enabled, so rapid A → B → C tab changes coalesce to C rather than queueing three independent selections.
- A fresh editor selection holds a short editor-focus intent lease while that coalesced focus command crosses the controller. The tmux-to-editor poll suppresses the previously focused pane during the lease, acknowledges the intended pane without reopening the editor, and restores normal tmux-to-editor following if the focus command does not converge before the lease expires.
- Reverse tmux-to-editor focus sync reads the Project Controller-owned `tmux_focus_state` projection through the Project Controller socket. That projection yields a document only when the configured tmux session's current window is `agent-doc`, so switching to another tmux window must not recall the editor selection from the stale active pane in the hidden agent-doc window. If the active pane still has an exact route-owned process-tree binding but its actor projection was pruned, the controller reports that bound document instead of `active_pane_unbound`; foreign-root and ambiguous owners remain unbound. Before `Claim for Tmux Pane` starts a CLI claim, JetBrains records the current tmux-focused document as already seen; if the claim fails and tmux focus remains on the previous pane, the reverse focus mirror must not reopen that previous document over the editor-selected claim target.
- Split-layout tab-selection changes submit `agent-doc.sync_tmux_layout.v1` with `no_autostart=true` through the Project Controller command plane so a selected visible document can be rescued back out of stash into the `agent-doc` window without replacing a live owner. That passive path should hand off quickly by preferring the matching pane first, then an alive exclusive registered pane, and only cold-starting when neither exists.
- Visible markdown set changes submit `agent-doc.sync_tmux_layout.v1` with `no_autostart=true`, reusing the workspace-root chooser for cross-root layouts. A single-file JetBrains selection snapshot is authoritative and must not let sync expand from remembered columns, which prevents stale panes such as `tasks/software/corky.md` from returning after the editor has switched away from them.
- Dedup state tracks the visible markdown signature plus the active file so repeated selection churn does not rerun the same command, but a real markdown `selectionChanged` event must still run the guarded background reconciliation even when the signature is unchanged. The immediate focus fast path can only select an existing pane; the background `sync --no-autostart` pass is what safely cold-starts a missing actor/supervisor for a reopened document such as `tasks/software/corky.md`.
- Automatic tab sync may coalesce a short burst of selection events, but it must not suppress the first deliberate left/right split selection after a prior sync.
- During `selectionChanged`, JetBrains must treat the event's `newFile` as the authoritative active markdown target for dedup/snapshot planning instead of trusting `selectedTextEditor` to have already switched inside the same callback.
- If a newer automatic selection/layout request lands while another automatic sync is still running, JetBrains must queue only the latest request and replay it immediately after the running command finishes instead of dropping the event.
- If that older running sync finishes with a retryable preserved-layout or sync-lock-contention result after a newer request exists, JetBrains must not schedule a delayed retry for the older snapshot. It should let the older process finish silently and replay only the latest selected document.
- On a real markdown selection event, JetBrains should also start a best-effort immediate Project Controller command-plane `focus_document_pane` handoff for the event's selected document before debounce or sync-lock acquisition. A closed/missing actor projection may yield to the latest open session-log pane only when that pane still runs a live agent and its process tree exactly owns the selected document; bare-shell and cross-document reuse fail closed. Missing/dead panes and inactive-window terminal receipts are Project Controller-owned diagnostics; the guarded command-plane `sync_tmux_layout` reconciliation remains the authoritative background layout repair.
- If a Project Controller-backed manual `Sync Tmux Layout` terminal outcome later reports that the current layout was preserved because a visible protected pane could not detach yet, the command projection/log must retain the protected pane id, open-cycle phase, and document path so the user can tell which pane is delaying sync. Current controller builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- Manual `Sync Tmux Layout` submits `agent-doc.sync_tmux_layout.v1` to the Project Controller command plane
  with `no_autostart=false` and returns after admission; the controller runs the full sync path and repairs
  window order before reconciliation: `0:agent-doc`, `1:stash`, then adjacent
  overflow `N:stash` windows. Automatic tab sync stays on
  `agent-doc.sync_tmux_layout.v1` with `no_autostart=true` and must not perform that repair step.
- If passive `agent-doc sync --no-autostart ...` output from an older build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must treat that as deferred rather than complete for both the generic `[sync] sync preserved...` marker and the safe-passive `[sync] safe passive sync preserved...` marker: leave dedup state unchanged and schedule bounded retries until the requested selection applies or a newer request supersedes it.
- If a passive sync terminal outcome contains `[sync] safe_passive_sync_lock_contention_retry`, JetBrains must treat the command as deferred, keep the dedup state unchanged, and retry the newest pending automatic selection/layout request rather than waiting for the CLI's full sync-lock budget. Manual `Sync Tmux Layout` uses Project Controller command supersede/admission instead of a long-lived editor-side native sync guard.
- JetBrains split-editor focus follows both editor focus-gained events and editor mouse-press activation because Swing focus is not guaranteed to change on every click between already-open split editors. Consecutive events for the same markdown path are deduped locally, but alternating paths such as A -> B -> A must each attempt the Project Controller focus handoff.
- The structural layout-change detector owns the editor-side sync guard for the lifetime of its background CLI process; it must build and run the exact-visible `agent-doc sync --no-autostart` command directly instead of calling a helper that reacquires the same guard and self-skips. If a newer generation arrives while that guarded command is running, the detector replays the newest structural snapshot after unlock.
- JetBrains must bound every plugin-spawned automatic sync subprocess and Project Controller-backed manual sync call. On timeout or failure, it releases the plugin-local sync guard, logs the failure, and leaves automatic tab-sync dedup state unchanged so the latest queued selection or a manual retry can run sync again. A dead or externally killed tmux pane may make one sync attempt stall, but it must not leave the IDE permanently reporting `Sync deferred: another tmux layout sync is already running`.

### Prompt Poller Removed

- JetBrains must not start a defensive `PromptPoller` / `PromptPanel`, poll `agent-doc prompt --all`, auto-save tracked documents, or run timer-based merge/reload logic from prompt handling.
- Permission prompts remain in the owning agent/tmux surface.

### Run Feedback

- `Run Agent Doc` saves only the active markdown document and immediately dispatches a Project Controller `editor_route` request carrying dispatch-only, plain-trigger routing with a 15-second ready wait. The editor path does not block on the typing debounce and does not save unrelated open documents; the active document save is the editor-owned flush boundary before the controller route request runs. Even after `Clear Session Context`, this action still sends the plain `agent-doc <FILE>` reopen into the live session instead of restarting Codex.
- Every `Run Agent Doc` click writes a durable attempt ledger entry for click receipt, active document save, `editor_route` request construction/start, retry/dedupe, and terminal route outcome. The ledger may persist diagnostic route shape and route output summaries, but it must not persist raw document prompt text; prompt/trigger proof is represented by byte counts and hashes in binary/controller diagnostics.
- Repeating `Run Agent Doc` while an `editor_route` request is still in flight coalesces with that request instead of canceling and recreating route/controller work. The duplicate click is recorded as deduped and gets an already-dispatching hint; a fresh click is eligible as soon as the bounded request completes.
- That JetBrains request-level coalescing must not leak into controller admission for an already-authorized operator reopen. `managed_reopen` and `dispatch_only_reopen` bypass stale same-generation in-flight dispatch receipts so a fresh `Run Agent Doc` request cannot settle successfully without reaching the pane; automatic/non-operator redispatches remain coalesced.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the JetBrains action layer before route/clear work is submitted. Repeated Run clicks coalesce with the first alive `editor_route` request; `Clear Session Context` preempts a still-dispatching Run by canceling the in-flight request and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear completes synchronously. A clear accepted for deferred delivery by the binary does not release the queued Run immediately.
- If route fails only because the authoritative actor is still in its startup window, `Run Agent Doc` performs one short retry before surfacing the final route failure. This includes dispatch-only `latest run is still booting ... (timed_out)` results; active-turn blockers get a still-running notification, and protected-input blockers such as shell history search are not retried. Repeated clicks while that bounded retry is active coalesce with it.
- A stale startup record must not keep a settled actor in the boot wait. If the requested pane's authoritative actor is `Ready`, dispatch-eligible, and the pane has a recognized busy or interactive blocker, routing exits startup probing and applies the normal busy/queue policy.
- If route succeeds by queueing a prompt behind an already-busy authoritative actor instead of injecting a duplicate trigger, `Run Agent Doc` must surface a visible queued/still-running warning rather than treating the route request as silent success. Repeating `Run Agent Doc` after editing that same prompt must replace the sole live route-owned `agent:queue` prompt instead of leaving stale wording queued behind the active turn. The plugin accepts the new `active agent:queue` route diagnostic and the older `agent:queue auto` wording for compatibility.
- If route refuses a frontmatter `agent:` harness switch because a live authoritative actor is still bound to the previous harness and route defers to the supervisor boundary restart, `Run Agent Doc` must classify that output as a typed recovery state instead of a persistent route failure. Paused-queue defers show an information notification with `Restart Supervisor and resume`; not-ready panes that require `restart-supervisor --force` show `Interrupt and restart`; otherwise the notification offers `Restart Supervisor`. All variants keep `Show status` and `Copy details`.
- When Codex hook tracking is installed, `Run Agent Doc` must not report success from a live reroute that only proved tmux acceptance. If the bare reopen was accepted but Codex never records routed submission proof, the binary must fail once with that exact stage-specific reason and must not precede it with an optimistic success/progress line.
- If the binary exhausts its bounded submit or dispatch-start proof window for `Run Agent Doc`, it must also file a deduped `#jbrunautobug #agent-doc-bug` item in the session document backlog. The item must include the saved route diagnostic path, failure class, document, stage, pane, best-effort actor generation, editor attempt id, dispatch proof, and `ops.log` marker/path; repeated failures for the same document/stage/failure append evidence to the existing item instead of creating duplicate backlog work.
- When the same `agent-doc <FILE>` reopen is already drafted in a Codex/Claude composer, the binary must press one bare `Enter` instead of appending a duplicate trigger. A visible trigger with a later idle prompt below it is stale scrollback and must not receive an Enter.
- A delayed direct-pane resubmit may press `Enter` only while the pane still exposes a recognizable harness surface. It must preserve the draft and refuse injection after the harness has exited to a bare shell.
- The action is silent on route progress/success. Failures are logged to the IDE Event Log / notification tool window instead of showing bottom-right balloon popups.
- A failed route persists the exact `editor_route` output under `.agent-doc/state/editor-route-errors/` and the notification exposes copy/open actions so startup-miss and pending-drift diagnostics remain inspectable after the toast moment. Typed recovery states such as queued, paused, protected prompt input, dispatch-start-unproven, busy/running, and actor-switch defer do not persist generic route-error files. A later successful `Run Agent Doc`, binary route, or focused sync for that document deletes the saved route-error file so the editor cannot keep showing an obsolete startup/proof failure after route recovery.
- Route session targeting follows the same root-aware chooser as sync: a nested document reroute uses that file's nearest `.agent-doc` root, while a mixed-root visible layout stays pinned to the shared workspace root instead of the focused child repo.

### Patch Application Safety

- JetBrains defers socket and file-watch patch application until the target markdown document has been idle long enough for the typing debounce. If the bounded wait times out, the plugin logs the timeout, does not mutate the document, and retries file-watch patches after another debounce window. Socket patches fail closed so the CLI can retry or fall back through the binary-owned closeout path.
- JetBrains handles socket `save_document` messages as typed editor-owned saves. The handler waits for typing idle, locates the open IntelliJ `Document`, calls `FileDocumentManager.saveDocument(document)`, and publishes the saved text through `agent_doc_editor_content_applied_for_editor_v1` with `lazily_transport_receipts_v1` for the supplied `patch_id`. It must not write `.agent-doc/ack-content`, replace document text, use VFS binary writes, or revive reconnect reread repair on this path. Missing lazily receipt support is an incompatible plugin/native-library version error.
- Component patches must honor an explicit JSON `op` (`replace`, `append`, or `prepend`) ahead of the component marker's `patch=` / `mode=` attribute. Binary-side live-buffer convergence relies on `op: "replace"` to repair an `agent:exchange patch=append` body through the Document API instead of appending a second response and forcing a stale-cache disk fallback.
- JetBrains must not apply a socket or file-watch patch while IntelliJ has a pending File Cache Conflict for that document. The plugin treats the conflict as a terminal editor-side refusal for that IPC payload: socket IPC returns failure, file-watch IPC records `file_cache_conflict_pending`, deletes the queued patch file, refreshes agent-doc visual highlighting, and leaves the response for the binary-owned retry path. It must not retain a conflict-deferred patch id, wait for the dialog to resolve, or replay the old payload after the user keeps memory changes or later accepts filesystem changes. The binary-side follow-up contract for older already-written Cancel-shaped closeouts remains: if the response already reached the working tree/snapshot but not `HEAD`, the next `agent-doc preflight` auto-commits the missing boundary rather than surfacing the older manual `write --commit` recovery requirement.
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
- When the visible markdown layout spans multiple nested agent-doc roots, JetBrains uses the workspace root `.agent-doc/` as the sync project root instead of the focused file's nearest submodule root. This keeps shared column memory (`.agent-doc/last_layout.json`) stable when focus moves from a workspace session doc to an unmanaged spec/doc file inside a child repo.
- The Rust binary owns passive autostart, ambiguity handling, remembered-column restoration, and tmux window targeting.
- JetBrains CRDT replica IPC uses the Project Controller socket (`.agent-doc/controller.sock`) with the controller `crdt_replica` envelope. It must not connect to per-session supervisor sockets for replica register/update/pull/ack/deregister/current-text work.
- A forced CRDT replica refresh must reconcile any retained deferred-write target, compare-and-swap those exact bytes into the visible IntelliJ `Document` under the non-operator mutation guard, and only then register/seed/swap the replacement replica. Pending local work or editor drift rejects the refresh without replacing the cached member. The plugin must never register a target-only replica while leaving stale text visible, because that split can replay pre-response boundaries or unconsumed queue state after an otherwise successful ACK.
- JetBrains drains remote CRDT deliveries from editor events and `.agent-doc/crdt-replica-events/*.json` watcher events. It must not run a fixed interval remote-update pull loop.
- Remote CRDT delivery into the editor is backpressured by one keyed RelayCell hot head per document. The merge retains the oldest guarded editor baseline, the newest converged text, and the union of acknowledgements represented by that text. A single `invokeLater` mutation is admitted per EDT turn; the replica worker must never wait on `invokeAndWait`, and it must not pull or realign the same document while its coalesced EDT mutation is pending. No-op drain backoff schedules one coalesced delayed retry instead of sleeping on the single replica executor, so a 30-second idle backoff cannot hold local delta forwarding behind it.
- JetBrains turn-state projection is event-driven, cached, and read from the Project Controller `state_subscribe` lazily projection. It must not read or watch `.agent-doc/turn-scope/*.json`, `.agent-doc/state/cycles/*.json`, or any other sidecar for ordinary turn-state UI; sidecars are reserved for crash recovery or a documented exceptional path where they must be used. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`, with no sidecar compatibility fallback. Projection drains must cap each work slice and yield between backlog slices so a burst of editor or Project Controller events cannot monopolize a plugin worker or indirectly starve the UI.
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
- `[layout-sync]` (`EditorTabSyncListener`) — `selectionChanged`, immediate `focus`, the automatic-sync `exec:` command (including the `--col` editor-layout args derived from `[layout-detect]`), the `result:` line, plus debounce/guard/deferred/timeout/generation diagnostics.
- `[sync:PHASE]` (tmux-router reconciler, stderr; merged into the `[layout-sync] result:` line because `runCommandWithTimeout` redirects stderr) — `GLOBAL` window+pane state at sync-start/sync-end and `SELECT`/`ATTACH`/`DETACH`/`REORDER`/`VERIFY`/`SWAP` phases. The `DETACH` phase names the reason each pane is kept or stashed (last pane in window, protected busy pane, registered to another session, stashed/broke, focus-steal), which explains *why* an extra pane survived into the visible window.

Binary auto-start forensics also land in `/tmp/agent-doc-sync.log` and the per-document `.agent-doc/logs/ops.log` (`[sync] auto-started %XX for <file>` plus batch summaries and per-phase latency).

### Dynamic Lifecycle

- `PluginLifecycleListener` handles `projectOpened`/`projectClosing`.
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
