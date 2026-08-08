# Plan: tmux auto-sync latency — reactive conversions (`#tmux-autosync-reactive`)

Reported: "significant delay with tmux auto-sync when switching editor agent-doc
documents. Please optimize all bottlenecks and fix any wedges." + "convert any
imperative state operations into lazily reactives."

## Context

Investigation mapped the full editor tab-switch → tmux pane-focus latency path.
The reactive plane itself is exemplary (`agent-doc-editor-surface/{lib,graph}`,
`idle_revision.rs`, `idle_watch.rs`) — no `#stategraphjoin` islands, single
state-machine cells, derived `Computed`s, gated `Effect`s. The wedges live in the
imperative I/O scaffolding around it.

Shipped in 0.35.145 (this cycle): the `sync --no-autostart` run now opens a
`ProcessObservationScope`, routing per-pane `/proc` ownership reads through the
reactive `owner_graph`. That is the contained, high-confidence reactive
conversion. The items below are the larger hot-path conversions, sequenced.

## Wedges (action-ranked)

### 1. `controller_tmux_layout_for_surface` — synchronous per-observation tmux probe (PRIMARY wedge + imperative-state violation)

> **0.35.146 update:** the contained sub-fix (sequencing item 2) LANDED —
> `pane_session` now reads from the shared `list-panes -a` snapshot
> (`#panesessionsnapshot`), and the controller drift survey opens a
> `begin_pane_snapshot_scope()`, collapsing the N per-pane `display-message`
> spawns to one listing. Item 1 main's scope is **larger than the original fix
> shape below**: the Sync intent effect is **eager** —
> `ControllerEditorSurfaceGraph` constructs with `run_controller_editor_intent`,
> which on `SurfaceIntent::Sync` calls `sync_tmux_layout` synchronously inside
> the `on_intent` effect during `observe` (rpc.rs:21214 asserts the effect ran
> within the `observe` call). So the editor socket request blocks on BOTH the
> survey AND the structural sync. True decoupling therefore requires changing
> the **consequence model** for automatic editor Sync from eager to deferred:
> the intent runner must feed the `pane_layout_graph` (scheduling the existing
> background `pane_layout_effect_worker`) and return immediately, and the
> worker must publish the actual `TmuxLayout` back via a new
> `observe_tmux_for_project` so the surface graph's `tmux` `Source` refreshes
> without the editor request waiting for it. The editor-facing receipt's
> synchronous `outcome` becomes `None` for the deferred path (a contract
> change), and the drift-detection tests in rpc.rs (~21200+) must reconcile.

`agent-doc-controller-io/src/project_controller/rpc.rs:17198` (called from
`handle_editor_surface_observe` at `rpc.rs:16985`), inside the
`editor_surface_graph.observe` mutex (`rpc.rs:248-330`).

For every multi-column (≥2) editor observation, this imperatively surveys tmux
(`tmux_layout_sync_state_for_invocation_with_effect_assignment`, `rpc.rs:17250`)
to fill "the controller's half of the mirror" for drift detection. It spawns:
`Tmux::default_server()` + `session_alive`, per-pane `tmux.pane_session(pane)`
(`rpc.rs:17275-17279`, NOT batched by the pane snapshot scope which only caches
`list-panes -a` liveness), `list_panes_ordered`, and per-pane
`active_pane_process_owner_document`. This blocks the editor socket request
(CONTROLLER_SYNC_TMUX_LAYOUT_TIMEOUT = 35s) and serializes behind the global
editor-observation mutex.

**Violation:** `#reactive-boundary-ingress` / `#lazily-hot-path` — it recomputes
a structural tmux-mirror comparison imperatively per observation, when the
controller already derives `runtime.pane_layout_desired()` and
`runtime.pane_layout_effect_file_panes(desired.generation)` reactively.

**Fix shape (reactive):** fold editor observations against a *retained* tmux
observation `Source` instead of a synchronous probe.
- `handle_editor_surface_observe` calls `EditorSurfaceState::observe` (keeps the
  retained tmux `Source`) instead of `observe_with_tmux` (overwrites it).
- The controller-runtime retains a `TmuxLayout` observation updated by the
  existing background `pane_layout_effect_worker` (`rpc.rs:18006`) when it
  completes a structural effect, and/or a bounded controller-side tmux
  observation. That worker already produces the receipts; publish them into the
  graph via `observe_tmux` (`graph.rs:224`).
