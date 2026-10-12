# Plan: independent tmux navigation per JetBrains installation (`#ideriderindependentnav`)

Status: P1 landed on branch `ridernav`; P2a landed on branch `ridernavp2a`; P2b landed on
branch `ridernavp2b`; P3 landed on branch `ridernavp3`; P4's explicit handoff landed on branch
`ridernavp4`. P5's migration, lane cleanup, docs, and two-IDE end-to-end test landed on branch
`ridernavp5`; the lane awareness carried over from P3/P4 is still open (listed under P5).

## Problem

IntelliJ IDEA and Rider are often open on the same repository. Since
`#idearidereditors` (9796d8c8a), each IDE has its own installation-scoped editor
client family (`jetbrains@<productCode>.<config-dir hash>-pid:<pid>`), so text and
CRDT state converge in both. Navigation is still shared: both IDEs publish into one
Project Controller root, and that root owns one tmux `agent-doc` window.
`#idefocusowner` (fd04ad845) stopped the window flipping between the two IDEs'
layouts by letting only the most recently focused installation drive it. That
removes the thrash, but the two IDEs still cannot navigate independently. Focusing
Rider re-projects Rider's layout over IDEA's.

Goal: each installation gets its own tmux navigation surface. Its layout, focused
pane, and reverse tmux-to-editor mirror are independent of the other IDE's. The
current `#idefocusowner` behavior stays as the single-IDE path and the default.

## Key decision: a lane is a tmux session, not a window

A *navigation lane* is the unit of tmux navigation authority. Each lane is backed
by its **own tmux session** (`<project session>-<installation>`). It is not a second
window in the shared session, for three reasons:

1. tmux keeps one current window per session, shared by every client attached to
   it. The two IDE terminals attached to one session would always show the same
   window. Grouped sessions (`new-session -t`) do get separate current windows, but
   they still share every window's active pane.
2. A pane belongs to exactly one session. The `ghmultitmux` policy already
   guarantees that sync never moves a pane across sessions
   (`drop_columns_bound_to_other_sessions`), and `start` and `claim` already respect
   an allowed-session list. Pane ownership per lane therefore reuses a boundary that
   already exists and is already tested.
3. Each lane session keeps the canonical `agent-doc` and `stash` windows. The
   roughly 40 window-name sites in `agent-doc-sync-io` stay valid unchanged, because
   they are all scoped to a session.

Session name: `NavigationLane::tmux_session_name`. The shared lane is the
configured `tmux_session`. An installation lane is `<pin>-<installation>`, with
every character outside `[A-Za-z0-9_-]` mapped to `-` (for example
`agent-loop-IU-0a1b2c3d`).

## Modes and the single-IDE fallback

- `NavigationMode::Shared` is the default and is today's behavior. Every client of
  a root maps to the shared lane, and `#idefocusowner` decides which installation
  drives it. A single IDE behaves exactly as before in both modes.
- `NavigationMode::PerInstallation` gives each installation-scoped client its own
  lane. Each lane has exactly one installation, so the focus-owner fold for that
  lane always lets it drive, and nothing is ever deferred. Unscoped clients (VS
  Code, a pre-`#idearidereditors` JetBrains plugin) stay on the shared lane, which
  keeps the `#idefocusowner` rule.

## Phases

Each phase can ship and be verified on its own. Every commit keeps `make check`
green.

### P1: lane identity in the layout-ownership key, and `tmux_focus_state` split per lane (LANDED)

- `agent-doc-editor-surface/src/navigation_lane.rs` (pure): `NavigationMode`
  (`shared` / `per_installation`, plus `parse`), `NavigationLane` (`Shared` /
  `Installation(id)`, plus `for_client`, `tmux_session_name`, `label`), and
  `LayoutOwnerKey { root, lane }`.
- Controller (`ControllerEditorSurfaceGraph`): `focus_owners` is keyed by
  `LayoutOwnerKey` instead of the root. `retain_live_installations` folds live
  installations per lane, so retiring one lane's installation never re-stamps
  another lane's owner. The graph carries a `navigation_mode`. Production
  constructs `Shared`; `with_navigation_mode` is `#[cfg(test)]` until P2.
