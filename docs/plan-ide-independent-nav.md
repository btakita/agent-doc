# Plan: independent tmux navigation per JetBrains installation (`#ideriderindependentnav`)

Status: P1 landed on branch `ridernav`; P2a landed on branch `ridernavp2a`. P2b through P5 are open.

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

### P2b: opt-in config, lane routing, and lane session provisioning

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

### P3: editor wiring (FFI-first, then JetBrains)

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

### P4: contended documents (one document visible in both IDEs)

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

### P5: rollout, GC, and migration

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

- `pane_layout_target_window_id` and the drift survey still read the project pin
  (`specs/08-session-routing.md`, "Deferred"). P2 must make both lane-aware before
  enabling the mode, or a lane's survey observes the wrong session.
- An operator terminal attached to the shared session sees none of the lane
  sessions. P3's attach command is the supported path.
- The structural-effect cancellation generation
  (`agent_doc_process_owner_io::set_structural_effect_generation`, bound to
  `published_generation_handle()`) is process-global, and the pane-layout effect
  worker plus its `ControllerPaneLayoutProjectionSink` are installed once per
  runtime. A newer generation in one lane must not cancel another lane's sync
  body. P2 should either key that generation by lane or drive every lane from one
  worker that drains lanes in order. The `.agent-doc/sync.lock` flock already
  serializes the tmux bodies.
- About 20 `ControllerRuntime` accessors delegate to the single
  `pane_layout_graph`. P2 can split into P2a and P2b. P2a moves the graph behind a
  lane map in which `Shared` is the only entry, which changes no behavior. P2b adds
  the config switch, lane routing, and provisioning.
