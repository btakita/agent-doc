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
2. Item 1 sub-fix: `pane_session` snapshot batching + controller snapshot scope.
3. Item 1 main: retained tmux observation fed by the effect worker.
4. Item 2: editor poll → state-plane subscription.
5. Item 3: blocking flock.
6. Item 4: `ThreadSafeDependencyMap`.

Items 2–6 are independently shippable. Item 1 is the headline latency win.