- Drift detection then derives from the retained value; the `Sync` intent runs on
  the background worker (as it already does for `caller_kind = "automatic"`), and
  the worker's receipt refreshes the observation. Worst case is one redundant
  background reconciliation if the retained value is one cycle stale — acceptable
  and off the editor hot path.
- Single-column focus path is already exempt (`rpc.rs:17203`); no change there.

**Contained sub-fix (land independently, lower risk):** make `pane_session`
(`tmux-router/src/tmux.rs:751`) snapshot-aware by enriching the pane-snapshot
format to `#{pane_id}\t#{pane_dead}\t#{session_name}` and reading the session
from the cached listing when a scope is open; then open a
`tmux_router::begin_pane_snapshot_scope()` around the controller probe so the
per-pane `pane_session` calls share one `list-panes -a`. This does not remove the
synchronous probe but collapses its N `display-message` spawns to one listing.
Update `parse_pane_dead_flag` / `parse_alive_pane_ids` / the three spawn sites.

**Tests to reconcile:** `controller_tmux_layout_for_surface` and
`handle_editor_surface_observe` drift-detection tests in `rpc.rs` (~21200+).
Add SimWorld coverage asserting an editor observation does NOT issue a
synchronous tmux survey when a retained observation is fresh.

### 2. Editor 500 ms fixed-delay poll — `TmuxPaneFocusSync.kt`

`editors/jetbrains/.../TmuxPaneFocusSync.kt:50-57,177,354-362` —
`scheduleWithFixedDelay(...POLL_MS=500L)` issues `CpRouteClient.tmuxFocusState`
socket RPC per `candidateProjectRoots` every 500 ms regardless of activity.

**Violation:** `#reactive-boundary-ingress` — the controller already publishes the
editor-surface projection on every accepted observation
(`rpc.rs:17004-17016`, `editor_surface_projection_channel`). The plugin polls a
different RPC instead of subscribing to the state-plane channel it already
receives on the focus-projection lane.

**Fix shape:** subscribe to `agent-doc/editor-surface/<hash>/projection/v1`
state-plane frames and drive `selectEditorDocument` from the published projection
instead of a fixed poll. Keep a long-interval (e.g. 5s) fallback poll only as a
reconnect/liveness backstop, not the primary driver. Mirror in the VS Code
adapter if it has an equivalent poll.

### 3. `acquire_sync_lock` sleep-poll — `agent-doc-sync-io/src/lib.rs:138-161`

`SYNC_LOCK_POLL_INTERVAL = 50ms` against a `SafePassive` 100ms budget = 2 polls
floor before any work; contention triggers a full `/proc` reap (`lib.rs:674-715`).

**Fix shape:** use blocking `flock(2)` (or `fcntl` lock) with a watchdog thread
for the budget instead of a 50ms sleep-poll. This is the `#idlerevisionreactive`
analog (event-driven wait vs poll). Lock semantics must be preserved (stale
orphan reaping stays).

### 4. `ControllerEditorSurfaceGraph` — `Mutex<BTreeMap>` over the controller scope

`rpc.rs:103-106,248-330` — per-client machines share the controller `ProcessScope`
(good), but the map is a plain `Mutex<BTreeMap>` and the lock is held across the
fold + synchronous consequence.

**Fix shape:** `ThreadSafeDependencyMap<(PathBuf,String), …>` over the controller
scope with per-client machines materialized as cells (the same shape
`EditorSurfaceState` uses internally for `document_authorities`, `graph.rs:111`).
The retained effect still fires per-client; the mutex goes away. Depends on
item 1 (the synchronous probe is what makes holding the mutex across the effect
expensive).

## Sequencing