- `tmux_focus_state` is per lane. The request's `caller` is the editor client id.
  `tmux_focus_state_for_lane` reports the active pane of that lane's session, and
  reports a document only while that session's current window is `agent-doc`. A
  lane whose session is missing is `tmux_session_not_alive`, never a fallback onto
  the shared session. `ControllerTmuxFocusState.navigation_lane` is set for an
  installation lane and omitted for the shared lane, so the wire shape is
  unchanged.
- SimWorld: `src/sim_world.rs` `two_ide_navigation_model` drives the production
  `InstallationFocus` fold and `LayoutOwnerKey` and checks every event sequence of
  depth 5. In shared mode, only the focus owner reshapes the shared session between
  handoffs. In per-installation mode, nothing is deferred and no lane session is
  ever reshaped by another installation. The model detects three mutations: owner
  keyed by root, every lane projected onto the shared session, and focus authority
  ignored.
- Behavior: none in production, because every client resolves to the shared lane.
  Migration: none, because no serialized state changed.

### P2a: per-lane pane-layout graph map (LANDED)

- `ControllerRuntime.pane_layout_lanes` (`ControllerPaneLayoutLanes`) holds
  `BTreeMap<NavigationLane, Arc<ControllerPaneLayoutGraph>>`. The `Shared` entry is
  created eagerly with the runtime; an installation lane's graph is created on first
  use (`pane_layout_graph_for_lane`) in the same controller `ProcessScope` graph and
  derives from the same live actor bindings, so an actor rebind retires that lane's
  ownership too. Each lane has its own `desired`/`observed`/`receipt`/`assignments`/
  `width_memory`, retained effect, sink slot, and `published_generation`.
- Every existing caller goes through `ControllerRuntime::pane_layout_graph()`, the
  shared lane, so behavior is unchanged. Nothing in production creates an
  installation lane yet.
- Verification: `navigation_lanes_hold_independent_pane_layout_state` (two lanes'
  desired generations, width memory, and structural ownership stay independent; the
  shared lane is untouched; a lane graph follows actor rebinds). It fails when every
  lane resolves to the shared graph. Existing layout tests are unchanged apart from
  `runtime.pane_layout_graph` becoming a method call.
- Open for P2b: the effect-worker sink is still installed only on the shared lane,
  and `set_structural_effect_generation` is still process-global (see Risks).

### P2b: opt-in config, lane routing, and lane session provisioning (LANDED)

What landed (the original scope follows below):
- Config: `ProjectConfig.editor_navigation` (raw spelling) and
  `agent_doc_project_config_io::editor_navigation_mode{,_for_root}` (unknown =>
  `shared` plus the rejected value, which the controller logs as
  `editor_navigation_mode_rejected`). `ControllerRuntime::new` reads it once and
  `with_navigation_mode` is no longer test-only.
- Effect workers: `ControllerPaneLayoutLanes` holds a sink factory; every lane graph,
  including lanes created later, gets its own `ControllerPaneLayoutProjectionSink`
  and latest-wins worker. The worker takes its lane and binds
  `set_structural_effect_generation` to that lane graph's published generation, so
  the cancellation generation is keyed by lane.
- Routing: `handle_editor_surface_observe` publishes a `Sync` through
  `publish_pane_layout_desired_invocation_in_lane` into the client's lane graph
  (shared lane is byte-identical; installation lanes skip the shared durable column
  memory and the shared status channel). The worker resolves `pane_layout_lane_route`:
  identity in shared mode; in per-installation mode an installation lane needs a
  pin, provisions `<pin>-<installation>` (`agent-doc` + `stash`, marked
  `@agent_doc_navigation_lane=<root>`, unmarked same-named sessions refused) and
  targets its `agent-doc` window id. The survey (`tmux_layout_sync_state_for_lane_invocation`)
  and target-window fallback (`pane_layout_target_window_id_for_lane`) observe the
  lane session. `observe_tmux_for_lane` refolds only that lane's clients.
- Ownership: `partition_lane_layout_columns` / `LanePaneMembership` (pure, editor-surface)
  drop other lanes' documents in the controller (`cross_lane_layout_dropped`) and again
  in the sync body after column memory (`drop_columns_owned_by_other_navigation_lanes`).
  Editor focus on another lane's pane is refused (`owned_by_other_lane`) and never
  escalates; an installation lane's focus escalation republishes its own lane.
  `TmuxSessionPolicy::with_navigation_lanes` allows lane sessions derived from an allowed
  session under `tmux_sessions`.
