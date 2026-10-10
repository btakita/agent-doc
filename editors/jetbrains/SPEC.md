# JetBrains Plugin Specification

Extends `editors/SPEC.md` with JetBrains-specific behavior.

## Plugin Metadata

- **ID:** `com.github.btakita.agent-doc`
- **Name:** Agent Doc
- **Restart:** Required for every package upgrade on builds with asynchronous classloader retirement; compatible builds may upgrade dynamically
- **Native upgrades:** Safe in-process generation handoff
- **Distribution:** The classic and exact-262 updates share this ID but use distinct versions, disjoint compatibility ranges, and separate validated custom-repository listings; see [`docs/reference/jetbrains-distribution.md`](../../docs/reference/jetbrains-distribution.md)

## Implementation Details

### Document lint inspection

**Inspect Document Directives** runs `agent-doc lint <FILE>` for the focused
Markdown document. The command is read-only and resolves the same authoritative
editor content and lint policy as closeout gates. Clean results use a transient
hint; warnings and blocking findings are retained in the Agent Doc Event Log so
the full rule, position, and hint remain available even when a compact/write
failure notification is intentionally concise.

IDE notifications render their content as HTML, so every Agent Doc notification
escapes the plain-text message (`&`, `<`, `>`, `"`) and turns line breaks into
`<br>`. CLI text such as a lint hint's `preset=<value>` placeholder must reach the
operator verbatim instead of being swallowed as an unknown tag (GH #227).

When registration carries a retained native CRDT state vector, a controller may
force a full canonical bootstrap for a durable projection without discarding the
vector. The response reports whether canonical causally covers that frontier.
JetBrains may resolve a retained-projection ambiguity by adopting canonical only
when the visible buffer still equals its settled shadow and coverage is explicitly
true. Missing or false proof keeps the operator buffer and retry hold intact.

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

A reload intent whose target library is the loaded build (same mtime), or a
build that already failed validation, is satisfied before any handoff work: no
listener or replica is quiesced (`#steerreplicachurn`). Quiescing tears down
every open document's CRDT replica, and a repeated `reload_library` fan-out for
one install used to deregister and re-register all of them every 10-60 s while
the native generation never changed. Replicas are rebuilt after a reload attempt
only when its quiesce actually disposed the replica managers; a checkpoint that
missed its deadline returns before disposing anything, so the live replicas are
left attached rather than force-refreshed.

Before native reload reattaches any CRDT replica, JetBrains republishes each
open session document's reliable-liveness registration with the current plugin
classloader endpoint identity. Existing local presence tags are retained; if
the local liveness graph was rebuilt, presence is reopened before registration.
The controller may therefore keep its strict superseded-endpoint admission
fence: a delayed old classloader cannot reclaim replica membership, while the
current classloader is admitted without depending on a duplicate-open edge.

The package omits `require-restart`, so JetBrains may unload the plugin and
replace its classloader during an update. A plugin-owned project service is the
parent disposable for every programmatic startup listener, including VFS
subscriptions, and releases the project's static manager registries. No
listener may use the longer-lived project itself as its disposable parent. An
application service additionally cleans every still-open project during plugin
unload.
Application-global workers and JVM hooks are generation resources, not project
resources. Each such owner registers an idempotent closer when it is first
created. After the generation is fenced and every project is disposed, unload
closes those resources in reverse creation order. In particular it cancels the
current-document reporter, controller-socket watchdog, and route-attempt ledger;
quiesces, drains, and terminates the four native-call workers off the EDT; closes
the native handle only when quiesce and drain are proven; and removes the JVM
shutdown hook that would otherwise root the outgoing classloader. Once this
registry is closed, late work may neither create another worker nor reload the
native library. Repeated unload cleanup is a no-op.
Generation-owned projections that must be looked up from extension callbacks
use plugin-static per-project registries, not IntelliJ light-service lookup:
project containers can retain a light-service adapter by implementation class
name while a replacement classloader is activating, which would return an old
generation instance to new bytecode. Unload cleanup removes and disposes every
registry entry before the replacement initializes open projects.
On compatible builds, an unload leak is a defect and JetBrains' explicit
unload-failure prompt is the package-update restart fallback. Builds that retire
plugin classloaders asynchronously cannot satisfy agent-doc's synchronous
retirement proof: agent-doc deliberately declines dynamic replacement, stages
every package upgrade, and requires an IDE restart to load it. An IDE that
already loaded a package generation declaring `require-restart="true"` also
requires a restart before the dynamic lifecycle can govern later upgrades on a
compatible build.

Package selection is per target IDE. The classic package
(`agent-doc-jetbrains-<version>.zip`, `editors/jetbrains`) covers builds
242-261; the modular package (`agent-doc-jetbrains-262-<version>.zip`,
`editors/jetbrains-262`) covers exactly 262. `agent-doc plugin install|update
jetbrains`, the `agent-doc upgrade` installed-plugin reconciliation, and `plugin install jetbrains --local
--all-installed` derive each target's platform build from its versioned IDE data
directory (`IntelliJIdea2026.2` -> 262) and select only that range's asset or
local ZIP. A target whose build cannot be proven, or is outside both ranges, is
refused with guidance; `--local --all-installed` resolves every target before
replacing any, so one unprovable target or a missing modular build changes
nothing. Both archive roots install into the canonical `agent-doc-jetbrains/`
tree, and a stale `agent-doc-jetbrains-262/` tree is removed so one plugin ID
never has two on-disk roots. `make install-editor-plugins` builds both packages
before the all-installed convergence.

Restart-free dynamic upgrade is a classic-line capability
(`#jb262dynupgrade`). Only the classic plugin jar carries the
`JetBrainsPluginUpgradeBootstrap` `Main-Class`/`Agent-Class` launcher, and that
launcher replaces one classic `agent-doc-jetbrains/` tree. The exact-262
package is a Plugin Model v2 split-mode distribution (descriptor-only
`agent.doc-<version>.jar` plus unversioned `lib/modules/agent.doc.*.jar`, with a
frontend half that may run in a separate JetBrains Client) and ships no
launcher. When a live IDE owns a 262 target, the installer therefore recognizes
the package line from its `agent.doc-<version>.jar`, does not attempt an attach,
replaces the plugin files on disk, and reports restart-required with the reason
"the exact-262 modular JetBrains package has no restart-free dynamic upgrade
entry point" (`ops.log`: `declined_by=modular_package`). It never reports the
262 package as one that "predates restart-free dynamic upgrade support"; that
wording is reserved for a classic jar without a `Main-Class`. The live-jar
probes (`#pluginbyteidentity` superseded-bytes detection, the controller's
`editor_route` admission, the install's `#jbdynamicfalsereport` load proof and
the `#pluginactivationprobe` loaded-generation reading) match both versioned
plugin jars, `agent-doc-jetbrains-<version>.jar` and `agent.doc-<version>.jar`,
and never the unversioned module jars.

Local package convergence compares every ZIP payload byte and relative path
with the installed plugin tree before replacing it. A byte-identical package is
a true no-op: the installer leaves the existing files and inodes in place so a
live IDE does not retain deleted mappings of the same generation. For a changed
package, the installer discovers live JetBrains JVMs and attaches the packaged
system-classloader upgrade bridge. The bridge first verifies that the live
plugin root belongs to the installation being updated, then explicitly invokes
the outgoing generation's open-project cleanup hook before unload. This cleanup
must stop every document listener, CRDT worker, and static project registry; two
plugin-classloader generations must never observe the same IntelliJ `Document`,
because each would classify the other's remote projection as operator input and
rebroadcast it. For the first upgrade from a generation predating the public
cleanup hook, the bridge calls its compatible Kotlin companion cleanup entry
point reflectively. A missing or failed cleanup is fail-closed. The bridge then
unloads the current descriptor with JetBrains' explicit update semantics (`disable=false`,
`isUpdate=true`). It must prove that descriptor is no longer loaded before
calling JetBrains' dynamic install-and-load API with the complete ZIP, and it
must reject an identity- or classloader-equal result. This keeps the plugin set
on one live generation across the unload, on-disk replacement, and activation
sequence; the replacement must initialize every already-open project and synchronously
reattach each eligible open document before the installer reports success. The
installer must verify the loaded version, those live replica receipts, and the
final package bytes. It
may replace the directory directly only when every discovered IDE reports that
it does not own that plugin root. Attach or dynamic-unload failure is fail-closed
and must not be papered over by unlinking the JAR behind the live process.
This package lifecycle is independent of the native-library handoff.

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