1. (DONE 0.35.145) `ProcessObservationScope` in the sync run.
2. (DONE 0.35.146) Item 1 sub-fix: `pane_session` snapshot batching + controller snapshot scope (`#panesessionsnapshot`).
3. (DONE 0.35.147–0.35.153) Item 1 main: retained tmux observation fed by the effect worker. Editor `Sync`/`Focus` intents are now resolved IN-PROCESS (`handle_editor_surface_observe` folds against the retained tmux `Source` at `rpc.rs:17059` and publishes the desired layout through the background `pane_layout_effect_worker`; both intents return immediately and the worker refreshes the `tmux` `Source` via `observe_tmux_for_project`). The editor socket request no longer blocks on a tmux survey or a structural sync.
4. (DONE 0.35.158) `handle_tmux_focus_state` reads the active pane's document from the runtime's reactive actor store (`actor_store_snapshot`) instead of reopening SQLite on every 500 ms JetBrains reverse-mirror poll (`#lazily-hot-path`). Cold start (no runtime) still hydrates from SQLite; the lookup rule is unified in the pure `actor_record_for_active_pane` helper. The tmux active-pane probe itself stays (tmux has no push event for arbitrary pane-focus changes, so the reverse mirror cannot be pure-subscription).
5. (DONE 0.35.157) Item 3: kernel-blocked `flock` — no sleep-poll, no `/proc` reap, `libc` dep removed (`#tmuxautosyncreactive`). Note: this is hygiene only; the editor tab-switch path is uncontended (the `pane_layout_effect_worker` already serializes it), so it does not by itself fix the reported latency — item 1 main does.
6. Item 4: `ThreadSafeDependencyMap`. Lower priority now: the `ControllerEditorSurfaceGraph` mutex is no longer held across expensive work once item 1 main moved the consequence off the editor-observe path.

7. (DONE 0.35.170) **The actual remaining wedge, and it was not reactive at all
   (`#syncdupespelling`).** After items 1–5 shipped, a tab switch was still slow.
   Measuring instead of assuming: `agent-doc sync --col A,B --focus …` reported
   **three** `auto-start check` passes for **two** documents, `latency budget
   exceeded: phase ownership_per_file_loop took 4120ms (budget 750ms)`, and one
   document listed **twice** in `unresolved files remain blocked` — once absolute,
   once relative.

   Cause: `auto_start_candidate_files` (`agent-doc-tmux/src/lib.rs`) deduped on the
   raw `PathBuf`. The column set merges the caller's `--col` args with remembered
   column memory, and the two disagree on spelling — memory retains absolute paths,
   the editor/CLI passes root-relative ones — so one document keyed as two
   candidates and paid the expensive stale-binding fallback (`alive but no longer
   proves ownership` → supervisor process-tree recovery) twice. The sibling
   `projected_sync_pane_count` already canonicalized for exactly this reason.

   Fixed by keying dedup on the canonical path while emitting the first-seen
   spelling. After: two passes, **no budget violation**, no blocked files;
   worst-case switch 2829ms → 802ms, steady state ~550ms → ~450ms.

   **Lesson for this plan:** items 1–5 were all real reactive-plane improvements,
   but none of them could have caught this — the duplicated work was upstream of
   the reactive graph, in how the candidate set was keyed. Measure the reported
   path before converting more of it.

Items 2–6 are independently shippable. Item 1 was the headline *reactive* win;
item 7 was the headline *latency* win.

## Note on Item 2 (editor fixed-delay poll)

The original Item 2 framed the JetBrains `TmuxPaneFocusSync.kt` 500 ms poll as
replaceable by an editor-surface state-plane subscription. That projection
carries the editor→tmux `intent`, not the tmux-active document, so it cannot
drive the *reverse* mirror (`selectEditorDocument`), which fundamentally needs
the tmux-active pane and therefore a tmux probe (tmux exposes no push event for
arbitrary pane-focus changes). The achievable, high-confidence reactive
conversion for the reverse mirror is step 4 above: serve the poll from the
controller's reactive actor store instead of SQLite. A pure-subscription
replacement remains out of reach until a tmux focus-change hook exists.

## Follow-ups (out of plan scope)

- **`#replicarefusalstorm` fallback: a "take over wedged endpoint" operation (`#replicaendpointtakeover`).** When `reload-lib` genuinely cannot reach a live editor endpoint (e.g. the endpoint's native listener is wedged beyond the reload-compat path), the controller refuses `replica_register` as `detached_authority` forever — the editor can't re-attach through the very RPC that would attach it. Add a controller operation that, for a proven-live editor PID, accepts a `replica_register` as the attach trigger (flipping the doc to `MultiReplica`) instead of refusing it for not being attached, with the existing `#replica-structure-guard` + `#qdelauth` backstops catching dangerous stale content. This is the fallback for the case the 0.35.157 reload-receipt fix (`IPC_RELOAD_LIBRARY_RECEIPT_TIMEOUT_SECS`) does not cover (a listener that won't accept the reload at all). Lower priority than the latency work.