- Verification: SimWorld `two_ide_navigation_model` tracks each document's pane session
  (every document contended by two IDEs) and checks that no pane changes session; the
  `MoveAcrossLanes` mutation is detected. Unit tests cover config parsing, per-lane
  sink/cancellation, lane routing, lane-only refold, policy derivation, and the sync-body
  guard; `make tmux-ci` adds isolated-server tests for lane-session provisioning (and the
  never-adopt refusal) and the sync-body guard. Shared mode's existing tests are unchanged.
- Not yet covered (moved to P3/P5): an end-to-end isolated tmux test with two lanes,
  two layouts, and alternating heartbeats through the real sync body; how the first sync treats
  the empty shell pane a new lane session starts with (not exercised end to end yet); a per-lane
  `pane_layout_status` wire channel; and route/start/claim lane awareness (a new pane for
  a document selected in a lane starts in that lane only through the layout sync path).

Original scope:

Scope (Rust, controller and sync):
- Config: `.agent-doc/config.toml` `editor_navigation = "shared" | "per_installation"`
  (`agent-doc-project-config-io`, parsed with `NavigationMode::parse`). An unknown
  value logs a warning and falls back to `shared`. The runtime constructor passes
  the mode to `ControllerEditorSurfaceGraph`, which removes `#[cfg(test)]` from
  `with_navigation_mode`.
- Install the pane-layout effect sink on each lane graph as it is created, and key
  the structural-effect cancellation generation by lane (the per-lane graphs
  themselves landed in P2a).
- The editor-surface `Sync`/`Focus` path passes the observation's lane.
  `publish_pane_layout_desired_invocation(lane, ..)` sets `invocation.window` to the
  lane session's `agent-doc` window, so the `Explicit` session authority applies.
  `observe_tmux_for_project` becomes `observe_tmux_for_lane` and refolds only that
  lane's clients with that lane's survey.
- Lane session provisioning happens as an `Effect` on the first driving `Sync` of a
  lane: `new-session -d -s <lane session> -n agent-doc -c <project root>`, with the
  receipt published back to a `Source` and no caller retry. `TmuxSessionPolicy`
  implicitly allows a lane session derived from an allowed pin.
- Pane ownership per lane: a document's pane lives in exactly one session. Sync for
  lane L includes only panes in L's session, or panes newly started for L. A
  document whose pane lives in another lane's session is dropped from L's layout
  and logged as `cross_lane_layout_dropped document=.. owner_lane=..`, mirroring
  `cross_session_layout_dropped`. Focus for it returns `owned_by_other_lane` and
  does not escalate to a structural sync. A new pane for a document selected in L
  starts in L's session. Stash never moves a pane across sessions.
- Verification: an isolated tmux test with two lanes and two different layouts
  under alternating heartbeats asserts that each session's `agent-doc` window
  layout stays stable and that no pane changes session. The existing
  `#idefocusowner` tests still pass in shared mode. Extend SimWorld with pane
  membership per session (the invariant that no pane changes session without a P4
  handoff).

### P3: editor wiring (FFI-first, then JetBrains) (LANDED)

What landed (the original scope follows below):
- Focus read: `project_controller::tmux_focus_state_for_client` sends the client id as
  `caller`; FFI `agent_doc_tmux_focus_state_for_client_json(project_root, client_id)` wraps
  it (thin). The old entry point keeps the shared lane. Without a runtime the focus handler
  reads the configured `editor_navigation` instead of assuming `shared`.
- Terminal attach: `agent-doc tmux ensure <file> --ide-terminal --client-id <id>`
  (`agent_doc_start_io::ensure_tmux_session_for_ide_client`). The project session is ensured
  as before; in per-installation mode an installation-scoped client's receipt then targets
  its lane session (`resolution=navigation_lane`), provisioned on first use through
  `project_controller::navigation_lane_session_for_client`, the same `ensure_navigation_lane_session`
  the controller's first driving layout uses (never adopts an unmarked session). The attach
  command is rendered by the same policy (including an operator `attach_command` template).
- Plugin publications: a `pane_layout_desired` frame carries `client_id`; the state-plane
  sink keeps one latest-wins projector per lane (`PaneLayoutDesiredLaneProjector`) and
  publishes into that client's lane graph, so the manual Sync Tmux Layout arranges the
  requesting IDE's own lane and two IDEs' frames never coalesce each other away. A lane frame
  is marked seen on the shared graph so a route's plane catch-up never waits for it.