- “Show Tab in New Window” is a first-class editor surface, not another project. The listener resolves each `EditorWindow` through its AWT frame to JetBrains' persisted `ToolWindowPane.paneId`, reports only the windows owned by that editor surface, and includes that stable `surface_id` in structural and focus-only observations. The controller retains projections by `(project_root, client_id, surface_id)`. A closed detached frame is retired by exact surface id; it must not retire sibling frames or the whole plugin generation. Mouse/focus and structural AWT ingress include every live editor surface, not only the main `EditorsSplitters` tree.
- The IDE-hosted agent terminal uses a dedicated `Agent Doc Terminal` tool window so relocation never moves unrelated stock Terminal tabs. After the exact controller focus receipt applies, the controller's `terminal_decision` may mount the dedicated tool window in one `ToolWindowPane`, keep it there, exclude another frame, or stash it. Internal pane-placement APIs are capability-probed; absence leaves terminal presentation untouched. Plugin/project disposal unregisters the dedicated tool window and clears retained placement.

- Registered once per IntelliJ project from `PluginLifecycleListener`; `plugin.xml`
  owns only the project lifecycle listener.
- Returning to an IntelliJ frame through an application-activation event is a
  visible-surface source edge. Window-manager workspace changes can reconstruct
  or resize the terminal tool window without emitting editor selection or split
  events, so activation republishes the bounded-settled surface and lets the
  controller repair tmux automatically.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Selection settling reads visible membership from each restored editor window rather than
  `FileEditorManager.selectedFiles`, whose aggregate can lag behind the individual split
  selections during restart. The restored window selections and detected split columns must
  contain the event's new document before publishing. If one advances before the other,
  bounded later-EDT reads continue; on exhaustion, the exact event old→new edge repairs each
  stale projection independently, so current window membership cannot mask stale one-column
  geometry.
- Agent-document `fileOpened` is also a visible-surface source edge. IDEA may restore split
  containers before their files and then emit neither a selection nor another
  structural event, so the listener retains the opened file as projection intent
  across bounded later-EDT reads. A surface is not published while any restored
  editor window still has no selected file; the completed file lifecycle edge
  republishes the settled surface without invoking tmux directly.