- JetBrains: `CpRouteClient.tmuxFocusState` sends `caller` = its surface client id
  (`tmuxFocusStateRequest`); `paneLayoutDesiredStatePublishRequest` adds `client_id`;
  `IdeTerminalCoordinator` passes `--client-id`; `TmuxPaneFocusSync.focusStateIsOwnLane`
  drops a projection labelled with another installation's lane. VS Code needs no change:
  its clients are unscoped (`vscode-pid:`), so they are always on the shared lane, and it
  keeps the shared FFI and ensure.
- Verification: Rust FFI test (lane label per client and mode, no tmux); controller tests for
  frame lane resolution and lane-routed plugin publications (two lanes, no coalescing, shared
  untouched, plane catch-up); isolated-tmux tests (`make tmux-ci`) for client lane-session
  provisioning, the IDE terminal ensure receipt per client and mode, and the end-to-end two-lane
  case through the real sync body: freshly provisioned lane sessions, alternating syncs with
  different layouts and a contended document, each window's layout stable round to round, no
  pane changing session, the shared session untouched, and each lane session's initial empty
  shell pane stashed inside its own lane on the first sync. Kotlin tests for the request shapes,
  the ensure command, and the lane filter.
- Compatibility: the plugin now passes `--client-id` to `agent-doc tmux ensure`; a binary
  older than this phase rejects the flag, so plugin and binary must ship together.
- Still open (moved to P4/P5): a per-lane `pane_layout_status` wire channel; lane awareness
  of route, start, claim, the `sync_tmux_layout` command path and the recycle republish
  (all still publish into the shared lane).

Original scope:

- FFI: `agent_doc_tmux_focus_state_for_client_json(project_root, client_id)` (the
  old entry point keeps the shared lane). The `ide_terminal` attach outcome takes the
  client id and returns `tmux attach -t <lane session>`, so each IDE's agent-doc
  terminal attaches to its own lane session.
- JetBrains: `CpRouteClient.tmuxFocusState` sends `caller` = its surface client id.
  `TmuxPaneFocusSync` ignores any projection whose `navigation_lane` is not its own
  installation, as defense in depth for "focus mirroring goes only to the IDE that
  owns the window". The coordinator bumps the plugin version per edit.
- Verification: Kotlin unit tests for the request shape and the lane filter, plus a
  Rust FFI test.

### P4: contended documents (one document visible in both IDEs) (LANDED: explicit handoff)

What landed (the original scope follows below):
- Pure (editor-surface): `LaneClaim` (`Passive` / `Explicit`, from `caller_kind`: only `manual`
  is explicit) and `partition_lane_layout_columns_for_claim`, which under an explicit claim keeps
  another lane's document and lists it in `LaneLayoutPartition.handoffs` instead of `dropped`.
  `LANE_HANDOFF_CALLER_KIND` (`lane_handoff`) is the passive kind of a post-handoff republish;
  `lane_owning_session` maps a session back to a known lane.
- Controller: `route_pane_layout_desired_to_lane` returns the handoffs (document, pane from the
  actor binding, owner session). Before the structural sync the lane worker runs
  `hand_over_navigation_lane_pane` for each: both sessions must be lanes agent-doc owns (the pin
  or a session marked `@agent_doc_navigation_lane` = root; `#bare-foreign-session-guard`), the pane
  must still be in the claimed session (else `pane_moved`; already in the lane is a no-op), an
  empty shell is left behind when it was the last pane of the old lane's `agent-doc` window, and
  the pane moves into the requester's `stash` window (`stash_pane`), from where the lane's sync
  places it. A refusal drops the document as a passive sync would
  (`navigation_lane_handoff_refused`). The lane the pane left is republished from its retained
  layout as a fresh `lane_handoff` generation with `no_autostart` (publisher `LaneHandoff`, a
  derived width authority), so it re-projects without the document and can never take it back.
  Ownership is where the pane lives (its id and actor binding are unchanged), so no separate
  transfer record is written; ops.log carries `navigation_lane_handoff` and
  `navigation_lane_handoff_republished`.
- Sync body: the lane guard is re-applied after a passive focus-only expansion, which restores
  columns from the project-wide remembered layout and could otherwise join another lane's pane
  (found by the end-to-end handoff test).
- Editors: no plugin change. Sync Tmux Layout, and the sync Claim for Tmux Pane / Force Claim /
  Load Tmux Window run after `agent-doc claim`, already publish `caller_kind = manual` with
  `client_id` (P3), which is the explicit claim. Every passive kind never moves a pane.
- Shared mode: no lane sessions are resolved, so no handoff; behavior is unchanged.
- Verification: SimWorld `two_ide_navigation_model` gains an explicit `Claim` event, pane counts,
  and per-lane projected documents, exhaustive to depth 5 over 12 events: handoff only on an
  explicit claim and always into the claimer's lane, exactly one pane per document, and no lane
  window showing a document whose pane lives elsewhere. New mutations `HandoffOnPassive`,
  `DuplicateOnHandoff`, `LoseOnHandoff`, and `NoRepublish` are all detected. Unit tests cover the
  claim kinds, the explicit partition (plus a proptest), lane routing with handoffs and refusal,
  and the passive republish. `make tmux-ci` adds an isolated-server test of
  `hand_over_navigation_lane_pane` (placeholder, idempotence, stale pane, shared-session source,
  foreign-name refusal both ways) and an end-to-end test through the real sync body: passive
  rounds keep the contended pane in IDEA's lane, Rider's explicit claim moves it, passive rounds
  never move it back, IDEA's explicit claim returns it, no pane is lost or visible in two lanes,
  and the shared session is untouched.