- Every agent-document selection and visible-layout change reports one editor-surface observation containing the focused document, visible documents, open documents, and detected columns. Session-document classification reads the live editor buffer and requires agent-doc frontmatter; a plain Markdown plan is never a desired tmux target. When a non-session tab is selected, that split retains its last classified agent document and the event republishes spanning layout only. Layout, file-open, and IDE-activation observations set `preserve_focus`: they may reconcile pane membership but cannot derive `Focus`, update controller focus tracking, or attach a focus target to structural sync. This keeps a background project/root publication from overwriting a newer explicit focus projection in a mixed-root split surface. An agent-document `selectionChanged` event carries pane-focus authority only when its file is selected in JetBrains' active editor window; background-split selection remains a layout-only observation and omits the event file as preferred focus. The selection callback captures only this typed authority plus its event file and schedules projection; a later EDT pass snapshots immutable editor state, while project-root discovery, filesystem access, patch-watch registration, JSON construction, native transport, and controller delivery run on a serialized background executor. Native transport adds an ordered reload generation/cursor and publishes it to an existing controller. The controller's process-scoped reactive graph derives idle or layout reconciliation and owns the tmux effect. One spanning editor surface has exactly one active controller-root subscription. Candidate roots are recovered from all open session documents on every publication; before admitting the replacement observation, the listener requests same-family retirement for incompatible roots. The controller stops every retained `jetbrains-pid` generation no newer than the requester, so a prior JVM cannot contract the tmux surface while the replacement controller is still reconciling, while a delayed old-JVM retirement cannot stop the replacement. A failed pre-publication forget stays tracked and is retried after publication and on the next observation. Active selection and component-focus also publish one generation-fenced `focus_only` surface Source to the focused agent document's own controller root under a distinct `jetbrains-focus-pid` family. The controller graph derives `Focus` and must never infer structural `Sync` from that deliberately narrow payload. When its exact receipt reports a stashed, dead, reaped, missing, or non-focusable owner, the adapter generation-fences the receipt and republishes the complete spanning surface as a forced focus-authoritative structural edge; only that layout graph may restore the owner and reconcile pane cardinality. Component-focus also republishes the spanning surface directly. This separate retained handoff is required when one visible editor surface spans a superproject document and a submodule document. The plugin never chooses a pane or tmux window, and a missing-owner outcome installs no reverse-focus suppression lease.
- An observation with no detected columns is structurally `unknown`, not a one-column layout.
  It may select the focused document's existing pane, but it cannot add, stash, reorder, or
  provision panes. A proven local single editor window publishes one column. The presence of
  Remote Dev client editor-manager sessions selects the remote evidence path even when the
  backend happens to expose an incidental local window (for example a source/Find Usages window);
  backend window count is not remote-layout authority. Remote client-selected files name only the
  focused document, so the plugin reports per-client `visible` (the Remote Dev editor tracker's active client editors),
  `selected`, and `open` session documents to the shared native fold
  `agent_doc_editor_surface_resolve_remote_layout_json` (GH #134). The plugin snapshots those
  client/editor collections and the backend-focused files on the EDT, then runs the fold from the
  generation-fenced surface delivery worker; direct synchronous detection also marshals only the
  immutable fold off the EDT. The native bridge's EDT guard therefore remains intact and a
  selection observation can use retained split memory instead of silently taking the memoryless
  fallback (GH #157). One client naming two or
  more visible documents is a detected split; `selected` remains separate focus evidence and is
  never unioned into that visible set. Remote Dev can leave the text half of a hidden
  Editor/Preview tab active, so when the sole visible-set addition is also the newly selected
  document, the fold retains the established width and replaces the previously focused column
  instead of appending (GH #175). Every successful `[layout-detect] observed` line records each
  client's raw `visible`, `selected`, and `open` lists. A later single-selection observation also
  retains the known split, and documents whose tabs closed drop out. Only with no split
  evidence and no retained split does the observation stay `unknown`. The controller's coalesced ingress receipt joins
  `layout=unknown|observed|focus_only` to `pane_action=none|focus_only|structural_sync` on one
  line so every pane effect names its authority.
- When a Remote Dev observation has the same width as retained memory and exactly one document
  leaves while one enters, the new document inherits the dropped document's column slot. The
  client `visible`/`open` lists prove membership, not geometry, so this memory-derived result is
  published as `column_order=retained`, never `editor` (GH #185). A retained publication is
  already resolved and passes through controller ordering unchanged.
- Explicit layout publications (`Sync Tmux Pane`, claim, resync, and the Run Agent Doc route's
  `--col` list) name their order source as `column_order` (GH #112). Only locally detected
  multi-column geometry is `editor`; the undetected fallback lists `selectedFiles`, which IntelliJ
  orders focused-window first, so it is `unknown`. The controller resolves an `unknown` order
  against the order it already retains for those documents (`agent_doc_tmux::order_layout_columns`):
  focus never reorders columns, and a newly visible document appends. On a Remote Dev backend
  whose detector stays `unknown` (no single client reports a multi-file split set, GH #97) tmux
  pane order is therefore stable but is **not** a mirror of the editor's split order; agent-doc
  has no input from which to derive it. `pane_layout_desired_published` and every
  `pane_layout_projection` line record `caller_kind` and `column_order=editor|retained|unknown`.
- Manual `Sync Tmux Layout` publishes `exact_visible=true` only when layout detection returns
  known columns. If columns are unreadable, it refuses before terminal preparation or controller
  publication and tells the operator that the retained tmux layout was not changed; a focused-only
  fallback must never narrow a retained multi-column surface to one column (GH #154).
- A fresh editor selection is authoritative for the next projection even if the split component tree is briefly interstitial and still exposes the opposite editor as selected. When a tab-chrome click initially looks like a background-split selection, the listener performs one generation-fenced authority check after IntelliJ's focus manager reports settled focus; it emits focus only if that exact file has become the active editor window while the project frame is active. A generic next-EDT check is insufficient because real split/tab transfers can span multiple EDT turns. This closes the opposite-tab event gap without promoting programmatic or genuinely background selections. Component-focus republishes the spanning surface when no selection is pending, but cannot replace a pending document-selection observation; the shared generation guard still collapses repeated focus callbacks.
- The listener first re-reads an interstitial selection projection on bounded later EDT turns. If it still contains the old selected file when that budget is exhausted, the listener applies the selection event's old-to-new file edge to the stale visible/layout projection before publishing; it never synthesizes a replacement unless the old event file is actually present.
- Reverse tmux-to-editor focus sync is installed at project startup and reads the Project Controller-owned `tmux_focus_state` projection through the Project Controller socket. That projection yields a document only when the configured tmux session's current window is `agent-doc`, so switching to another tmux window must not recall the editor selection from the stale active pane in the hidden agent-doc window. If the active pane still has an exact route-owned process-tree binding but its actor projection was pruned, the controller reports that bound document instead of `active_pane_unbound`; foreign-root and ambiguous owners remain unbound. An actually focused editor component remains authoritative over background controller focus changes. The IDE frame alone is not focus authority: selecting a pane in an embedded terminal leaves the frame active but permits the corresponding visible editor document to be selected, including a document from another project root in the same split surface. A hidden foreign-root target remains suppressed. A reverse mirror may select a different editor tab but must pass `focusEditor=false`, so it cannot reactivate the JetBrains desktop window after the operator moves to another i3 window. Before `Claim for Tmux Pane` starts a CLI claim, JetBrains records the current tmux-focused document as already seen; if the claim fails and tmux focus remains on the previous pane, the reverse focus mirror must not reopen that previous document over the editor-selected claim target.
- The plugin applies only a short event-storm debounce and generation guard before reporting the final surface. Its separate focus lane keeps only an in-memory generation and a micro-coalescing delay, then publishes retained focus state to the Project Controller; it does not submit an editor command. The controller-owned Lazily graph supplies transition deduplication and the exact selection effect receipt. The plugin stores no previous layout signature, last-focused file, pending retry, or durable controller copy.
- A `sync` projection is the product of columns and focused document. The controller first reconciles the passive layout, then applies the requested pane through a generation-fenced effect. Matching columns or a successful `select-pane` receipt alone cannot retire the projection: observation must show the focused document's actor pane active in the target window. A repeated foreground command republishes a physical observation even when its desired value is identical, reactivating the retained effect after focus or geometry drift. A newer surface generation cancels stale focus before it reaches tmux.
- Safe-passive exact-visible reconciliation is atomic across stale-supervisor admission. If any requested editor column is gated while its pane recycles, automatic sync preserves the current tmux layout instead of realizing a narrower subset; the recycle-settled generation republishes the complete surface. Manual `Sync Tmux Layout` keeps full repair authority and may realize a non-empty admitted remainder.
- The retained focus projection may surface a proven live pane from stash inside
  the controller's latest-wins focus fence, then selects only after a live-window
  recheck. The listener models that controller call as a bounded request/response
  effect: the request carries the focus generation admitted with its spanning
  surface observation, and the response is interpreted under the lifecycle lock.
  Any newer admitted surface observation invalidates and interrupts the in-flight
  request. The socket has a one-second deadline as a transport backstop, so a slow
  controller cannot block newer focus work. Only a current successful response may
  install a focus lease or, for `actor_pane_not_visible`, republish the complete
  editor surface with forced reconciliation. Stale, timed-out, and failed responses
  are inert. The full surface remains exact-layout authority; other focus failures
  do not trigger layout repair. The controller classifies
  `actor_pane_not_visible` from the selected pane's own window before applying the
  active-window guard: a stashed selected pane therefore requests layout repair,
  while a pane already in the `agent-doc` window still reports
  `outside_agent_doc_window` when the operator is viewing another tmux window.
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
  any failure is shown with the controller diagnostic. The manual action uses the
  same live-buffer session classification and ignores non-agent Markdown tabs;
  if the focused split currently shows one, its last classified agent document
  remains the requested member. Automatic editor-surface
  sync publishes the same autostart-capable desired layout with
  `caller_kind=automatic`, but completes at desired-state admission; the binary
  owns safe cold-start checks, ambiguity refusal, and inactive-desktop focus
  suppression. Visible restored files may therefore acquire missing panes
  without an imperative republish, while merely open background files remain
  outside the desired columns.
- A native-library generation handoff has one shared five-second CRDT-worker
  shutdown deadline across all open projects. Operator-triggered Compact
  Exchange and manual layout sync wait on the handoff completion edge instead
  of observing the intentional manager/listener removal window. After endpoint
  restart, the coordinator republishes every open project's current editor
  surface so tab/focus-to-tmux synchronization resumes without another click.
- If passive `agent-doc sync --no-autostart ...` output from an older build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must treat that as deferred rather than complete for both the generic `[sync] sync preserved...` marker and the safe-passive `[sync] safe passive sync preserved...` marker: leave dedup state unchanged and schedule bounded retries until the requested selection applies or a newer request supersedes it.
- If a passive sync terminal outcome contains `[sync] safe_passive_sync_lock_contention_retry`, JetBrains must treat the command as deferred, keep the dedup state unchanged, and retry the newest pending automatic selection/layout request rather than waiting for the CLI's full sync-lock budget. Manual `Sync Tmux Layout` uses Project Controller command supersede/admission instead of a long-lived editor-side native sync guard.
- JetBrains split-editor focus follows editor focus-gained events, editor-content mouse activation,
  and mouse presses anywhere in the owning project's editor-split component tree. The last source
  includes tab chrome and preview/custom editors, which can change the active editor window without
  emitting either of the narrower callbacks. An editor-tree press schedules a latest-wins next-EDT
  probe and publishes only the then-current active window's selected session document; presses
  outside the project tree, callbacks after disposal, superseded callbacks, and callbacks while the
  project frame is inactive are inert. The process-wide AWT listener is paired with project/plugin
  disposal. Consecutive events for the same markdown path are deduped locally, but alternating paths
  such as A -> B -> A must each attempt the Project Controller focus handoff.
- Local operator splices are retained immediately but published after a 250 ms trailing quiet window, coalescing human typing bursts into one ordered durable push, broadcast, and settled projection while preserving the existing per-document retry fence.
- The structural layout-change detector only reports a new editor surface observation into `EditorTabSyncListener`; it has no second CLI planner, lock, or tmux process. The JetBrains/native boundary validates and enqueues that observation without waiting for a controller probe or tmux consequence. One delivery worker serializes publications and controller-root handoff off the EDT; each controller's process-scoped graph replaces any not-yet-started surface with the newest one. Tab selection and structural changes therefore share one latest-wins surface graph, while component focus uses only the separate selection lane, without letting a blocked controller call freeze or disable the IDE bridge.
- A failed editor-surface publication remains generation-fenced retry work. The
  listener retries the latest observation with capped exponential backoff (100,
  200, 400, 800, 1600, then 2000 ms) so a controller recycle or brief socket
  refusal cannot permanently disable automatic tmux pane synchronization. A
  successful publication resets the retry counter, and a newer generation
  supersedes every older retry.
- JetBrains bounds every command-plane automatic and manual sync call. On timeout or failure, it releases the plugin-local request guard, logs the failure, and leaves automatic tab-sync dedup state unchanged so the latest queued selection or a manual retry can run again. A dead or externally killed tmux pane may delay one request, but it cannot leave a second editor-side sync owner wedged.

### Prompt Poller Removed

- JetBrains must not start a defensive `PromptPoller` / `PromptPanel`, poll `agent-doc prompt --all`, auto-save tracked documents, or run timer-based merge/reload logic from prompt handling.
- Permission prompts remain in the owning agent/tmux surface.
- Interactive Codex, Claude, and OpenCode prompts are detected by the supervisor and projected as
  controller-stream `input_required` state. JetBrains shows an important notification and editor
  banner with a `Focus Agent Terminal` action on the false-to-true transition; it does not poll or
  become a second answer authority.

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
- Native visual-token projection classifies the complete binary-authored
  `> **Queue prompt:**` blockquote as `prompt`, including its continuation quote
  lines. JetBrains therefore applies prompt styling consistently to queue turns
  emitted by Codex, Claude, and OpenCode without harness-specific parsing.
- `Run Agent Doc` saves only the active markdown document and immediately dispatches a Project Controller `editor_route` request carrying dispatch-only, plain-trigger routing with a 15-second ready wait. The request's complete `--col` / `--focus` payload includes empty physical split placeholders and is an exact-visible layout boundary: the controller canonicalizes those facts, may materialize a missing focus only into one unique empty split, and converges the result before dispatch so route cannot append a pane beside stale visible state. The editor path does not block on the typing debounce and does not save unrelated open documents; the active document save is the editor-owned flush boundary before the controller route request runs. Even after `Clear Session Context`, this action still sends the plain `agent-doc <FILE>` reopen into the live session instead of restarting Codex.
- Editor selection never forks `Run Agent Doc` onto another transport. Selected-text invocations and saved-document diff steering use the exact same Project Controller request as an unselected click and submit the same bare `agent-doc <FILE>` trigger; Rust route policy decides whether that starts, replaces, or steers the live turn. Kotlin must not send `selected_text` / `steering_id`, await a turn-steering acknowledgement, or derive actor-state admission independently.
- Every `Run Agent Doc` click writes a durable attempt ledger entry for click receipt, active document save, `editor_route` request construction/start, retry/dedupe, and terminal route outcome. The ledger may persist diagnostic route shape and route output summaries, but it must not persist raw document prompt text; prompt/trigger proof is represented by byte counts and hashes in binary/controller diagnostics.
- Repeating `Run Agent Doc` while an `editor_route` request is still in flight coalesces with that request instead of canceling and recreating route/controller work. The duplicate click is recorded as deduped and gets an already-dispatching hint; a fresh click is eligible as soon as the bounded request completes.
- That JetBrains request-level coalescing must not leak into controller admission for an already-authorized operator reopen. `managed_reopen` and `dispatch_only_reopen` bypass stale same-generation in-flight dispatch receipts so a fresh `Run Agent Doc` request cannot settle successfully without reaching the pane; automatic/non-operator redispatches remain coalesced.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the JetBrains action layer before route/clear work is submitted. Repeated Run clicks coalesce with the first alive `editor_route` request; `Clear Session Context` preempts a still-dispatching Run by canceling the in-flight request and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear completes synchronously. A clear accepted for deferred delivery by the binary does not release the queued Run immediately.
- `Run Agent Doc` submits its controller-owned `editor_route` directly and never
  creates, attaches, selects, focuses, or reveals an IDE terminal tab. The
  controller route owns cold start and preserves the live actor's tmux session.
  An autostarting Sync may call `agent-doc tmux ensure <FILE> --json`; its strict
  JSON receipt is parsed from stdout only, with stderr kept separate. An
  already-attached client remains authoritative even when a surviving IDE tab
  exists, so Sync does not reveal that tab. For a detached session whose host is
  `ide`, Sync may reuse or create one live `agent-doc` tab, execute the
  binary-provided attach command, and then resume. The Terminal plugin remains
  optional; failures expose the exact attach command with a Copy action.
- The accepted async command executes as controller-owned work with the local
  reactive document projection installed. Nested authority reads must not
  self-RPC through the controller socket; the generic 5-second external-client
  deadline is therefore not a terminal `Run Agent Doc` outcome. The binary must
  not retry the whole route after an ambiguous deadline because dispatch may
  already have occurred.
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
- JetBrains has no broad `save_document` or save-all command. The generation-fenced `persist_current` intent enters the same per-document FIFO worker lane as accepted local and remote CRDT updates, so it cannot overtake an earlier update signal. After the lane reaches the command, the intent is accepted only when the post-registration IntelliJ `Document` still matches the requested hash/byte length and its active replica. It calls `saveDocument` without replacing the buffer, verifies the same bytes on disk, and publishes `disk_persisted`; if IntelliJ defers the write behind File Cache Conflict, the later VFS content event publishes the receipt after the user-approved overwrite, provided disk, editor, and the same replica remain exact. A mismatch requests CRDT redelivery and never replaces the editor from disk. Every rejection records a typed reason plus the expected and observed revision evidence so a retained latest-durable intent can retry or be superseded without treating operator typing as an error.
- Replica refresh registers from the controller bootstrap, reports reconnect propagation, and schedules an ordinary canonical-projection drain. The observed editor cut is only a swap fence; it is never seeded or published as a whole-document recovery baseline.
- A successful `replica_pull` with `refused=true, reason=missing_replica` means the cached client belongs to a prior controller generation. JetBrains must classify it as transport invalidation, atomically replace the cached forwarder through registration, and then resume pull/projection. It must not treat that typed refusal as an idle empty update list.
- Ordinary dirty `DocumentEvent`s retain their exact UTF-16 range and old/new fragments, validate that splice against the current replica shadow, translate it to CRDT code-point units, and publish a bounded splice batch. The debounce may batch transport publication but may not reread the whole `Document` and infer one replacement. Whole-text events and clean incremental cache reloads are non-operator projections: they advance the projection epoch, fence queued operator splices, and enter the explicit refresh/recovery path.
- `#splicebaselength` / `#agentpatchlineage`: every captured splice records the Document's UTF-16 length immediately before it and the `patch_id` of the last agent-applied patch (`withAgentAppliedEditorMutation(file, patchId, postText)`, which also records the exact post-patch text hash). A splice whose pre-edit length differs from the shadow at that point is never replayed by offset; a pure insertion has no range text to mismatch, so without this check it lands as many characters away as the shadow is short (live 2026-10-09: an applied response the replica never received shifted a queue line 769 chars into the backlog). When the splices' lineage names the last agent patch and undoing them from a read-locked editor cut reproduces exactly that patch's post-text, the operator splices are transformed through the missing patch (before it: same offset; after it: shifted by its length change; overlapping it: not rebased) and ONLY the operator text is forwarded — the agent text stays the controller's to fold (`#appliedresponsefold`), so nothing is inserted twice, and the editor's visible state is not projected from the operator-only text. Without a proven lineage the splices are held and a remote drain requested until the missing base arrives. An agent mutation without a patch id clears the lineage.
- Component intents must honor an explicit JSON `op` (`replace`, `append`, or `prepend`) ahead of the component marker's `patch=` / `mode=` attribute. Lazily convergence uses `op: "replace"` for an `agent:exchange patch=append` body so recovery cannot append a second response; it never falls back behind an attached editor.
- JetBrains must not apply a socket or file-watch patch while IntelliJ has a pending File Cache Conflict for that document. The plugin treats the conflict as a terminal editor-side refusal for that IPC payload: socket IPC returns failure, file-watch IPC records `file_cache_conflict_pending`, deletes the queued patch file, refreshes agent-doc visual highlighting, and leaves the response for the binary-owned retry path. It must not retain a conflict-deferred patch id, wait for the dialog to resolve, or replay the old payload after the user keeps memory changes or later accepts filesystem changes. The binary-side follow-up contract for older already-written Cancel-shaped closeouts remains: if the response already reached the working tree/snapshot but not `HEAD`, the next `agent-doc preflight` auto-commits the missing boundary rather than surfacing the older manual `write --commit` recovery requirement.
- Every disk change observed while an IntelliJ buffer is open is retained by the binary as an independent pending external-disk candidate. Accepting filesystem changes produces a clean document event; JetBrains asks the shared resolver for the exact candidate, resets and re-registers the replica from the visible buffer, and reports settlement only after successful CRDT propagation. Keeping memory changes never receives or merges the disk candidate: a later edit or save clears it in the shared binary state. Closing the final editor clears the candidate and falls back to disk; closing one of several editor replicas does not.
- JetBrains file-watch IPC must also accept reposition-only patches emitted by commit cleanup (`patches: []`, `reposition_boundary: true`, `preserve_head: true`). These patches are applied through `Document.setText` / VFS APIs, reuse the committed boundary id when provided, and preserve visible ` (HEAD)` response markers so closeout cleanup does not trigger IDE file-cache conflict dialogs.
- `Run Agent Doc` does not use the patch-application typing debounce. It saves the active markdown document and routes immediately; patch/socket mutation paths keep their bounded idle guards.
- `Claim for Tmux Pane` and `Force Claim Current Pane` save only the target document, fail closed if it remains dirty, and temporarily make every editor view of that file read-only while the bounded background CLI mutation runs. Completion refreshes that virtual file synchronously before restoring editor writability, so IntelliJ cannot race user typing against claim scaffolding or surface a File Cache Conflict from the plugin's own action.

### Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in an IDE notification instead of re-deriving status inside the plugin. A successful status response deletes the saved route-error file for that document because the old failure is no longer the latest observed state.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The action ID remains `AgentDoc.RestartSupervisorProcess`. If the binary refuses because the pane is busy or the authoritative actor is still starting, JetBrains must show the typed restart warning with `Interrupt and restart`, `Show status`, and `Copy details` actions; the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. The binary owns live pane status for diagnostics and clear safety: stale busy actor/supervisor projection is reconciled when direct live pane evidence is `alive-idle` with `prompt_ready=true`, including a bottom Codex model/cwd/context footer below older transcript text. A live `agent-doc` wrapper process is not by itself proof that the session is still running; ordinary active/status panes must proceed through the normal clear submit path. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clear clicks report the existing queued clear instead of injecting another `/clear`, and JetBrains surfaces that output as accepted deferred work. Clear may fail closed when the captured pane contains protected prompt input such as a permission prompt, queued draft, shell history search, or drafted user text, or when it shows an explicit busy cue that cannot be deferred such as an active Codex turn, hook-review prompt, or help screen; the operator can then choose the explicit interrupt-clear discard path.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals must show a typed warning with the relevant interrupt action, `Show status`, and `Copy details` instead of the generic command-failed text. Legacy alive-busy and `active_agent_doc` refusals may still use the busy-session warning, but the binary must not emit that warning solely because the live pane is the `agent-doc` wrapper.
- `Interrupt and clear` requires an explicit IDE confirmation and then runs `agent-doc session interrupt-clear <relative-path>`, leaving harness-specific interrupt keys, Vim/Neovim prompt recovery, idle/closed waiting, and the final clear retry in the binary-owned operator path. If the binary finds the live pane already idle with `prompt_ready=true`, it must skip the interrupt key sequence and proceed directly to the clear retry so the standalone `Interrupt and Clear Session Context` action cannot perturb an idle Codex pane into an editor or other terminal mode. It is available both from busy/protected clear-refusal notifications and as the standalone `Interrupt and Clear Session Context` IDE action. It is the only action that intentionally discards protected prompt input.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>`, copies the exact output, and keeps the binary-owned diagnostics text available for bug reports.
- Plugin verification must cover exact session-status display, `session clear` command wiring, and persistent route-failure retention for stage-specific dispatch diagnostics.
- `About Agent Doc` (`AgentDoc.About`, Tools/editor/project-view menus and the popup's More Actions) runs `agent-doc version --json` (falling back to `--version`) with the binary from `TerminalUtil.resolveAgentDoc(projectRoot)` on a pooled thread, reads the loaded native generation through `AgentDocLib.aboutSnapshot()` (canonical path, validated `agent_doc_version()`, `agent_doc_build_info_json()`; never loads or reloads), and shows a dialog with a Copy button. The plugin version is the IntelliJ plugin descriptor version. Mismatch rules follow `editors/SPEC.md` section 6b.
- Binary/native resolution follows the IDE process `PATH` before workspace and home-directory fallbacks, and logs every attempted install. A native load failure names both the executable that supplied `lib-path` and the library; recognizable GNU/musl loader failures are labelled as libc mismatches. Because `admin reload-lib` requires a loaded native endpoint, initial-FFI remedies direct the operator to correct/remove the conflicting install and rely on the loader's automatic retry instead of presenting `reload-lib` as a reachable first step.
- `Agent Doc Dashboard` (`AgentDoc.Dashboard`, `DashboardAction`, `gvqv`) is project-scoped: it runs `agent-doc dashboard --write` from `TerminalUtil.cleanupProjectRoot`, then refreshes and opens `<root>/.agent-doc/dashboard.md` on the EDT. The controller keeps that projection current; `isAgentDocDocumentTextUtil` rejects it, so tab sync, CRDT replicas, and layout sync ignore the open tab. It is listed last in the popup's numbered group. `DashboardActionTest` covers the arguments, path, menu declarations, popup reachability, and session-classification rejection.

### Safe Sync Surface

- JetBrains startup must not run automatic `agent-doc resync` or `resync --fix`. Session repair/audit is explicit operator action only, because startup audits can traverse large process/session graphs and make the IDE unresponsive.
- Editor-driven layout syncs report absolute file paths to the binary sync surface, preserve empty column placeholders for mixed markdown/non-markdown splits, and keep cross-root markdown siblings in the reported layout even when the focused file lives in a nested submodule.
- When the visible markdown layout spans multiple nested agent-doc roots, JetBrains uses the workspace root `.agent-doc/` as the sync project root instead of the focused file's nearest submodule root. This keeps shared `state.db` column memory stable when focus moves from a workspace session doc to an unmanaged spec/doc file inside a child repo.
- The Rust binary owns passive autostart, ambiguity handling, remembered-column restoration, and tmux window targeting.
- An automatic pane-layout generation whose focused document has no file-to-pane assignment settles the exact visible structure without focus and retires. Replaying the same generation cannot create that assignment; a later editor-surface observation owns any structural change. Manual focus, failed `select-pane`, and pane-window co-visibility drift remain fail-closed.
- JetBrains handles a session-document rename or move through a retained Lazily path-transition projection. It publishes new-path liveness before old-path removal, waits for the existing Project Controller's convergence receipt, then registers/swaps the new-path replica before retiring the old forwarder. The exact liveness frame is retained across enqueue/flush failure. When a move crosses a nested project boundary, controller resolution chooses the narrowest available root that contains both old and new paths; it must not retain retries against the new nested controller when that controller cannot own the old identity. This path does not launch the CLI and does not invoke layout sync, so the existing pane, window, and horizontal/vertical layout remain untouched.
- A `<stem>.done.md` archive candidate must never become a path-transition alias for a still-existing canonical `<stem>.md` document. The editor rejects that VFS edge before moving liveness, and the controller independently refuses it before relay rekey or alias publication. A real rename whose old path is absent and an independently addressed `.done.md` document retain their own identities.
- JetBrains CRDT replica IPC uses the Project Controller socket (`.agent-doc/controller.sock`) with the controller `crdt_replica` envelope and includes `ProcessHandle.current().pid()` as `editor_pid`. That process-scoped proof lets a detached agent-doc registration establish editor authority through the relay; the controller must not require authority to pre-exist registration. It must not connect to per-session supervisor sockets for replica register/update/pull/projection/deregister/current-text work.
- A CRDT replica refresh captures the current IntelliJ `Document` only as an expected-editor-text generation fence, then registers and swaps a replacement opened from the controller bootstrap. Pending local work or editor drift rejects the swap without replacing the cached member. The retained Lazily target is projected downstream after registration; no editor text-adopt or full-state request surface exists.
- JetBrains drains remote CRDT deliveries from targeted `EditorIntent` events and the Lazily-backed controller subscription. It must not watch a filesystem event directory or run a fixed interval remote-update pull loop.
- Remote CRDT delivery into the editor is backpressured by one keyed RelayCell hot head per document. The merge retains the oldest guarded editor baseline and newest converged text. Every document has one FIFO replica worker (with idle thread timeout), so attach, normalization, local deltas, remote drain, and projection work stay ordered within that document without blocking another open document.
- After integrating the coalesced remote frontier, JetBrains publishes one full visible-state hash. The controller derives the cumulative represented prefix from that hash; the plugin keeps no pending-ACK sidecar and sends no replay request.
- Before mutating a clean document, the plugin refreshes that target file's VFS stamp and rechecks that the document stayed clean. Novel external disk text rejects the delivery without mutating or overwriting it. IntelliJ's ordinary save lifecycle produces the separate persistence projection.
- JetBrains turn-state projection is event-driven, cached, and read from the Project Controller `state_subscribe` Lazily projection. It does not read filesystem state for ordinary turn-state UI. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`, with no fallback authority. Projection drains cap each work slice and yield between backlog slices so bursts cannot monopolize a plugin worker or indirectly starve the UI.
- Prompt steering is Project Controller-owned. JetBrains must not treat stale supervisor freshness as a local editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.

### Component Folding, Gutter Markers, and Structure View

GH #19 (plugin UX phases 5-7); shared contract in `editors/SPEC.md` § 12.

- Registered in `agent-doc-markdown.xml`, loaded through an optional dependency on the bundled
  Markdown plugin (`org.intellij.plugins.markdown`): `lang.foldingBuilder` and
  `codeInsight.lineMarkerProvider` for language `Markdown`, plus a `lang.structureViewExtension`
  that adds component nodes under the Markdown structure view's file root (the heading outline is
  kept). Without the Markdown plugin none of them load.
- Boundaries come from `agent_doc_parse_components` (`NativePatching.componentSpansOrNull`), whose
  UTF-8 byte offsets `AgentDocComponentOutline` maps to UTF-16 editor offsets. The parse runs on
  `VisualHighlighterManager`'s debounced (120 ms) background refresh with the same text snapshot
  as the visual tokens, and only for a session document (`isAgentDocDocumentTextUtil`); a plain
  Markdown file installs an empty outline, so every extension is inert there.
- `ComponentOutlineStore` (owned by the project's `VisualHighlighterManager`, weak document keys)
  keeps each component as open/close `RangeMarker`s, so between refreshes boundaries track edits
  exactly. The folding builder, line-marker provider, and structure extension read only that store
  and the live document text: no native call, no disk read, no PSI walk per keystroke. A refresh
  restarts the daemon for the file only when the installed outline differs from what the markers
  already track (component added, removed, renamed, or re-bounded). A failed or unavailable native
  parse (library missing, unclosed marker mid-edit) keeps the last outline. Disposal clears the
  store and its markers.
- Folding: each multi-line component folds from its open marker through its close marker to
  `<!-- agent:NAME · N items -->`; nothing is collapsed by default. Gutter: one
  `AllIcons.Nodes.Template` icon on the leaf holding each open marker; the tooltip names the
  component, its item count and inline attributes; clicking toggles that component's fold.
  Structure view: `agent:NAME` nodes with the item count as location, nested components and
  items as children, each navigating to its line.

### Agent Doc Actions popup

- `AgentDocPopupAction` (`AgentDoc.Popup`) defaults to `Ctrl+Shift+Alt+D`. It never uses `Alt+Space` (`#gh116`: Windows consumes it for the window system menu, so the IDE never receives it) in `$default`. The `Default for XWin` keymap adds `Alt+Space` on top of `Ctrl+Shift+Alt+D` (`altshiftmenu`: i3 and other bare X window managers deliver it, and it was the Linux operator's working menu key until #gh116 removed it everywhere); GNOME and KDE keymaps do not get it because those desktops claim Alt+Space. The `$default` shortcut is declared first so the XWin shortcut is added to, not substituted for, the inherited one. In IntelliJ IDEA Ultimate, `Ctrl+Shift+Alt+D` is also the bundled Database grid's `Console.TableResult.CloneColumn`, which is disabled outside a data grid, so it does not compete in a markdown editor. It installs no `ActionPromoter`, so native `Alt+Enter` intentions stay intact. The popup is also in the Tools menu and editor context menu, and like every `AgentDoc.*` action it can be rebound under Settings > Keymap.
- Every declared `AgentDoc.*` action is listed in the popup's primary or More Actions group; `AgentDocPopupActionTest` enforces this and rejects OS-reserved default keystrokes.

### Logging

- Uses `com.intellij.openapi.diagnostic.Logger` (IntelliJ platform logger).
- Enable debug output: `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`.
- Output appears in `idea.log`. No temp files.

#### Layout sync diagnostics

Navigation/tab-switch layout decisions are traceable end to end across these prefixes:

- `[layout-detect]` (`LayoutDetector.detectEditorLayout`) — the **editor-side input** to every sync. Logs the editor window count, each window's on-screen position and selected `.md` file (`x=… y=… file=…`), and the resulting column grouping (e.g. `grouped into 2 column(s): [left.md] | [right.md]`). This is the line that explains *why* a navigation produced an N-editor-pane layout: a 2-column grouping is what makes sync provision two editor panes plus the agent-doc pane. Detection failures are logged via `LOG.warn` instead of being silently swallowed.
- `[layout-sync]` (`EditorTabSyncListener`) — `selectionChanged`, IDE activation, micro-coalesced command-plane `focus`, the single surface-graph observation enqueue (including the columns derived from `[layout-detect]`), plus debounce/guard/deferred/timeout/generation diagnostics. One editor click's selection and component-focus notifications collapse into one project-scoped intent; inactive or expired intent is refused, and `LayoutChangeDetector` only reports structural changes into the authoritative graph. Selection precedence ends as soon as the EDT captures a self-consistent surface: controller delivery is in-flight state, not pending editor observation, so a later genuine focus event can supersede a slow layout realization instead of being discarded.
- `[sync:PHASE]` (controller-owned tmux-router reconciler) — `GLOBAL` window+pane state at sync-start/sync-end and `SELECT`/`ATTACH`/`DETACH`/`REORDER`/`VERIFY`/`SWAP` phases. The `DETACH` phase names the reason each pane is kept or stashed (last pane in window, protected busy pane, registered to another session, stashed/broke, focus-steal), which explains *why* an extra pane survived into the visible window.

Binary auto-start forensics also land in `/tmp/agent-doc-sync.log` and the per-document `.agent-doc/logs/ops.log` (`[sync] auto-started %XX for <file>` plus batch summaries and per-phase latency).

### Dynamic Lifecycle

- `PluginLifecycleListener` handles `projectOpened`/`projectClosing`.
- Startup root discovery and native listener registration run on the pooled application executor, not the IDEA event-dispatch thread. This keeps fallback scans away from UI startup when Linux inotify watches are exhausted.
- Editor layout/window snapshots and document mutations are captured on the event-dispatch thread, while project-root discovery, filesystem walks, patch-watch registration, command-plane/native work, and tmux consequences run in background executors. Remote Dev's zero-window layout path snapshots client-visible, selected, open, and focused session documents on the EDT, but resolves the native retained-split fold on the surface delivery worker. Socket patch apply first captures an immutable document text/stamp proof on the EDT, performs every native replay/component/normalization calculation on the listener worker, then returns only the computed target to the EDT for a proof-fenced minimal edit. The immutable post-write content receipt is published from the socket worker before acknowledgement. The native generation bridge rejects event-dispatch-thread calls so a busy controller cannot freeze IDEA, and an adapter path must not catch that rejection as an ordinary patch failure caused by scheduling native computation inside an EDT closure.
- Project close and plugin unload dispose CRDT replica, patch watcher, layout detector, and visual highlighter resources. Queued visual-refresh callbacks check their manager generation's disposed state before scheduling or applying, and a scheduler-shutdown race is inert rather than an IDE exception.

## Keybindings

| Action | Default Shortcut |
|--------|-----------------|
| Run | `Ctrl+Shift+Alt+A` |
| Initialize Session | none |
| Fix Document | none |
| Claim | `Ctrl+Shift+Alt+C` |
| Sync Layout | `Ctrl+Shift+Alt+L` |
| Agent Doc Actions popup | `Ctrl+Shift+Alt+D` (also `Alt+Space` in the `Default for XWin` keymap) |
| Run with Junie | `Ctrl+Shift+Alt+J` |
| Load Tmux Window | `Ctrl+Shift+Alt+W` |
| Refresh Environment | `Ctrl+Shift+Alt+R` |

All defaults are registered in the `$default` keymap and can be changed under Settings > Keymap (search "Agent Doc").

## Context Menu

Run, Initialize Session, Fix Document, Claim, Compact Exchange, Sync Layout, Show Session Status, Recycle Supervisor, Restart Agent, Stop Agent, Clear Session Context, Interrupt and Clear Session Context, and Copy Session Diagnostics are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run, Initialize Session, Fix Document, Claim, and session operator actions)