- Not done (moved to P5): route, start and `agent-doc claim` still provision into the shared lane
  (a pane they start reaches an installation lane through that lane's next explicit claim, which
  the plugin's Claim action performs); the `sync_tmux_layout` command and the recycle republish
  still publish into the shared lane; the per-lane `pane_layout_status` channel; the optional
  focus-driven handoff (not built; it would make passive focus move panes, so it needs its own
  opt-in).

Original scope:

- Default rule from P2: the pane stays in the lane that owns it.
- Explicit handoff: `Claim for Tmux Pane` or manual `Sync Tmux Layout` in the other
  IDE moves the pane (`join-pane` across sessions). It records the ownership
  transfer in the actor store and publishes a fresh epoch to both lanes, so the old
  lane drops the column.
- Optional focus-driven handoff: a per-document `InstallationFocus` fold, which is
  `#idefocusowner` at document scope. A focus claim on a contended document moves
  its pane to the focused installation's lane.
- Verification: SimWorld covers contended handoff, including the invariant that one
  pane belongs to exactly one lane. An isolated tmux test covers `join-pane`.

### P5: rollout, GC, and migration (PARTLY LANDED)

What landed (the original scope follows below):
- Pure (editor-surface): `plan_navigation_lane_reconcile(mode, project_session,
  project_session_alive, lanes)` over `LaneSessionObservation` (session, lane mark, live surface,
  panes with `document_owned` / `busy`), returning `FoldPane` / `Close` / `Retain { reason }`
  (`not_marked`, `live_surface`, `owned_panes`, `busy_unowned_pane`, `project_session_missing`).
  Only sessions named as installation lanes of the pin are planned; the project session never is.
- Where existing panes go: enabling `per_installation` moves nothing (shared panes stay in the
  project session, which stays the lane of unscoped clients; installation lanes start empty).
  Disabling folds each document pane of each marked lane session into the project session's
  `stash` window (same pane id and actor binding), from where the shared sync places it, then
  closes the lane unless a busy unowned pane or an attached client keeps it; with no live project
  session nothing moves.
- Cleanup: a marked lane with no document pane, no busy pane, and no live surface (an attached
  tmux client, or an installation the caller reports live) is closed with `kill-session`, after a
  re-observation confirms the plan. Unmarked sessions are retained and never read
  (`#bare-foreign-session-guard`). A registry read failure observes nothing. The shells agent-doc
  creates in a lane carry `@agent_doc_navigation_lane_placeholder`, and are idle until a client
  first attaches to the lane, whatever their starting shell runs (`#lanegcflake`: a lane GC'd while
  its fresh shell still ran rc commands was kept as `busy_unowned_pane`).
- IO: `project_controller::reconcile_navigation_lane_sessions(tmux, root, live_installations,
  dry_run)`; `agent-doc gc` runs it after its file GC (CLI passes no live installations, so an
  attached terminal is its liveness proof) and prints each planned action; `--dry-run` changes
  nothing. ops.log: `navigation_lane_pane_folded`, `navigation_lane_session_closed`.
- Verification: unit tests and a proptest of the planner (no fold under `per_installation`; a
  closed lane never held a busy unowned or unfolded document pane; unmarked never touched).
  SimWorld `two_ide_navigation_model` gains `SetMode` (a controller restart, so no retained layout
  survives) and `Gc` events driving the production planner, a foreign idle lane-named session, and
  lane-session provisioning, exhaustive to depth 5 over 15 events; new invariants (foreign session
  never closed, a live installation's lane never closed, a disable's gc leaves no document pane in
  a lane, plus the existing no-loss/no-duplicate/no-cross-lane checks across mode switches) and
  mutations `MoveOnEnable`, `GcIgnoresMark`, `GcIgnoresOwnedPanes`, `GcIgnoresLiveSurface`, all
  detected. `tests/cross_editor_simworld.rs`
  `two_ide_navigation_lanes_migrate_and_clean_up_end_to_end` (`make tmux-ci`, isolated server):
  IDEA, Rider, and a retired CLion client; enable moves no pane; gc closes only CLion's empty lane,
  keeps IDEA's (document pane) and Rider's (live) lanes and a foreign lane-named session; disable
  folds both lanes' document panes into the shared stash (same ids, once each), closes both lanes,
  and a second pass is a no-op.
- Docs: README, `specs/07-session-tmux-commands.md`, `specs/07-core-commands.md` (gc), and
  `editors/jetbrains/SPEC.md`.
- Still open: (a) route, start, and `agent-doc claim --client-id` creating panes in the
  requester's lane; (b) the `sync_tmux_layout` command and the recycle republish publishing into
  the right lane(s); (c) a per-lane `pane_layout_status` channel; a controller-side trigger that
  passes its live installations to the reconcile (today only `agent-doc gc` runs it). The optional
  focus-driven handoff is not planned.

Original scope:

Carried over from P3/P4: lane awareness of route, start, `agent-doc claim` (a `--client-id`
so a new pane starts in the requesting lane), the `sync_tmux_layout` command and the recycle
republish; a per-lane `pane_layout_status` channel; optionally an opt-in focus-driven handoff.

- Enabling `per_installation` on a project with an existing shared window: existing
  panes stay in the shared session, which remains the lane of unscoped clients, and
  each installation lane starts empty and acquires panes through route, start, or
  claim. No pane moves on enable.
- Disabling: `resync` adopts lane-session panes back into the shared session, since
  all lane sessions are allowed. Lane sessions with no panes are closed by `gc`.
- An installation that retires for good: its lane session is retained until `gc`
  proves no live surface and no agent-doc-owned pane remains, under the
  `#bare-foreign-session-guard` rule (never reap what agent-doc did not start).
- Docs: README, SPEC, and `editors/jetbrains/SPEC.md` describe the feature as
  available. A two-IDE end-to-end test in `tests/cross_editor_simworld.rs` uses an
  isolated tmux server and two simulated editor clients.

## Risks

- (Resolved in P2b.) `pane_layout_target_window_id` and the drift survey read the
  project pin; installation lanes now use `pane_layout_target_window_id_for_lane`
  and `tmux_layout_sync_state_for_lane_invocation`.
- An operator terminal attached to the shared session sees none of the lane
  sessions. P3's attach command is the supported path.
- (Resolved in P2b by per-lane workers and per-lane generation binding.) The
  structural-effect cancellation generation
  (`agent_doc_process_owner_io::set_structural_effect_generation`, bound to
  `published_generation_handle()`) was process-global, and the pane-layout effect
  worker plus its `ControllerPaneLayoutProjectionSink` are installed once per
  runtime. A newer generation in one lane must not cancel another lane's sync
  body. P2 should either key that generation by lane or drive every lane from one
  worker that drains lanes in order. The `.agent-doc/sync.lock` flock already
  serializes the tmux bodies.
- About 20 `ControllerRuntime` accessors delegate to the single
  `pane_layout_graph`. P2 can split into P2a and P2b. P2a moves the graph behind a
  lane map in which `Shared` is the only entry, which changes no behavior. P2b adds
  the config switch, lane routing, and provisioning.
